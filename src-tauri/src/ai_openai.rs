// SPDX-License-Identifier: GPL-3.0-or-later

//! OpenAI Chat Completions provider: request builder, SSE stream assembler,
//! and the HTTP `chat` entry point. Wire contract verified against current
//! OpenAI docs (platform.openai.com API reference / OpenAPI spec) — see
//! task-5 report for details.

use futures_util::StreamExt;

use crate::ai_provider::{
    AssistantBlock, ChatMessage, ChatRequest, ChatTurn, OnDelta, ProviderError, StopReason,
    ToolDef, ToolResultMsg,
};

const OPENAI_CHAT_COMPLETIONS_URL: &str = "https://api.openai.com/v1/chat/completions";

/// OpenAI provider. `api_key` is only ever placed in the `Authorization:
/// Bearer` header — never logged, never embedded in error messages. `Debug`
/// is hand-written to redact it.
pub struct OpenAiProvider {
    pub api_key: String,
}

impl std::fmt::Debug for OpenAiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OpenAiProvider {{ api_key: \"<redacted>\" }}")
    }
}

impl OpenAiProvider {
    /// Send a chat request and stream text deltas through `on_delta`.
    ///
    /// Builds the request body, POSTs it with the Bearer auth header, and —
    /// on success — parses the `text/event-stream` response through
    /// [`SseAssembler`]. On a non-success HTTP status, the response body is
    /// passed through verbatim in [`ProviderError::Http`] (it is the
    /// provider's own error payload and cannot contain our key, since the
    /// key is never echoed back).
    pub async fn chat(
        &self,
        req: &ChatRequest,
        on_delta: OnDelta<'_>,
    ) -> Result<ChatTurn, ProviderError> {
        let body = build_request_body(req);
        let client = reqwest::Client::new();
        let resp = client
            .post(OPENAI_CHAT_COMPLETIONS_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let message = resp
                .text()
                .await
                .unwrap_or_else(|_| "failed to read error response body".to_string());
            return Err(ProviderError::Http { status, message });
        }

        let mut assembler = SseAssembler::default();
        let mut line_buffer: Vec<u8> = Vec::new();
        let mut stream = resp.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| ProviderError::Network(e.to_string()))?;
            line_buffer.extend_from_slice(&chunk);
            if consume_lines(&mut line_buffer, &mut assembler, on_delta)? {
                break;
            }
        }

        assembler.finish()
    }
}

/// Drain complete lines from `line_buffer` (everything up to and including
/// each `\n`), feeding `data:` payloads into `assembler`. Returns `Ok(true)`
/// as soon as the `[DONE]` sentinel is seen (remaining buffered bytes, if
/// any, are simply not processed further). Splitting on the raw byte `\n` is
/// UTF-8 safe even across chunk boundaries: `\n` (0x0A) never appears as a
/// continuation byte of a multi-byte UTF-8 sequence, so a complete line's
/// bytes are always valid to decode once fully buffered.
fn consume_lines(
    line_buffer: &mut Vec<u8>,
    assembler: &mut SseAssembler,
    on_delta: OnDelta<'_>,
) -> Result<bool, ProviderError> {
    while let Some(newline_pos) = line_buffer.iter().position(|&b| b == b'\n') {
        let line_bytes: Vec<u8> = line_buffer.drain(..=newline_pos).collect();
        let line = String::from_utf8_lossy(&line_bytes);
        let line = line.trim_end_matches(['\r', '\n']);

        if line.is_empty() {
            continue;
        }
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if assembler.feed(data, on_delta)? {
                return Ok(true);
            }
        }
        // Other SSE fields (comments, unexpected field names) are ignored.
    }
    Ok(false)
}

/// Build the `POST /v1/chat/completions` request body per the wire
/// contract. Pure — no network, no secrets (the key never enters the body).
pub(crate) fn build_request_body(req: &ChatRequest) -> serde_json::Value {
    let mut messages: Vec<serde_json::Value> = Vec::with_capacity(req.messages.len() + 1);
    messages.push(serde_json::json!({
        "role": "system",
        "content": req.system,
    }));
    for msg in &req.messages {
        messages.extend(message_to_json(msg));
    }
    let tools: Vec<serde_json::Value> = req.tools.iter().map(tool_to_json).collect();
    serde_json::json!({
        "model": req.model,
        "stream": true,
        "max_completion_tokens": req.max_tokens,
        "messages": messages,
        "tools": tools,
    })
}

fn tool_to_json(tool: &ToolDef) -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema,
        },
    })
}

/// Map one [`ChatMessage`] to zero or more wire messages. `ToolResults`
/// expands to one `{"role":"tool", ...}` message per result — everything
/// else maps one-to-one.
fn message_to_json(msg: &ChatMessage) -> Vec<serde_json::Value> {
    match msg {
        ChatMessage::User { text } => vec![serde_json::json!({
            "role": "user",
            "content": text,
        })],
        ChatMessage::Assistant { blocks } => vec![assistant_message_to_json(blocks)],
        ChatMessage::ToolResults { results } => results.iter().map(tool_result_to_json).collect(),
    }
}

