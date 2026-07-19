// SPDX-License-Identifier: GPL-3.0-or-later
//! The embedded assistant's chat core (M7 slice 3): bounded history, the
//! system prompt, and the chat loop connecting providers (slice 2) to the
//! tool executor (slice 1). The assistant NEVER writes to the ECU: the only
//! apply path is the user clicking Apply in the UI, which uses the same
//! `set_cells` path as AutoTune.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::ai_provider::{
    AssistantBlock, ChatMessage, ChatRequest, Provider, StopReason, ToolDef, ToolResultMsg,
};
use crate::ai_tools::AiToolExecutor;

/// Hard cap on tool round-trips within one user turn (runaway-loop guard).
pub const MAX_TOOL_ITERATIONS: usize = 8;
/// Hard cap on retained conversation messages (context guard).
/// ponytail: naive head-eviction — token-aware trimming if models hit limits.
pub const MAX_HISTORY_MESSAGES: usize = 40;
/// A realtime snapshot older than this is annotated stale (issue #27).
pub const REALTIME_STALE_MS: u64 = 2000;

/// Bounded conversation history. Eviction drops the oldest message; the
/// system prompt is NOT stored here (rebuilt per request).
#[derive(Debug, Default)]
pub struct ChatHistory {
    messages: Vec<ChatMessage>,
}

impl ChatHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
        while self.messages.len() > MAX_HISTORY_MESSAGES {
            self.messages.remove(0);
        }
        // I1: the single-item eviction above can stop mid tool-pair,
        // leaving a dangling Assistant or ToolResults message at the head
        // — not a legal provider-facing history prefix (both Anthropic and
        // OpenAI expect the conversation to open with a user turn), and a
        // stray ToolResults head would reference tool_use ids no longer in
        // history. Keep dropping from the head until it's a User message
        // (or history is empty).
        while !matches!(self.messages.first(), None | Some(ChatMessage::User { .. })) {
            self.messages.remove(0);
        }
    }

    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

/// The assistant's operating contract. Lists the available tools by name
/// and states the advisory-level rules in plain language. Numbers come
/// from tools, never from the model (ADR-0008 thin-AI rule).
pub fn system_prompt(tools: &[opentune_ai::ToolSpec]) -> String {
    let tool_lines: String = tools
        .iter()
        .map(|t| format!("- {}: {}\n", t.name, t.description))
        .collect();
    format!(
        "You are OpenTune's tuning assistant, running at the advisory \
         authority level inside an engine-tuning application.\n\
         \n\
         Available tools:\n{tool_lines}\
         \n\
         Rules you must follow:\n\
         - All numeric analysis comes from tools. Never invent or compute \
           tuning numbers yourself.\n\
         - You can NEVER apply changes to the ECU. To suggest a change, call \
           propose_change; the user reviews and applies it manually. Never \
           claim a change was applied.\n\
         - read_realtime results include ageMs (snapshot age in \
           milliseconds). If a result is marked stale, or ageMs is older \
           than about 2 seconds, say so and do not treat it as current.\n\
         - If a tool returns an error or a proposal comes back not-ok, \
           report the reason honestly.\n\
         - Be concise; the user may be tuning a running engine."
    )
}

/// Issue #27: annotate a read_realtime tool result whose snapshot is older
/// than [`REALTIME_STALE_MS`] so the model cannot mistake it for current
/// data. `null` results (no frame yet) are left untouched.
pub fn annotate_realtime_staleness(result: &mut serde_json::Value) {
    let Some(age) = result.get("ageMs").and_then(|v| v.as_u64()) else {
        return;
    };
    if age > REALTIME_STALE_MS {
        result["stale"] = serde_json::Value::Bool(true);
    }
}

/// Progress events the chat loop emits as it runs. Plain Rust — Task 4
/// wires the specta event DTO that carries these over IPC.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatEvent {
    Delta {
        text: String,
    },
    ToolStart {
        name: String,
    },
    ToolEnd {
        name: String,
        ok: bool,
        summary: String,
    },
    ProposalReady {
        id: u32,
    },
    Done,
    Cancelled,
    Error {
        message: String,
    },
}

