// SPDX-License-Identifier: GPL-3.0-or-later

//! Anthropic Messages API provider: request builder, SSE stream assembler,
//! and the HTTP `chat` entry point. Wire contract verified against current
//! Anthropic docs (platform.claude.com) — see task-4 report for details.

use futures_util::StreamExt;

use crate::ai_provider::{
    http_client, truncate_message, AssistantBlock, ChatMessage, ChatRequest, ChatTurn, OnDelta,
    ProviderError, StopReason, ToolDef, ToolResultMsg,
};

const ANTHROPIC_MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic provider. `api_key` is only ever placed in the `x-api-key`
/// header — never logged, never embedded in error messages. `Debug` is
/// hand-written to redact it.
pub struct AnthropicProvider {
    pub api_key: String,
}

impl std::fmt::Debug for AnthropicProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AnthropicProvider {{ api_key: \"<redacted>\" }}")
    }
}

impl AnthropicProvider {
    /// Send a chat request and stream text deltas through `on_delta`.
    ///
    /// Builds the request body, POSTs it with the three required headers,
    /// and — on success — parses the `text/event-stream` response through
    /// [`SseAssembler`]. On a non-success HTTP status, the response body is
    /// truncated (see [`crate::ai_provider::truncate_message`]) and carried
    /// in [`ProviderError::Http`] (it is the provider's own error payload
    /// and cannot contain our key, since the key is never echoed back).
    pub async fn chat(
        &self,
        req: &ChatRequest,
        on_delta: OnDelta<'_>,
    ) -> Result<ChatTurn, ProviderError> {
        let body = build_request_body(req);
        let resp = http_client()
            .post(ANTHROPIC_MESSAGES_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
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
            return Err(ProviderError::Http {
                status,
                message: truncate_message(message),
            });
        }

        let mut assembler = SseAssembler::default();
        let mut line_buffer: Vec<u8> = Vec::new();
        let mut current_event: Option<String> = None;
        let mut stream = resp.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| ProviderError::Network(e.to_string()))?;
            line_buffer.extend_from_slice(&chunk);
            consume_lines(
                &mut line_buffer,
                &mut current_event,
                &mut assembler,
                on_delta,
            )?;
        }

        assembler.finish()
    }
}

/// Drain complete lines from `line_buffer` (everything up to and including
/// each `\n`), feeding `event:`/`data:` pairs into `assembler`. Splitting on
/// the raw byte `\n` is UTF-8 safe even across chunk boundaries: `\n` (0x0A)
/// never appears as a continuation byte of a multi-byte UTF-8 sequence, so a
/// complete line's bytes are always valid to decode once fully buffered.
fn consume_lines(
    line_buffer: &mut Vec<u8>,
    current_event: &mut Option<String>,
    assembler: &mut SseAssembler,
    on_delta: OnDelta<'_>,
) -> Result<(), ProviderError> {
    while let Some(newline_pos) = line_buffer.iter().position(|&b| b == b'\n') {
        let line_bytes: Vec<u8> = line_buffer.drain(..=newline_pos).collect();
        let line = String::from_utf8_lossy(&line_bytes);
        let line = line.trim_end_matches(['\r', '\n']);

        if line.is_empty() {
            // Blank line resets the current event name per the SSE spec.
            *current_event = None;
            continue;
        }
        if let Some(event_name) = line.strip_prefix("event:") {
            *current_event = Some(event_name.trim().to_string());
            continue;
        }
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            let event_name = current_event.as_deref().unwrap_or("message");
            assembler.feed(event_name, data, on_delta)?;
        }
        // Other SSE fields (id:, retry:, comments) are ignored.
    }
    Ok(())
}