fn assistant_message_to_json(blocks: &[AssistantBlock]) -> serde_json::Value {
    let text: String = blocks
        .iter()
        .filter_map(|block| match block {
            AssistantBlock::Text { text } => Some(text.as_str()),
            AssistantBlock::ToolUse { .. } => None,
        })
        .collect();
    let content = if text.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(text)
    };

    let tool_calls: Vec<serde_json::Value> = blocks
        .iter()
        .filter_map(|block| match block {
            AssistantBlock::ToolUse { id, name, input } => Some(serde_json::json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    // Arguments travel as a STRING of JSON on the wire, not
                    // an object — serializing a `Value` to a string cannot
                    // fail.
                    "arguments": serde_json::to_string(input)
                        .expect("serde_json::Value serialization is infallible"),
                },
            })),
            AssistantBlock::Text { .. } => None,
        })
        .collect();

    let mut message = serde_json::json!({
        "role": "assistant",
        "content": content,
    });
    if !tool_calls.is_empty() {
        message["tool_calls"] = serde_json::Value::Array(tool_calls);
    }
    message
}

fn tool_result_to_json(result: &ToolResultMsg) -> serde_json::Value {
    serde_json::json!({
        "role": "tool",
        "tool_call_id": result.tool_use_id,
        "content": result.content,
    })
}

/// A tool call still accumulating streamed argument fragments, keyed by its
/// `index` in the response. `id`/`name` normally arrive on the first
/// fragment for a given index; `arguments` is a JSON string built by
/// concatenating every fragment in arrival order.
#[derive(Debug, Default)]
struct PendingCall {
    id: String,
    name: String,
    arguments: String,
}

/// Assembles a sequence of OpenAI SSE `data:` payloads into a [`ChatTurn`].
/// Unlike Anthropic, every line carries the same shape (`choices[0].delta`)
/// — there are no named event types. Tool calls stream as fragments
/// (`{index, id?, function: {name?, arguments-fragment?}}`) accumulated by
/// `index` in a `BTreeMap` so `finish()` yields them in deterministic
/// ascending-index order regardless of arrival sequence.
#[derive(Debug, Default)]
pub(crate) struct SseAssembler {
    text: String,
    calls: std::collections::BTreeMap<u64, PendingCall>,
    finish_reason: Option<StopReason>,
    done: bool,
}

impl SseAssembler {
    /// Feed one `data:` payload. Returns `Ok(true)` when `data` is the
    /// literal `[DONE]` sentinel (and is otherwise idempotent once `[DONE]`
    /// has been seen); otherwise parses `data` as JSON and folds
    /// `delta.content`, `delta.tool_calls`, and `finish_reason`.
    pub(crate) fn feed(
        &mut self,
        data: &str,
        on_delta: OnDelta<'_>,
    ) -> Result<bool, ProviderError> {
        if self.done || data == "[DONE]" {
            self.done = true;
            return Ok(true);
        }

        let value: serde_json::Value = serde_json::from_str(data)
            .map_err(|e| ProviderError::Protocol(format!("invalid SSE JSON payload: {e}")))?;

        if let Some(content) = value["choices"][0]["delta"]["content"].as_str() {
            on_delta(content);
            self.text.push_str(content);
        }

        if let Some(tool_calls) = value["choices"][0]["delta"]["tool_calls"].as_array() {
            for fragment in tool_calls {
                self.fold_tool_call_fragment(fragment)?;
            }
        }

        if let Some(reason) = value["choices"][0]["finish_reason"].as_str() {
            self.finish_reason = Some(map_finish_reason(reason));
        }

        Ok(false)
    }

    fn fold_tool_call_fragment(
        &mut self,
        fragment: &serde_json::Value,
    ) -> Result<(), ProviderError> {
        let index = fragment["index"].as_u64().ok_or_else(|| {
            ProviderError::Protocol("tool_calls fragment missing numeric index".into())
        })?;
        let call = self.calls.entry(index).or_default();
        if let Some(id) = fragment["id"].as_str() {
            call.id = id.to_string();
        }
        if let Some(name) = fragment["function"]["name"].as_str() {
            call.name = name.to_string();
        }
        if let Some(arguments) = fragment["function"]["arguments"].as_str() {
            call.arguments.push_str(arguments);
        }
        Ok(())
    }

    /// Finish the stream, requiring that a `finish_reason` was seen (OpenAI
    /// sends it on the final content chunk, before `[DONE]`). The
    /// accumulated text becomes a leading [`AssistantBlock::Text`] (when
    /// non-empty), followed by tool calls in ascending index order with
    /// their accumulated argument JSON parsed (empty string → `{}`).
    pub(crate) fn finish(self) -> Result<ChatTurn, ProviderError> {
        let stop_reason = self
            .finish_reason
            .ok_or_else(|| ProviderError::Protocol("stream ended without finish_reason".into()))?;

        let mut blocks = Vec::with_capacity(1 + self.calls.len());
        if !self.text.is_empty() {
            blocks.push(AssistantBlock::Text { text: self.text });
        }
        for call in self.calls.into_values() {
            let input = parse_tool_arguments(&call.arguments)?;
            blocks.push(AssistantBlock::ToolUse {
                id: call.id,
                name: call.name,
                input,
            });
        }

        Ok(ChatTurn {
            blocks,
            stop_reason,
        })
    }
}