/// Run one user turn to completion: push the user message, then loop
/// provider calls and tool round-trips (bounded by [`MAX_TOOL_ITERATIONS`])
/// until the model ends its turn, errors, or the cap is hit. The assistant
/// NEVER writes to the ECU here — every tool call goes through
/// `executor.execute`, which is gated by the same policy/guardrail/audit
/// path as every other AI entry point (ADR-0008).
#[allow(clippy::too_many_arguments)]
pub async fn run_chat_turn(
    provider: &Provider,
    executor: &AiToolExecutor,
    history: &mut ChatHistory,
    tools: &[ToolDef],
    system: &str,
    model: &str,
    max_tokens: u32,
    user_text: String,
    cancel: &AtomicBool,
    emit: &(dyn Fn(ChatEvent) + Send + Sync),
) -> Result<(), String> {
    history.push(ChatMessage::User { text: user_text });

    for _ in 0..MAX_TOOL_ITERATIONS {
        // ponytail: flag checks between steps — mid-stream cancel needs
        // select!; add if users ask.
        if cancel.load(Ordering::SeqCst) {
            emit(ChatEvent::Cancelled);
            return Ok(());
        }

        let req = ChatRequest {
            system: system.to_owned(),
            messages: history.messages().to_vec(),
            tools: tools.to_vec(),
            model: model.to_owned(),
            max_tokens,
        };
        let mut on_delta = |text: &str| {
            emit(ChatEvent::Delta {
                text: text.to_owned(),
            })
        };
        let turn = match provider.chat(&req, &mut on_delta).await {
            Ok(turn) => turn,
            Err(err) => {
                emit(ChatEvent::Error {
                    message: err.to_string(),
                });
                return Err(err.to_string());
            }
        };

        history.push(ChatMessage::Assistant {
            blocks: turn.blocks.clone(),
        });

        match &turn.stop_reason {
            StopReason::EndTurn => {
                emit(ChatEvent::Done);
                return Ok(());
            }
            StopReason::MaxTokens => {
                push_pending_tool_results(
                    history,
                    &turn.blocks,
                    Vec::new(),
                    "not executed (max_tokens)",
                );
                emit(ChatEvent::Error {
                    message: "stopped: the model hit its max-tokens limit".into(),
                });
                return Ok(());
            }
            StopReason::Other(reason) => {
                push_pending_tool_results(
                    history,
                    &turn.blocks,
                    Vec::new(),
                    &format!("not executed ({reason})"),
                );
                emit(ChatEvent::Error {
                    message: format!("stopped: {reason}"),
                });
                return Ok(());
            }
            StopReason::ToolUse => {
                let mut results = Vec::with_capacity(turn.blocks.len());
                for block in &turn.blocks {
                    let AssistantBlock::ToolUse { id, name, input } = block else {
                        continue;
                    };
                    if cancel.load(Ordering::SeqCst) {
                        push_pending_tool_results(
                            history,
                            &turn.blocks,
                            results,
                            "cancelled by user",
                        );
                        emit(ChatEvent::Cancelled);
                        return Ok(());
                    }
                    results.push(execute_tool_block(executor, id, name, input, emit).await);
                }
                history.push(ChatMessage::ToolResults { results });
            }
        }
    }

    emit(ChatEvent::Error {
        message: format!(
            "stopped: exceeded the {MAX_TOOL_ITERATIONS}-iteration tool-call cap for one turn"
        ),
    });
    Ok(())
}