/// Build the `POST /v1/messages` request body per the wire contract. Pure —
/// no network, no secrets (the key never enters the body).
pub(crate) fn build_request_body(req: &ChatRequest) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = req.messages.iter().map(message_to_json).collect();
    let mut body = serde_json::json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "system": req.system,
        "stream": true,
        "messages": messages,
    });
    // Omitting `tools` when there are none is the documented shape (and
    // mirrors OpenAI, which returns HTTP 400 for `"tools": []`).
    if !req.tools.is_empty() {
        let tools: Vec<serde_json::Value> = req.tools.iter().map(tool_to_json).collect();
        body["tools"] = serde_json::Value::Array(tools);
    }
    body
}

fn tool_to_json(tool: &ToolDef) -> serde_json::Value {
    serde_json::json!({
        "name": tool.name,
        "description": tool.description,
        "input_schema": tool.input_schema,
    })
}

fn message_to_json(msg: &ChatMessage) -> serde_json::Value {
    match msg {
        ChatMessage::User { text } => serde_json::json!({
            "role": "user",
            "content": text,
        }),
        ChatMessage::Assistant { blocks } => {
            let content: Vec<serde_json::Value> =
                blocks.iter().map(assistant_block_to_json).collect();
            serde_json::json!({
                "role": "assistant",
                "content": content,
            })
        }
        ChatMessage::ToolResults { results } => {
            let content: Vec<serde_json::Value> = results.iter().map(tool_result_to_json).collect();
            serde_json::json!({
                "role": "user",
                "content": content,
            })
        }
    }
}

fn assistant_block_to_json(block: &AssistantBlock) -> serde_json::Value {
    match block {
        AssistantBlock::Text { text } => serde_json::json!({
            "type": "text",
            "text": text,
        }),
        AssistantBlock::ToolUse { id, name, input } => serde_json::json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": input,
        }),
    }
}

fn tool_result_to_json(result: &ToolResultMsg) -> serde_json::Value {
    serde_json::json!({
        "type": "tool_result",
        "tool_use_id": result.tool_use_id,
        "content": result.content,
        "is_error": result.is_error,
    })
}

/// A content block still accumulating streamed deltas.
#[derive(Debug)]
enum PendingBlock {
    Text(String),
    ToolUse {
        id: String,
        name: String,
        json: String,
    },
}

/// Assembles a sequence of Anthropic SSE `(event, data)` pairs into a
/// [`ChatTurn`]. Blocks are keyed by their `index` field in a `BTreeMap` so
/// `finish()` yields them in deterministic ascending-index order regardless
/// of the (already-ordered, but let's not rely on that) arrival sequence.
#[derive(Debug, Default)]
pub(crate) struct SseAssembler {
    blocks: std::collections::BTreeMap<u64, PendingBlock>,
    stop_reason: Option<StopReason>,
}

impl SseAssembler {
    /// Feed one complete SSE `(event, data)` pair; text deltas are forwarded
    /// to `on_delta` before being accumulated. `data` is parsed as JSON only
    /// for event types that need it — `message_start`/`message_stop`/`ping`
    /// carry no information this assembler uses, and unknown event types
    /// (see below) skip parsing entirely, so a non-JSON `data` payload on
    /// any of those is never an error.
    pub(crate) fn feed(
        &mut self,
        event: &str,
        data: &str,
        on_delta: OnDelta<'_>,
    ) -> Result<(), ProviderError> {
        match event {
            "message_start" | "message_stop" | "ping" => Ok(()),
            "content_block_start" => {
                let value = parse_event_json(data)?;
                self.handle_content_block_start(&value)
            }
            "content_block_delta" => {
                let value = parse_event_json(data)?;
                self.handle_content_block_delta(&value, on_delta)
            }
            "content_block_stop" => {
                let value = parse_event_json(data)?;
                self.handle_content_block_stop(&value)
            }
            "message_delta" => {
                let value = parse_event_json(data)?;
                self.handle_message_delta(&value);
                Ok(())
            }
            "error" => {
                let value = parse_event_json(data)?;
                let message = value["error"]["message"]
                    .as_str()
                    .unwrap_or("unknown provider error")
                    .to_string();
                Err(ProviderError::Protocol(message))
            }
            // Unknown events are ignored per Anthropic's versioning policy:
            // new event types may be added and clients must tolerate them.
            // Their `data` payload shape is unknown, so it is never parsed —
            // tolerance must extend to non-JSON data, not just JSON shapes
            // we don't recognize.
            _ => Ok(()),
        }
    }

