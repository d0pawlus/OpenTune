// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for the chat loop (`run_chat_turn`): arrange a
//! simulator-backed `AiToolExecutor` (mirroring `owner_ai_tests.rs` and
//! `ai_tools.rs`'s test setup) and a scripted `FakeProvider`, then pin every
//! clause of the behavior contract in the task brief.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::*;
use crate::ai_provider::{
    AssistantBlock, ChatMessage, ChatTurn, FakeProvider, Provider, StopReason, ToolDef,
};
use crate::ai_tools::{AiToolExecutor, AuditSink};
use crate::connection::ConnectSource;
use crate::owner::{request, spawn_owner_with_emitter, Command, Emitter};
use opentune_ai::{available_tools, AuditChannel, GuardrailLimits, PermissionPolicy};

/// Audit sink that collects lines for assertions — copied from
/// `ai_tools.rs`'s `VecSink` test helper.
#[derive(Default, Clone)]
struct VecSink(Arc<Mutex<Vec<String>>>);

impl AuditSink for VecSink {
    fn append(&self, line: &str) {
        self.0.lock().unwrap().push(line.to_owned());
    }
}

/// A simulator-backed executor with a connected owner and a loaded tune —
/// mirrors `ai_tools.rs`'s `connected_executor` and `owner_ai_tests.rs`'s
/// `connect_and_load` arrangement.
async fn connected_executor() -> (AiToolExecutor, VecSink) {
    let emit: Emitter = Arc::new(|_| {});
    let owner = spawn_owner_with_emitter(emit);
    request(&owner, |reply| Command::Connect {
        source: ConnectSource::Simulator { ini_path: None },
        reply,
    })
    .await
    .expect("simulator connects");
    request(&owner, |reply| Command::LoadTune { reply })
        .await
        .expect("tune loads");
    let sink = VecSink::default();
    let exec = AiToolExecutor::new(
        owner,
        PermissionPolicy::advisory(),
        GuardrailLimits::default(),
        AuditChannel::Assistant,
        Box::new(sink.clone()),
    );
    (exec, sink)
}

/// The advisory tool list, converted to the provider-facing `ToolDef` shape.
fn tools() -> Vec<ToolDef> {
    available_tools(&PermissionPolicy::advisory())
        .into_iter()
        .map(ToolDef::from)
        .collect()
}

#[tokio::test]
async fn full_turn_with_tool_round_trip() {
    let (executor, sink) = connected_executor().await;
    let provider = Provider::Fake(FakeProvider::new(vec![
        ChatTurn {
            blocks: vec![
                AssistantBlock::Text {
                    text: "checking".into(),
                },
                AssistantBlock::ToolUse {
                    id: "call_1".into(),
                    name: "read_tune".into(),
                    input: serde_json::json!({ "names": ["reqFuel"] }),
                },
            ],
            stop_reason: StopReason::ToolUse,
        },
        ChatTurn {
            blocks: vec![AssistantBlock::Text {
                text: "done".into(),
            }],
            stop_reason: StopReason::EndTurn,
        },
    ]));

    let events: Arc<Mutex<Vec<ChatEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_sink = events.clone();
    let emit = move |ev: ChatEvent| events_sink.lock().unwrap().push(ev);

    let mut history = ChatHistory::new();
    let cancel = AtomicBool::new(false);
    let result = run_chat_turn(
        &provider,
        &executor,
        &mut history,
        &tools(),
        "system prompt",
        "fake-model",
        1024,
        "check reqFuel".to_owned(),
        &cancel,
        &emit,
    )
    .await;
    assert_eq!(result, Ok(()));

    let events = events.lock().unwrap();
    assert!(
        events.iter().any(|e| matches!(e, ChatEvent::Delta { .. })),
        "at least one delta streamed: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ChatEvent::ToolStart { name } if name == "read_tune")),
        "ToolStart for read_tune: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ChatEvent::ToolEnd { name, ok: true, .. } if name == "read_tune")),
        "ToolEnd(ok) for read_tune: {events:?}"
    );
    assert!(
        matches!(events.last(), Some(ChatEvent::Done)),
        "Done is the last event: {events:?}"
    );
    drop(events);

    let messages = history.messages();
    assert_eq!(messages.len(), 4, "User, Assistant, ToolResults, Assistant");
    assert!(matches!(&messages[0], ChatMessage::User { text } if text == "check reqFuel"));
    assert!(matches!(&messages[1], ChatMessage::Assistant { .. }));
    let ChatMessage::ToolResults { results } = &messages[2] else {
        panic!("expected ToolResults at index 2, got {:?}", messages[2]);
    };
    assert_eq!(results.len(), 1);
    assert!(!results[0].is_error);
    let parsed: serde_json::Value =
        serde_json::from_str(&results[0].content).expect("tool result content is JSON");
    assert!(
        parsed["values"].is_array(),
        "read_tune result carries a values array: {parsed}"
    );
    assert!(matches!(&messages[3], ChatMessage::Assistant { .. }));

    assert_eq!(
        sink.0.lock().unwrap().len(),
        1,
        "exactly one audit line for the one read_tune execution"
    );
}

