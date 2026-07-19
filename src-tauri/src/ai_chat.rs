// SPDX-License-Identifier: GPL-3.0-or-later
//! The embedded assistant's chat core (M7 slice 3): bounded history, the
//! system prompt, and the chat loop connecting providers (slice 2) to the
//! tool executor (slice 1). The assistant NEVER writes to the ECU: the only
//! apply path is the user clicking Apply in the UI, which uses the same
//! `set_cells` path as AutoTune.

use crate::ai_provider::ChatMessage;

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
           milliseconds). If a result is marked stale or ageMs is large, \
           say so and do not treat it as current.\n\
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
