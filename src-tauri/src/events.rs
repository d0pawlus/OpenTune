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
}