#[tokio::test]
async fn tool_error_is_returned_to_the_model_not_fatal() {
    let (executor, _sink) = connected_executor().await;
    let provider = Provider::Fake(FakeProvider::new(vec![
        ChatTurn {
            blocks: vec![AssistantBlock::ToolUse {
                id: "call_1".into(),
                name: "read_tune".into(),
                input: serde_json::json!({ "bogus": true }),
            }],
            stop_reason: StopReason::ToolUse,
        },
        ChatTurn {
            blocks: vec![AssistantBlock::Text {
                text: "sorted".into(),
            }],
            stop_reason: StopReason::EndTurn,
        },
    ]));

    let events: Arc<Mutex<Vec<ChatEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_sink = events.clone();
    let emit = move |ev: ChatEvent| events_sink.lock().unwrap().push(ev);

    let mut history = ChatHistory::new();
    let cancel = AtomicBool::new(false);
    let result = run_chat_turn(
        &provider,
        &executor,
        &mut history,
        &tools(),
        "system prompt",
        "fake-model",
        1024,
        "read something invalid".to_owned(),
        &cancel,
        &emit,
    )
    .await;
    assert_eq!(result, Ok(()));

    let events = events.lock().unwrap();
    assert!(
        events.iter().any(
            |e| matches!(e, ChatEvent::ToolEnd { name, ok: false, .. } if name == "read_tune")
        ),
        "ToolEnd(err) for read_tune: {events:?}"
    );
    assert!(
        matches!(events.last(), Some(ChatEvent::Done)),
        "loop continued through to Done: {events:?}"
    );
    drop(events);

    let messages = history.messages();
    let ChatMessage::ToolResults { results } = &messages[2] else {
        panic!("expected ToolResults at index 2, got {:?}", messages[2]);
    };
    assert_eq!(results.len(), 1);
    assert!(
        results[0].is_error,
        "invalid tool input surfaces as is_error, not an abort"
    );
    assert!(!results[0].content.is_empty(), "carries the error message");
    assert_eq!(messages.len(), 4, "the loop was not aborted early");
}

#[tokio::test]
async fn runaway_tool_loop_stops_at_cap() {
    let (executor, _sink) = connected_executor().await;
    let repeated_tool_use = || ChatTurn {
        blocks: vec![AssistantBlock::ToolUse {
            id: "call".into(),
            name: "read_tune".into(),
            input: serde_json::json!({ "names": ["reqFuel"] }),
        }],
        stop_reason: StopReason::ToolUse,
    };
    let scripted: Vec<ChatTurn> = (0..(MAX_TOOL_ITERATIONS + 1))
        .map(|_| repeated_tool_use())
        .collect();
    let provider = Provider::Fake(FakeProvider::new(scripted));

    let events: Arc<Mutex<Vec<ChatEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_sink = events.clone();
    let emit = move |ev: ChatEvent| events_sink.lock().unwrap().push(ev);

    let mut history = ChatHistory::new();
    let cancel = AtomicBool::new(false);
    let result = run_chat_turn(
        &provider,
        &executor,
        &mut history,
        &tools(),
        "system prompt",
        "fake-model",
        1024,
        "loop forever".to_owned(),
        &cancel,
        &emit,
    )
    .await;
    assert_eq!(result, Ok(()));

    let events = events.lock().unwrap();
    let tool_starts = events
        .iter()
        .filter(|e| matches!(e, ChatEvent::ToolStart { .. }))
        .count();
    assert_eq!(
        tool_starts, MAX_TOOL_ITERATIONS,
        "exactly the cap worth of tool starts: {events:?}"
    );
    match events.last() {
        Some(ChatEvent::Error { message }) => {
            assert!(
                message.contains(&MAX_TOOL_ITERATIONS.to_string()),
                "error names the cap: {message}"
            );
        }
        other => panic!("expected a final Error event mentioning the cap, got {other:?}"),
    }
}