/// C1: push a `ChatMessage::ToolResults` covering every `ToolUse` id in
/// `blocks`, if any — used by exit paths that have already pushed an
/// Assistant message containing tool calls but then leave before executing
/// all of them (a mid-loop cancel, or a `MaxTokens`/`Other` stop that ends
/// the turn without ever reaching the `ToolUse` match arm). Both real
/// providers reject the *next* send if history ends with an Assistant
/// message whose `tool_use` blocks have no matching result: Anthropic
/// requires a `tool_result` in the following user message, OpenAI requires
/// a `role: "tool"` message per `tool_call`. `collected` carries any real
/// results already produced (e.g. earlier tool calls in the same message
/// that ran before a mid-loop cancel); every remaining id is synthesized as
/// `is_error: true` with `reason` as its content, in block order. A no-op
/// when `blocks` has no `ToolUse` block, so a plain-text `MaxTokens`/cancel
/// exit never appends a spurious empty `ToolResults` message.
fn push_pending_tool_results(
    history: &mut ChatHistory,
    blocks: &[AssistantBlock],
    collected: Vec<ToolResultMsg>,
    reason: &str,
) {
    let tool_use_ids: Vec<&str> = blocks
        .iter()
        .filter_map(|b| match b {
            AssistantBlock::ToolUse { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    if tool_use_ids.is_empty() {
        return;
    }
    let results = tool_use_ids
        .into_iter()
        .map(|id| {
            collected
                .iter()
                .find(|r| r.tool_use_id == id)
                .cloned()
                .unwrap_or_else(|| ToolResultMsg {
                    tool_use_id: id.to_owned(),
                    content: reason.to_owned(),
                    is_error: true,
                })
        })
        .collect();
    history.push(ChatMessage::ToolResults { results });
}

/// Execute one `ToolUse` block: emits `ToolStart`/`ToolEnd`, annotates
/// `read_realtime` results for staleness before they go back to the model,
/// and surfaces `propose_change` proposals via `ProposalReady`. Tool errors
/// become `is_error: true` results — they are returned to the model, never
/// used to abort the loop.
async fn execute_tool_block(
    executor: &AiToolExecutor,
    tool_use_id: &str,
    name: &str,
    input: &serde_json::Value,
    emit: &(dyn Fn(ChatEvent) + Send + Sync),
) -> ToolResultMsg {
    emit(ChatEvent::ToolStart {
        name: name.to_owned(),
    });
    match executor.execute(name, input.clone()).await {
        Ok(mut result) => {
            if name == "read_realtime" {
                annotate_realtime_staleness(&mut result);
            }
            if name == "propose_change" {
                if let Some(proposal_id) = result.get("id").and_then(|v| v.as_u64()) {
                    emit(ChatEvent::ProposalReady {
                        id: proposal_id as u32,
                    });
                }
            }
            emit(ChatEvent::ToolEnd {
                name: name.to_owned(),
                ok: true,
                summary: tool_ok_summary(name, &result),
            });
            ToolResultMsg {
                tool_use_id: tool_use_id.to_owned(),
                content: result.to_string(),
                is_error: false,
            }
        }
        Err(err) => {
            emit(ChatEvent::ToolEnd {
                name: name.to_owned(),
                ok: false,
                summary: err.message.clone(),
            });
            ToolResultMsg {
                tool_use_id: tool_use_id.to_owned(),
                content: err.message,
                is_error: true,
            }
        }
    }
}

/// A compact one-line summary for a successful tool call's `ToolEnd` event.
/// `propose_change` names the proposal id/verdict; everything else is `"ok"`.
fn tool_ok_summary(name: &str, result: &serde_json::Value) -> String {
    if name != "propose_change" {
        return "ok".to_owned();
    }
    let id = result.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
    let ok = result.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    format!("proposal #{id} ok={ok}")
}

#[cfg(test)]
mod core_tests {
    use super::*;
    use crate::ai_provider::ChatMessage;

    #[test]
    fn history_evicts_oldest_beyond_cap() {
        let mut h = ChatHistory::new();
        for i in 0..(MAX_HISTORY_MESSAGES + 5) {
            h.push(ChatMessage::User {
                text: format!("m{i}"),
            });
        }
        assert_eq!(h.len(), MAX_HISTORY_MESSAGES);
        match &h.messages()[0] {
            ChatMessage::User { text } => assert_eq!(text, "m5"),
            other => panic!("unexpected head: {other:?}"),
        }
        h.clear();
        assert!(h.is_empty());
    }

    #[test]
    fn eviction_never_leaves_a_non_user_head() {
        // I1: naive single-item head-eviction can stop mid tool-pair,
        // leaving a dangling Assistant or ToolResults message at index 0 —
        // illegal as a provider-facing history prefix (both Anthropic and
        // OpenAI expect the conversation to open with a user turn), and a
        // stray ToolResults head references tool_use ids no longer present
        // in history. 40 (the cap) is not a multiple of 3, so repeatedly
        // pushing User/Assistant/ToolResults triples and evicting one
        // message at a time drifts the head across all three message kinds
        // over time — this reliably lands on a non-User head without the
        // fix.
        let mut h = ChatHistory::new();
        for i in 0..MAX_HISTORY_MESSAGES {
            h.push(ChatMessage::User {
                text: format!("u{i}"),
            });
            h.push(ChatMessage::Assistant { blocks: vec![] });
            h.push(ChatMessage::ToolResults { results: vec![] });
        }
        assert!(
            h.len() <= MAX_HISTORY_MESSAGES,
            "still within the cap: {}",
            h.len()
        );
        match h.messages().first() {
            Some(ChatMessage::User { .. }) => {}
            other => panic!("head must be a User message (or history empty), got {other:?}"),
        }
    }

    #[test]
    fn system_prompt_names_tools_and_advisory_rules() {
        let policy = opentune_ai::PermissionPolicy::advisory();
        let tools = opentune_ai::available_tools(&policy);
        let prompt = system_prompt(&tools);
        assert!(prompt.contains("read_tune"));
        assert!(prompt.contains("propose_change"));
        assert!(
            !prompt.contains("apply_change"),
            "locked tools are not advertised"
        );
        assert!(
            prompt.to_lowercase().contains("never appl"),
            "advisory rule stated"
        );
        assert!(prompt.contains("ageMs"), "staleness rule stated");
    }

    #[test]
    fn realtime_staleness_annotation() {
        let mut fresh = serde_json::json!({ "channels": [], "ageMs": 100 });
        annotate_realtime_staleness(&mut fresh);
        assert!(fresh.get("stale").is_none());
        let mut stale = serde_json::json!({ "channels": [], "ageMs": 5000 });
        annotate_realtime_staleness(&mut stale);
        assert_eq!(stale["stale"], true);
        let mut none = serde_json::json!(null); // snapshot absent
        annotate_realtime_staleness(&mut none);
        assert!(none.get("stale").is_none());
    }
}

#[cfg(test)]
#[path = "ai_chat_tests.rs"]
mod loop_tests;
