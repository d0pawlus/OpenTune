// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;

/// Tool definition for AI provider consumption.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

impl From<opentune_ai::ToolSpec> for ToolDef {
    fn from(spec: opentune_ai::ToolSpec) -> Self {
        Self {
            name: spec.name.to_owned(),
            description: spec.description.to_owned(),
            input_schema: spec.input_schema,
        }
    }
}

/// Tool result message.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolResultMsg {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}

/// Assistant response block — text or tool use.
#[derive(Debug, Clone, PartialEq)]
pub enum AssistantBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

/// Chat message for conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatMessage {
    User { text: String },
    Assistant { blocks: Vec<AssistantBlock> },
    ToolResults { results: Vec<ToolResultMsg> },
}

/// Reason the chat turn ended.
#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Other(String),
}

/// Chat request to a provider.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    pub system: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDef>,
    pub model: String,
    pub max_tokens: u32,
}

/// Chat turn response.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatTurn {
    pub blocks: Vec<AssistantBlock>,
    pub stop_reason: StopReason,
}

/// Error from a provider.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderError {
    MissingKey,
    Http { status: u16, message: String },
    Network(String),
    Protocol(String),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderError::MissingKey => {
                write!(f, "API key is missing or not configured")
            }
            ProviderError::Http { status, message } => {
                write!(f, "HTTP {}: {}", status, message)
            }
            ProviderError::Network(msg) => {
                write!(f, "Network error: {}", msg)
            }
            ProviderError::Protocol(msg) => {
                write!(f, "Protocol error: {}", msg)
            }
        }
    }
}

impl std::error::Error for ProviderError {}

/// Callback type for streaming text deltas.
pub type OnDelta<'a> = &'a mut (dyn FnMut(&str) + Send);

/// AI provider — enum dispatch (not dyn/async_trait).
/// ponytail: enum over dyn — two real providers; revisit if a third party needs to plug in
pub enum Provider {
    Fake(FakeProvider),
    // Tasks 4/5 add Anthropic/OpenAi variants
}

impl Provider {
    /// Chat with the provider, streaming text through `on_delta`.
    pub async fn chat(
        &self,
        req: &ChatRequest,
        on_delta: OnDelta<'_>,
    ) -> Result<ChatTurn, ProviderError> {
        match self {
            Provider::Fake(fake) => fake.chat(req, on_delta).await,
        }
    }
}

/// Fake provider for testing — pops scripted turns in order.
pub struct FakeProvider {
    turns: std::sync::Mutex<Vec<ChatTurn>>,
}

impl FakeProvider {
    /// Create a fake provider with scripted turns.
    /// Turns are reversed internally so `pop()` yields script order.
    pub fn new(turns: Vec<ChatTurn>) -> Self {
        let mut turns = turns;
        turns.reverse();
        Self {
            turns: std::sync::Mutex::new(turns),
        }
    }

    /// Pop the next turn and stream each Text block in two chunks.
    async fn chat(
        &self,
        _req: &ChatRequest,
        on_delta: OnDelta<'_>,
    ) -> Result<ChatTurn, ProviderError> {
        let mut turns = self
            .turns
            .lock()
            .map_err(|_| ProviderError::Protocol("failed to lock turns".into()))?;

        let turn = turns
            .pop()
            .ok_or_else(|| ProviderError::Protocol("fake provider script exhausted".into()))?;

        // Stream each Text block in two char-boundary-safe chunks.
        for block in &turn.blocks {
            if let AssistantBlock::Text { text } = block {
                let char_count = text.chars().count();
                let mid_index = char_count / 2;

                if mid_index > 0 {
                    // Safe: char_indices().nth(mid_index) is guaranteed to return Some
                    // since mid_index = char_count / 2 < char_count when mid_index > 0.
                    if let Some((byte_pos, _)) = text.char_indices().nth(mid_index) {
                        let first_half = &text[..byte_pos];
                        let second_half = &text[byte_pos..];
                        on_delta(first_half);
                        if !second_half.is_empty() {
                            on_delta(second_half);
                        }
                    }
                } else if !text.is_empty() {
                    on_delta(text);
                }
            }
        }

        Ok(turn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scripted_turn() -> ChatTurn {
        ChatTurn {
            blocks: vec![
                AssistantBlock::Text {
                    text: "hello world".into(),
                },
                AssistantBlock::ToolUse {
                    id: "t1".into(),
                    name: "read_tune".into(),
                    input: serde_json::json!({ "names": ["reqFuel"] }),
                },
            ],
            stop_reason: StopReason::ToolUse,
        }
    }

    #[tokio::test]
    async fn fake_provider_pops_turns_and_streams_text() {
        let provider = Provider::Fake(FakeProvider::new(vec![scripted_turn()]));
        let req = ChatRequest {
            system: "s".into(),
            messages: vec![ChatMessage::User { text: "hi".into() }],
            tools: vec![],
            model: "fake".into(),
            max_tokens: 100,
        };
        let mut chunks = Vec::new();
        {
            let mut on_delta = |d: &str| chunks.push(d.to_string());
            let turn = provider
                .chat(&req, &mut on_delta)
                .await
                .expect("scripted turn");
            assert_eq!(turn.stop_reason, StopReason::ToolUse);
            assert_eq!(turn.blocks.len(), 2);
        }
        // Verify the two-chunk streaming shape for the "hello world" text block.
        let text_chunks: Vec<_> = chunks.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            text_chunks.len(),
            2,
            "Text block must stream in exactly 2 chunks"
        );
        let concatenated: String = chunks.iter().map(|s| s.as_str()).collect();
        assert_eq!(concatenated, "hello world");
        // Verify script exhaustion.
        let mut deltas2 = String::new();
        let mut on_delta2 = |d: &str| deltas2.push_str(d);
        let err = provider.chat(&req, &mut on_delta2).await.unwrap_err();
        assert!(matches!(err, ProviderError::Protocol(_)));
    }

    #[test]
    fn tool_spec_converts_to_tool_def() {
        let spec = opentune_ai::registry()
            .into_iter()
            .find(|t| t.name == "read_tune")
            .expect("registry has read_tune");
        let def = ToolDef::from(spec);
        assert_eq!(def.name, "read_tune");
        assert_eq!(def.input_schema["type"], "object");
        assert!(!def.description.is_empty());
    }
}