#[tokio::test]
async fn cancel_flag_stops_before_next_provider_call() {
    let (executor, _sink) = connected_executor().await;
    let provider = Provider::Fake(FakeProvider::new(vec![
        ChatTurn {
            blocks: vec![AssistantBlock::ToolUse {
                id: "call_1".into(),
                name: "read_tune".into(),
                input: serde_json::json!({ "names": ["reqFuel"] }),
            }],
            stop_reason: StopReason::ToolUse,
        },
        ChatTurn {
            blocks: vec![AssistantBlock::Text {
                text: "should never be reached".into(),
            }],
            stop_reason: StopReason::EndTurn,
        },
    ]));

    let events: Arc<Mutex<Vec<ChatEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_sink = events.clone();
    // The cancel flag flips mid-turn, from inside the ToolEnd emit — proving
    // the check happens on the next loop pass, not just at entry.
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_writer = cancel.clone();
    let emit = move |ev: ChatEvent| {
        if matches!(ev, ChatEvent::ToolEnd { .. }) {
            cancel_writer.store(true, Ordering::SeqCst);
        }
        events_sink.lock().unwrap().push(ev);
    };

    let mut history = ChatHistory::new();
    let result = run_chat_turn(
        &provider,
        &executor,
        &mut history,
        &tools(),
        "system prompt",
        "fake-model",
        1024,
        "cancel me".to_owned(),
        cancel.as_ref(),
        &emit,
    )
    .await;
    assert_eq!(result, Ok(()));

    let events = events.lock().unwrap();
    assert!(
        matches!(events.last(), Some(ChatEvent::Cancelled)),
        "Cancelled is the last event: {events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e, ChatEvent::Done)),
        "the loop never reached Done: {events:?}"
    );
    drop(events);

    let remaining = match &provider {
        Provider::Fake(fake) => fake.turns.lock().unwrap().len(),
        _ => panic!("expected the fake provider"),
    };
    assert_eq!(
        remaining, 1,
        "the second scripted turn was never consumed — no second provider call"
    );
}

#[tokio::test]
async fn propose_change_emits_proposal_ready() {
    let (executor, _sink) = connected_executor().await;
    let provider = Provider::Fake(FakeProvider::new(vec![
        ChatTurn {
            blocks: vec![AssistantBlock::ToolUse {
                id: "call_1".into(),
                name: "propose_change".into(),
                input: serde_json::json!({
                    "constant": "reqFuel",
                    "edits": [{ "index": 0, "value": 13.0 }],
                    "reason": "test"
                }),
            }],
            stop_reason: StopReason::ToolUse,
        },
        ChatTurn {
            blocks: vec![AssistantBlock::Text {
                text: "proposed".into(),
            }],
            stop_reason: StopReason::EndTurn,
        },
    ]));

    let events: Arc<Mutex<Vec<ChatEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_sink = events.clone();
    let emit = move |ev: ChatEvent| events_sink.lock().unwrap().push(ev);

    let mut history = ChatHistory::new();
    let cancel = AtomicBool::new(false);
    let result = run_chat_turn(
        &provider,
        &executor,
        &mut history,
        &tools(),
        "system prompt",
        "fake-model",
        1024,
        "propose a change".to_owned(),
        &cancel,
        &emit,
    )
    .await;
    assert_eq!(result, Ok(()));

    let events = events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ChatEvent::ProposalReady { id: 1 })),
        "ProposalReady{{id:1}} emitted: {events:?}"
    );
    drop(events);

    let proposals = executor.proposals();
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].constant, "reqFuel");
    assert_eq!(proposals[0].id, 1);
}
