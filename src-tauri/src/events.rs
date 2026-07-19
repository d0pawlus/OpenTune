// SPDX-License-Identifier: GPL-3.0-or-later
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct Heartbeat {
    pub seq: u32,
}

/// Emitted whenever the in-memory [`Tune`](opentune_model::Tune) dirty state
/// changes — after a `set_value`, `undo`, `redo`, or `burn`. Drives the
/// "modified, not burned" badge: `dirty == true` means RAM diverges from flash.
///
/// The backend is the single source of truth; the frontend never computes
/// dirtiness itself, it just reflects the last event it received.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type, Event)]
pub struct TuneDirtyEvent {
    /// Whether any page has unburned edits.
    pub dirty: bool,
    /// The page numbers with unburned edits, ascending. Empty when clean.
    pub dirty_pages: Vec<u16>,
}

/// IPC-serialisable mirror of [`opentune_protocol::ConnectionState`].
///
/// `tauri-specta` requires `specta::Type` on every emitted event type.
/// `ConnectionState` lives in the `protocol` crate which intentionally has
/// no dependency on `specta`, so we mirror only the fields the UI needs.
/// The Rust → TS direction is write-only (backend emits, frontend listens).
///
/// # Variants
/// - `Disconnected` — no active link.
/// - `Connecting` — transport opening / handshake in progress.
/// - `Connected` — fully identified; includes the firmware signature.
/// - `Reconnecting` — link was lost; retry in progress; `attempt` is 1-based.
/// - `Failed` — gave up; `reason` is a human-readable diagnostic.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type, Event)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ConnectionStateEvent {
    Disconnected,
    Connecting,
    Connected {
        /// The firmware signature string reported by the ECU.
        signature: String,
        /// The human-readable firmware version string (may be empty).
        version: String,
    },
    Reconnecting {
        /// 1-based retry count so the UI can show progress.
        attempt: u32,
    },
    Failed {
        /// Diagnostic string; never expose internal paths or hardware details.
        reason: String,
    },
}

/// The M3 realtime dashboard frame payload, emitted at up to ~30 Hz.
///
/// Registered in `collect_events!` (`lib.rs`) alongside the
/// `start_realtime`/`stop_realtime` commands. The owner poll loop emits it,
/// coalesced to ≤30 Hz, for as long as realtime is armed — arming persists
/// across a link drop, so frames resume automatically after recovery.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type, Event)]
pub struct RealtimeFrameEvent {
    /// (channel name, physical value) pairs — the full decoded frame, ≤30 Hz.
    pub channels: Vec<(String, f64)>,
}

/// The M7 slice-3 embedded assistant's streaming progress for one chat
/// turn, mirroring [`crate::ai_chat::ChatEvent`] 1:1. Kept as a separate
/// type rather than deriving specta directly on `ChatEvent`: `ChatEvent`
/// lives in the pure chat-loop module (task 1/2) and stays IPC-agnostic,
/// same reasoning as [`ConnectionStateEvent`] mirroring
/// `opentune_protocol::ConnectionState` above.
///
/// # Variants
/// - `Delta` — a streamed text chunk from the model.
/// - `ToolStart` / `ToolEnd` — one tool call's lifecycle; `ToolEnd.ok`
///   distinguishes a successful result from a tool error (both are still
///   returned to the model, never abort the turn — see `ai_chat`).
/// - `ProposalReady` — a `propose_change` call recorded a new proposal;
///   `id` indexes into `ai_proposals`' list.
/// - `Done` — the turn ended normally (model reached `end_turn`).
/// - `Cancelled` — `ai_cancel` was observed before the turn finished.
/// - `Error` — the turn ended abnormally; `message` is an English
///   diagnostic (the frontend renders its own localized copy).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type, Event)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AiStreamEvent {
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

impl From<crate::ai_chat::ChatEvent> for AiStreamEvent {
    fn from(ev: crate::ai_chat::ChatEvent) -> Self {
        use crate::ai_chat::ChatEvent;
        match ev {
            ChatEvent::Delta { text } => Self::Delta { text },
            ChatEvent::ToolStart { name } => Self::ToolStart { name },
            ChatEvent::ToolEnd { name, ok, summary } => Self::ToolEnd { name, ok, summary },
            ChatEvent::ProposalReady { id } => Self::ProposalReady { id },
            ChatEvent::Done => Self::Done,
            ChatEvent::Cancelled => Self::Cancelled,
            ChatEvent::Error { message } => Self::Error { message },
        }
    }
}