fn map_finish_reason(reason: &str) -> StopReason {
    match reason {
        "stop" => StopReason::EndTurn,
        "tool_calls" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        other => StopReason::Other(other.to_string()),
    }
}

/// Parse an accumulated tool-call arguments JSON string. An empty string (a
/// tool called with no arguments never emits an `arguments` fragment) maps
/// to `{}`.
fn parse_tool_arguments(json: &str) -> Result<serde_json::Value, ProviderError> {
    let json = if json.is_empty() { "{}" } else { json };
    serde_json::from_str(json)
        .map_err(|e| ProviderError::Protocol(format!("invalid tool_calls arguments JSON: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_provider::{
        AssistantBlock, ChatMessage, ChatRequest, StopReason, ToolDef, ToolResultMsg,
    };

    fn req() -> ChatRequest {
        ChatRequest {
            system: "You are a tuner.".into(),
            messages: vec![
                ChatMessage::User {
                    text: "check afr".into(),
                },
                ChatMessage::Assistant {
                    blocks: vec![AssistantBlock::ToolUse {
                        id: "call_1".into(),
                        name: "read_tune".into(),
                        input: serde_json::json!({ "names": ["reqFuel"] }),
                    }],
                },
                ChatMessage::ToolResults {
                    results: vec![ToolResultMsg {
                        tool_use_id: "call_1".into(),
                        content: "{\"values\":[]}".into(),
                        is_error: false,
                    }],
                },
            ],
            tools: vec![ToolDef {
                name: "read_tune".into(),
                description: "d".into(),
                input_schema: serde_json::json!({ "type": "object", "properties": {} }),
            }],
            model: "gpt-x".into(),
            max_tokens: 4096,
        }
    }

    #[test]
    fn request_body_matches_wire_contract() {
        let body = build_request_body(&req());
        assert_eq!(body["model"], "gpt-x");
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_completion_tokens"], 4096);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(
            body["messages"][2]["tool_calls"][0]["function"]["name"],
            "read_tune"
        );
        // Arguments must be a STRING of JSON, not an object.
        assert!(body["messages"][2]["tool_calls"][0]["function"]["arguments"].is_string());
        assert_eq!(body["messages"][3]["role"], "tool");
        assert_eq!(body["messages"][3]["tool_call_id"], "call_1");
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "read_tune");
    }

    #[test]
    fn sse_assembler_builds_text_and_tool_call_turn() {
        let mut asm = SseAssembler::default();
        let mut deltas = String::new();
        let mut on_delta = |d: &str| deltas.push_str(d);
        assert!(!asm
            .feed(
                r#"{"choices":[{"delta":{"content":"Lean at "}}]}"#,
                &mut on_delta
            )
            .unwrap());
        assert!(!asm
            .feed(
                r#"{"choices":[{"delta":{"content":"4500rpm"}}]}"#,
                &mut on_delta
            )
            .unwrap());
        assert!(!asm
            .feed(
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_9","function":{"name":"run_ve_analyze","arguments":"{\"table\":"}}]}}]}"#,
                &mut on_delta
            )
            .unwrap());
        assert!(!asm
            .feed(
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"veTable1Tbl\"}"}}]},"finish_reason":null}]}"#,
                &mut on_delta
            )
            .unwrap());
        assert!(!asm
            .feed(
                r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
                &mut on_delta
            )
            .unwrap());
        assert!(asm.feed("[DONE]", &mut on_delta).unwrap());
        let turn = asm.finish().expect("complete turn");
        assert_eq!(deltas, "Lean at 4500rpm");
        assert_eq!(turn.stop_reason, StopReason::ToolUse);
        match &turn.blocks[1] {
            AssistantBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_9");
                assert_eq!(name, "run_ve_analyze");
                assert_eq!(input["table"], "veTable1Tbl");
            }
            other => panic!("expected tool call, got {other:?}"),
        }
    }

    #[test]
    fn missing_finish_reason_is_protocol_error() {
        let mut asm = SseAssembler::default();
        let mut on_delta = |_: &str| {};
        asm.feed(r#"{"choices":[{"delta":{"content":"hi"}}]}"#, &mut on_delta)
            .unwrap();
        asm.feed("[DONE]", &mut on_delta).unwrap();
        assert!(matches!(
            asm.finish(),
            Err(crate::ai_provider::ProviderError::Protocol(_))
        ));
    }

    #[test]
    fn debug_redacts_key() {
        let p = OpenAiProvider {
            api_key: "test-key".into(),
        };
        assert!(!format!("{p:?}").contains("test-key"));
    }
}