    /// Finish the stream, requiring that a `message_delta` was seen (it
    /// carries the only `stop_reason`). Tool-use blocks have their
    /// accumulated JSON parsed here (empty string → `{}`).
    pub(crate) fn finish(self) -> Result<ChatTurn, ProviderError> {
        let stop_reason = self
            .stop_reason
            .ok_or_else(|| ProviderError::Protocol("stream ended without message_delta".into()))?;

        let mut blocks = Vec::with_capacity(self.blocks.len());
        for pending in self.blocks.into_values() {
            blocks.push(match pending {
                PendingBlock::Text(text) => AssistantBlock::Text { text },
                PendingBlock::ToolUse { id, name, json } => {
                    let input = parse_tool_input(&json)?;
                    AssistantBlock::ToolUse { id, name, input }
                }
            });
        }

        Ok(ChatTurn {
            blocks,
            stop_reason,
        })
    }

    fn handle_content_block_start(
        &mut self,
        value: &serde_json::Value,
    ) -> Result<(), ProviderError> {
        let index = index_of(value)?;
        let block_type = value["content_block"]["type"].as_str().unwrap_or("");
        let pending = match block_type {
            "text" => PendingBlock::Text(String::new()),
            "tool_use" => {
                let id = value["content_block"]["id"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let name = value["content_block"]["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                PendingBlock::ToolUse {
                    id,
                    name,
                    json: String::new(),
                }
            }
            other => {
                return Err(ProviderError::Protocol(format!(
                    "unsupported content_block type: {other}"
                )));
            }
        };
        self.blocks.insert(index, pending);
        Ok(())
    }

    fn handle_content_block_delta(
        &mut self,
        value: &serde_json::Value,
        on_delta: OnDelta<'_>,
    ) -> Result<(), ProviderError> {
        let index = index_of(value)?;
        let delta_type = value["delta"]["type"].as_str().unwrap_or("");
        let block = self.blocks.get_mut(&index).ok_or_else(|| {
            ProviderError::Protocol(format!("content_block_delta for unknown index {index}"))
        })?;
        match (block, delta_type) {
            (PendingBlock::Text(text), "text_delta") => {
                let chunk = value["delta"]["text"].as_str().unwrap_or("");
                on_delta(chunk);
                text.push_str(chunk);
            }
            (PendingBlock::ToolUse { json, .. }, "input_json_delta") => {
                let chunk = value["delta"]["partial_json"].as_str().unwrap_or("");
                json.push_str(chunk);
            }
            (_, other) => {
                return Err(ProviderError::Protocol(format!(
                    "unexpected delta type \"{other}\" for block at index {index}"
                )));
            }
        }
        Ok(())
    }

    fn handle_content_block_stop(
        &mut self,
        value: &serde_json::Value,
    ) -> Result<(), ProviderError> {
        let index = index_of(value)?;
        let block = self.blocks.get(&index).ok_or_else(|| {
            ProviderError::Protocol(format!("content_block_stop for unknown index {index}"))
        })?;
        if let PendingBlock::ToolUse { json, .. } = block {
            // Validate now so malformed input surfaces as soon as the block
            // closes rather than being deferred to `finish()`.
            parse_tool_input(json)?;
        }
        Ok(())
    }

    fn handle_message_delta(&mut self, value: &serde_json::Value) {
        let Some(stop_reason) = value["delta"]["stop_reason"].as_str() else {
            return;
        };
        self.stop_reason = Some(match stop_reason {
            "end_turn" => StopReason::EndTurn,
            "tool_use" => StopReason::ToolUse,
            "max_tokens" => StopReason::MaxTokens,
            other => StopReason::Other(other.to_string()),
        });
    }
}

/// Parse an accumulated tool-input JSON string. An empty string (a tool
/// called with no arguments never emits an `input_json_delta`) maps to `{}`.
fn parse_tool_input(json: &str) -> Result<serde_json::Value, ProviderError> {
    let json = if json.is_empty() { "{}" } else { json };
    serde_json::from_str(json)
        .map_err(|e| ProviderError::Protocol(format!("invalid tool_use input JSON: {e}")))
}

/// Parse one SSE event's `data` payload as JSON. Only called for event types
/// whose handling actually needs the parsed value (see [`SseAssembler::feed`]).
fn parse_event_json(data: &str) -> Result<serde_json::Value, ProviderError> {
    serde_json::from_str(data)
        .map_err(|e| ProviderError::Protocol(format!("invalid SSE JSON payload: {e}")))
}

/// Extract the required `index` field shared by all content-block events.
fn index_of(value: &serde_json::Value) -> Result<u64, ProviderError> {
    value["index"]
        .as_u64()
        .ok_or_else(|| ProviderError::Protocol("SSE event missing numeric index field".into()))
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
                        id: "tu_1".into(),
                        name: "read_tune".into(),
                        input: serde_json::json!({ "names": ["reqFuel"] }),
                    }],
                },
                ChatMessage::ToolResults {
                    results: vec![ToolResultMsg {
                        tool_use_id: "tu_1".into(),
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
            model: "claude-sonnet-5".into(),
            max_tokens: 4096,
        }
    }

    #[test]
    fn request_body_matches_wire_contract() {
        let body = build_request_body(&req());
        assert_eq!(body["model"], "claude-sonnet-5");
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["stream"], true);
        assert_eq!(body["system"], "You are a tuner.");
        assert_eq!(body["tools"][0]["name"], "read_tune");
        assert!(body.get("temperature").is_none());
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(body["messages"][2]["role"], "user");
        assert_eq!(body["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "tu_1");
    }

    #[test]
    fn empty_tools_key_is_omitted() {
        let mut request = req();
        request.tools = vec![];
        let body = build_request_body(&request);
        assert!(
            body.get("tools").is_none(),
            "empty tools array must be omitted, not sent as `\"tools\": []`"
        );
    }

    #[test]
    fn sse_assembler_builds_text_and_tool_use_turn() {
        let mut asm = SseAssembler::default();
        let mut deltas = String::new();
        let mut on_delta = |d: &str| deltas.push_str(d);
        asm.feed(
            "message_start",
            r#"{"type":"message_start"}"#,
            &mut on_delta,
        )
        .unwrap();
        asm.feed(
            "content_block_start",
            r#"{"index":0,"content_block":{"type":"text","text":""}}"#,
            &mut on_delta,
        )
        .unwrap();
        asm.feed(
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"text_delta","text":"Lean at "}}"#,
            &mut on_delta,
        )
        .unwrap();
        asm.feed(
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"text_delta","text":"4500rpm"}}"#,
            &mut on_delta,
        )
        .unwrap();
        asm.feed("content_block_stop", r#"{"index":0}"#, &mut on_delta)
            .unwrap();
        asm.feed("content_block_start", r#"{"index":1,"content_block":{"type":"tool_use","id":"tu_9","name":"run_ve_analyze","input":{}}}"#, &mut on_delta).unwrap();
        asm.feed(
            "content_block_delta",
            r#"{"index":1,"delta":{"type":"input_json_delta","partial_json":"{\"table\":"}}"#,
            &mut on_delta,
        )
        .unwrap();
        asm.feed(
            "content_block_delta",
            r#"{"index":1,"delta":{"type":"input_json_delta","partial_json":"\"veTable1Tbl\"}"}}"#,
            &mut on_delta,
        )
        .unwrap();
        asm.feed("content_block_stop", r#"{"index":1}"#, &mut on_delta)
            .unwrap();
        asm.feed(
            "message_delta",
            r#"{"delta":{"stop_reason":"tool_use"}}"#,
            &mut on_delta,
        )
        .unwrap();
        asm.feed("message_stop", r#"{"type":"message_stop"}"#, &mut on_delta)
            .unwrap();
        let turn = asm.finish().expect("complete turn");
        assert_eq!(deltas, "Lean at 4500rpm");
        assert_eq!(turn.stop_reason, StopReason::ToolUse);
        assert_eq!(turn.blocks.len(), 2);
        match &turn.blocks[1] {
            AssistantBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tu_9");
                assert_eq!(name, "run_ve_analyze");
                assert_eq!(input["table"], "veTable1Tbl");
            }
            other => panic!("expected tool_use, got {other:?}"),
        }
    }

    #[test]
    fn sse_error_event_becomes_protocol_error() {
        let mut asm = SseAssembler::default();
        let mut on_delta = |_: &str| {};
        let err = asm
            .feed(
                "error",
                r#"{"type":"error","error":{"type":"overloaded_error","message":"try later"}}"#,
                &mut on_delta,
            )
            .unwrap_err();
        assert!(matches!(
            err,
            crate::ai_provider::ProviderError::Protocol(_)
        ));
    }

    // --- Task 3: unknown-event tolerance ------------------------------------
    //
    // Anthropic's versioning policy allows new SSE event types to appear at
    // any time; clients must tolerate them. `feed` must not even attempt to
    // parse `data` as JSON for an event type it doesn't recognize — a future
    // event's payload shape is unknown, so requiring it to be valid JSON we
    // can parse (and erroring when it isn't) would break the app on any
    // provider-side addition.

    #[test]
    fn unknown_event_with_non_json_data_is_tolerated() {
        let mut asm = SseAssembler::default();
        let mut on_delta = |_: &str| {};
        let result = asm.feed("some_future_event", "not json", &mut on_delta);
        assert!(result.is_ok(), "unknown event must not error: {result:?}");
    }

    #[test]
    fn known_event_with_non_json_data_still_errors() {
        let mut asm = SseAssembler::default();
        let mut on_delta = |_: &str| {};
        let err = asm
            .feed("content_block_start", "not json", &mut on_delta)
            .unwrap_err();
        assert!(matches!(
            err,
            crate::ai_provider::ProviderError::Protocol(_)
        ));
    }

    #[test]
    fn debug_redacts_key() {
        let p = AnthropicProvider {
            api_key: "test-key".into(),
        };
        let dbg = format!("{p:?}");
        assert!(!dbg.contains("test-key"));
    }

    // --- F2: consume_lines chunk-split table test ---------------------------
    //
    // `consume_lines` is pure over a `&mut Vec<u8>` line buffer plus the
    // `current_event` cell, so it can be driven directly with the transcript
    // re-chunked at awkward byte positions to prove line/event reassembly is
    // chunk-boundary independent.

    /// A realistic transcript: `event:`/`data:` pairs, a `ping` event, a
    /// blank-line event-name reset between every message (per the SSE
    /// spec), a text delta containing the Polish "ż" (U+017C / UTF-8
    /// `C5 BC`, to exercise a multi-byte char split), and a tool-use block
    /// whose JSON input streams across two `input_json_delta` fragments.
    const ANTHROPIC_TRANSCRIPT: &str = r#"event: message_start
data: {"type":"message_start"}

event: content_block_start
data: {"index":0,"content_block":{"type":"text","text":""}}

event: ping
data: {"type": "ping"}

event: content_block_delta
data: {"index":0,"delta":{"type":"text_delta","text":"Lean at "}}

event: content_block_delta
data: {"index":0,"delta":{"type":"text_delta","text":"4500rpm ż"}}

event: content_block_stop
data: {"index":0}

event: content_block_start
data: {"index":1,"content_block":{"type":"tool_use","id":"tu_9","name":"run_ve_analyze","input":{}}}

event: content_block_delta
data: {"index":1,"delta":{"type":"input_json_delta","partial_json":"{\"table\":"}}

event: content_block_delta
data: {"index":1,"delta":{"type":"input_json_delta","partial_json":"\"veTable1Tbl\"}"}}

event: content_block_stop
data: {"index":1}

event: message_delta
data: {"delta":{"stop_reason":"tool_use"}}

event: message_stop
data: {"type":"message_stop"}

"#;

    fn expected_transcript_turn() -> ChatTurn {
        ChatTurn {
            blocks: vec![
                AssistantBlock::Text {
                    text: "Lean at 4500rpm ż".into(),
                },
                AssistantBlock::ToolUse {
                    id: "tu_9".into(),
                    name: "run_ve_analyze".into(),
                    input: serde_json::json!({ "table": "veTable1Tbl" }),
                },
            ],
            stop_reason: StopReason::ToolUse,
        }
    }

    fn chunks_of_one_byte(bytes: &[u8]) -> Vec<Vec<u8>> {
        bytes.iter().map(|&b| vec![b]).collect()
    }

    fn split_at(bytes: &[u8], mut positions: Vec<usize>) -> Vec<Vec<u8>> {
        positions.retain(|&p| p > 0 && p < bytes.len());
        positions.sort_unstable();
        positions.dedup();
        let mut chunks = Vec::new();
        let mut start = 0;
        for pos in positions {
            chunks.push(bytes[start..pos].to_vec());
            start = pos;
        }
        chunks.push(bytes[start..].to_vec());
        chunks
    }

    /// Split every `data:` line right between `"da"` and `"ta:"`.
    fn mid_data_keyword_splits(bytes: &[u8]) -> Vec<Vec<u8>> {
        let needle = b"data:";
        let positions = (0..bytes.len().saturating_sub(needle.len() - 1))
            .filter(|&i| &bytes[i..i + needle.len()] == needle)
            .map(|i| i + 2)
            .collect();
        split_at(bytes, positions)
    }

    /// Split mid the multi-byte UTF-8 encoding of "ż" (`C5 BC`).
    fn mid_utf8_char_splits(bytes: &[u8]) -> Vec<Vec<u8>> {
        let pos = bytes
            .windows(2)
            .position(|w| w == [0xC5, 0xBC])
            .expect("transcript must contain the 'ż' fixture");
        split_at(bytes, vec![pos + 1])
    }

    /// Feed `chunks` through `consume_lines` exactly as
    /// `AnthropicProvider::chat` does (extend the buffer, drain complete
    /// lines against the running `current_event`) and return the assembled
    /// turn.
    fn assemble_over_chunks(chunks: Vec<Vec<u8>>) -> ChatTurn {
        let mut line_buffer: Vec<u8> = Vec::new();
        let mut current_event: Option<String> = None;
        let mut assembler = SseAssembler::default();
        let mut deltas = String::new();
        let mut on_delta = |d: &str| deltas.push_str(d);
        for chunk in chunks {
            line_buffer.extend_from_slice(&chunk);
            consume_lines(
                &mut line_buffer,
                &mut current_event,
                &mut assembler,
                &mut on_delta,
            )
            .unwrap();
        }
        assembler.finish().expect("complete turn")
    }

    #[test]
    fn consume_lines_reassembles_identically_across_chunk_splits() {
        let lf_bytes = ANTHROPIC_TRANSCRIPT.as_bytes();
        let crlf_transcript = ANTHROPIC_TRANSCRIPT.replace('\n', "\r\n");
        let expected = expected_transcript_turn();

        let cases: Vec<(&str, Vec<Vec<u8>>)> = vec![
            ("single_chunk", vec![lf_bytes.to_vec()]),
            ("one_byte_at_a_time", chunks_of_one_byte(lf_bytes)),
            ("mid_data_keyword_split", mid_data_keyword_splits(lf_bytes)),
            ("mid_utf8_char_split", mid_utf8_char_splits(lf_bytes)),
            ("crlf_whole_transcript", vec![crlf_transcript.into_bytes()]),
        ];

        for (name, chunks) in cases {
            let turn = assemble_over_chunks(chunks);
            assert_eq!(
                turn, expected,
                "chunking case {name:?} produced a different turn"
            );
        }
    }
}