impl From<opentune_protocol::ConnectionState> for ConnectionStateEvent {
    fn from(state: opentune_protocol::ConnectionState) -> Self {
        use opentune_protocol::ConnectionState;
        match state {
            ConnectionState::Disconnected => Self::Disconnected,
            ConnectionState::Connecting => Self::Connecting,
            ConnectionState::Connected { identity } => Self::Connected {
                signature: identity.signature,
                version: identity.version,
            },
            ConnectionState::Reconnecting { attempt } => Self::Reconnecting { attempt },
            ConnectionState::Failed { reason } => Self::Failed { reason },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentune_protocol::{ConnectionState, EcuIdentity};

    #[test]
    fn disconnected_maps_to_ipc_disconnected() {
        let ev: ConnectionStateEvent = ConnectionState::Disconnected.into();
        assert_eq!(ev, ConnectionStateEvent::Disconnected);
    }

    #[test]
    fn connecting_maps_to_ipc_connecting() {
        let ev: ConnectionStateEvent = ConnectionState::Connecting.into();
        assert_eq!(ev, ConnectionStateEvent::Connecting);
    }

    #[test]
    fn connected_maps_signature_and_version() {
        let state = ConnectionState::Connected {
            identity: EcuIdentity {
                signature: "speeduino 202504-dev".to_owned(),
                version: "Speeduino 2025.04".to_owned(),
            },
        };
        let ev: ConnectionStateEvent = state.into();
        assert_eq!(
            ev,
            ConnectionStateEvent::Connected {
                signature: "speeduino 202504-dev".to_owned(),
                version: "Speeduino 2025.04".to_owned(),
            }
        );
    }

    #[test]
    fn reconnecting_maps_attempt() {
        let state = ConnectionState::Reconnecting { attempt: 3 };
        let ev: ConnectionStateEvent = state.into();
        assert_eq!(ev, ConnectionStateEvent::Reconnecting { attempt: 3 });
    }

    #[test]
    fn failed_maps_reason() {
        let state = ConnectionState::Failed {
            reason: "too many retries".to_owned(),
        };
        let ev: ConnectionStateEvent = state.into();
        assert_eq!(
            ev,
            ConnectionStateEvent::Failed {
                reason: "too many retries".to_owned(),
            }
        );
    }

    // ── M7 slice 3 task 4: ChatEvent -> AiStreamEvent (mechanical 1:1) ────

    use crate::ai_chat::ChatEvent;

    #[test]
    fn delta_maps_text() {
        let ev: AiStreamEvent = ChatEvent::Delta {
            text: "hi".to_owned(),
        }
        .into();
        assert_eq!(
            ev,
            AiStreamEvent::Delta {
                text: "hi".to_owned()
            }
        );
    }

    #[test]
    fn tool_start_maps_name() {
        let ev: AiStreamEvent = ChatEvent::ToolStart {
            name: "read_tune".to_owned(),
        }
        .into();
        assert_eq!(
            ev,
            AiStreamEvent::ToolStart {
                name: "read_tune".to_owned()
            }
        );
    }

    #[test]
    fn tool_end_maps_name_ok_and_summary() {
        let ev: AiStreamEvent = ChatEvent::ToolEnd {
            name: "propose_change".to_owned(),
            ok: true,
            summary: "proposal #1 ok=true".to_owned(),
        }
        .into();
        assert_eq!(
            ev,
            AiStreamEvent::ToolEnd {
                name: "propose_change".to_owned(),
                ok: true,
                summary: "proposal #1 ok=true".to_owned(),
            }
        );
    }

    #[test]
    fn proposal_ready_maps_id() {
        let ev: AiStreamEvent = ChatEvent::ProposalReady { id: 3 }.into();
        assert_eq!(ev, AiStreamEvent::ProposalReady { id: 3 });
    }

    #[test]
    fn done_and_cancelled_map_to_unit_variants() {
        assert_eq!(AiStreamEvent::from(ChatEvent::Done), AiStreamEvent::Done);
        assert_eq!(
            AiStreamEvent::from(ChatEvent::Cancelled),
            AiStreamEvent::Cancelled
        );
    }

    #[test]
    fn error_maps_message() {
        let ev: AiStreamEvent = ChatEvent::Error {
            message: "boom".to_owned(),
        }
        .into();
        assert_eq!(
            ev,
            AiStreamEvent::Error {
                message: "boom".to_owned()
            }
        );
    }
}
