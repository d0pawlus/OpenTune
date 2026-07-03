// SPDX-License-Identifier: GPL-3.0-or-later
//! Connection state manager — testable core, decoupled from AppHandle.
//!
//! The live connection is stored as `tauri::State<ConnectionStore>` where
//! `ConnectionStore = Arc<Mutex<Option<ActiveConnection>>>`. Commands borrow
//! the Arc; tests pass a Vec-collecting emit closure directly.

use std::sync::{Arc, Mutex};

use opentune_ini::{parse_comms, parse_definition, CommsSettings, Definition};
use opentune_model::Tune;
use opentune_protocol::{
    reconnect::{ConnectionManager, ReconnectConfig},
    ConnectionState, ProtocolError,
};
use opentune_simulator::{ecu::EcuClientTransport, EcuSimulator};
use opentune_transport::{serial::SerialTransport, SerialConfig};

// ── Type aliases ─────────────────────────────────────────────────────────────

/// Boxed FnMut factory — makes the `ConnectionManager` type nameable in state.
type SimFactory = Box<dyn FnMut() -> std::result::Result<EcuClientTransport, ProtocolError> + Send>;
type SerialFactory = Box<dyn FnMut() -> std::result::Result<SerialTransport, ProtocolError> + Send>;

// ── Active connection ────────────────────────────────────────────────────────

/// A live connection held in Tauri managed state.
pub enum ActiveConnection {
    Sim {
        manager: ConnectionManager<EcuClientTransport, SimFactory>,
        /// Kept so `simulate_link_drop` can reach `set_link_dropped`.
        simulator: Arc<EcuSimulator>,
    },
    Serial {
        manager: ConnectionManager<SerialTransport, SerialFactory>,
    },
}

// ── Session: the single owner of connection + definition + tune ──────────────

/// Everything a live tuning session owns, held under one mutex so **all**
/// hardware/page access is serialized (ARCHITECTURE §9 — serial is inherently
/// single-conversation). The M1 [`ActiveConnection`] is the sole owner of the
/// transport; the immutable [`Definition`] (`Arc`) and the owned [`Tune`] live
/// alongside it, so a `set_value`/`burn`/`undo` never touches the wire outside
/// this lock. `tune` is `None` until [`Session::load_tune`] reads the pages.
pub struct Session {
    /// The live connection — sole owner of the transport.
    pub conn: ActiveConnection,
    /// The parsed firmware definition; immutable, shared cheaply.
    pub def: Arc<Definition>,
    /// The in-memory editable tune, once loaded from the ECU.
    pub tune: Option<Tune>,
    /// The diff/merge baseline (Task 8) — an in-memory snapshot of `tune`
    /// taken by `Session::snapshot_tune`. `None` until a snapshot is taken;
    /// file-based `.msq` snapshots are M6.
    pub snapshot: Option<Tune>,
}

/// Tauri managed state type — the whole session behind one mutex.
pub type SessionStore = Arc<Mutex<Option<Session>>>;

// ── Source discriminant (IPC-visible) ────────────────────────────────────────

/// Which ECU to connect to; deserialized from the frontend command payload.
#[derive(serde::Deserialize, specta::Type, Debug, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ConnectSource {
    /// Built-in simulator.
    Simulator {
        /// Override INI path; `None` → load bundled sample.
        ini_path: Option<String>,
    },
    /// Real serial port.
    Serial {
        port_name: String,
        /// INI path (required for serial).
        ini_path: String,
    },
}

// ── INI helpers ──────────────────────────────────────────────────────────────

/// Parse `CommsSettings` from a file path. Expands a leading `~` to the user's
/// home dir (Rust's `fs` does not do shell tilde expansion).
pub fn load_comms_from_path(path: &str) -> Result<CommsSettings, String> {
    let expanded = expand_tilde(path);
    let text = std::fs::read_to_string(&expanded)
        .map_err(|e| format!("cannot read INI `{expanded}`: {e}"))?;
    parse_comms(&text).map_err(|e| format!("cannot parse INI `{expanded}`: {e}"))
}

/// Expand a leading `~` / `~/` to `$HOME`. Other `~user` forms are left as-is.
fn expand_tilde(path: &str) -> String {
    match path.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => match std::env::var("HOME") {
            Ok(home) => format!("{home}{rest}"),
            Err(_) => path.to_owned(),
        },
        _ => path.to_owned(),
    }
}

/// Parse `CommsSettings` from an in-memory string (bundled INI).
pub fn load_comms_from_str(ini: &str) -> Result<CommsSettings, String> {
    parse_comms(ini).map_err(|e| format!("cannot parse bundled INI: {e}"))
}

/// Parse a full [`Definition`] from a file path (expands a leading `~`).
///
/// M2 needs the whole definition — pages (to size the simulator + tune),
/// constants (to decode/encode), and menus/dialogs (to render the UI) — not
/// just the M1 comms slice.
pub fn load_definition_from_path(path: &str) -> Result<Definition, String> {
    let expanded = expand_tilde(path);
    let text = std::fs::read_to_string(&expanded)
        .map_err(|e| format!("cannot read INI `{expanded}`: {e}"))?;
    parse_definition(&text).map_err(|e| format!("cannot parse INI `{expanded}`: {e}"))
}

/// Parse a full [`Definition`] from an in-memory string (bundled INI).
pub fn load_definition_from_str(ini: &str) -> Result<Definition, String> {
    parse_definition(ini).map_err(|e| format!("cannot parse bundled INI: {e}"))
}

// ── Connect helpers ──────────────────────────────────────────────────────────

/// Connect to the simulator. Emits `Connecting` then `Connected` (or returns
/// an error string). The returned `ActiveConnection` owns the live manager.
///
/// The simulator is built **from the definition** (`from_definition`) so its
/// RAM/flash image is sized per declared page — a prerequisite for M2 page
/// read/write/burn. (M1's handshake-only `EcuSimulator::new()` had no pages.)
pub fn connect_simulator(
    def: &Definition,
    emit: &dyn Fn(ConnectionState),
) -> Result<ActiveConnection, String> {
    let sim = Arc::new(EcuSimulator::from_definition(def));
    let sim_ref = Arc::clone(&sim);

    let factory: SimFactory =
        Box::new(move || Ok(sim_ref.client_transport() as EcuClientTransport));

    let cfg = ReconnectConfig {
        max_attempts: 10,
        base_delay: std::time::Duration::from_millis(500),
        max_delay: std::time::Duration::from_secs(30),
    };
    let mut mgr = ConnectionManager::new(def.comms.clone(), cfg, factory);

    emit(ConnectionState::Connecting);
    let state = mgr.connect().map_err(|e| e.to_string())?;
    emit(state);

    Ok(ActiveConnection::Sim {
        manager: mgr,
        simulator: sim,
    })
}

/// Connect to a real serial port. Emits `Connecting` then `Connected` (or
/// returns an error string).
pub fn connect_serial(
    port_name: String,
    comms: CommsSettings,
    emit: &dyn Fn(ConnectionState),
) -> Result<ActiveConnection, String> {
    let port = port_name.clone();
    let serial_cfg = SerialConfig::default();

    let factory: SerialFactory =
        Box::new(move || Ok(SerialTransport::new(port.clone(), serial_cfg.clone())));

    let cfg = ReconnectConfig {
        max_attempts: 10,
        base_delay: std::time::Duration::from_millis(500),
        max_delay: std::time::Duration::from_secs(30),
    };
    let mut mgr = ConnectionManager::new(comms, cfg, factory);

    emit(ConnectionState::Connecting);
    let state = mgr.connect().map_err(|e| e.to_string())?;
    emit(state);

    Ok(ActiveConnection::Serial { manager: mgr })
}

/// Drop the simulator link, then drive `reconnect_collect_states` on a
/// background thread. Each emitted state is forwarded to `emit`; the session
/// (with its definition + tune preserved intact) is stored back via `store`
/// on completion.
///
/// Returns immediately — the reconnect runs in the background. Only the
/// connection half of the session is reconnected; the reconnect *logic* is
/// M1's, unchanged — the definition and tune are simply threaded through.
pub fn simulate_link_drop_async(
    session: Session,
    store: SessionStore,
    emit: impl Fn(ConnectionState) + Send + 'static,
) {
    std::thread::spawn(move || {
        let Session {
            conn,
            def,
            tune,
            snapshot,
        } = session;
        match conn {
            ActiveConnection::Sim {
                mut manager,
                simulator,
            } => {
                // Drop the link; reconnect logic will restore it via the
                // factory (each `client_transport()` call opens a fresh
                // transport on the same shared Pipe, which is in the
                // `dropped=true` state here). We restore the link just before
                // the first reconnect attempt so the simulator actually answers.
                simulator.set_link_dropped(true);
                // Restore immediately so the first reconnect attempt succeeds.
                simulator.set_link_dropped(false);

                let states = manager.reconnect_collect_states();
                for s in states {
                    emit(s);
                }
                if let Ok(mut guard) = store.lock() {
                    *guard = Some(Session {
                        conn: ActiveConnection::Sim { manager, simulator },
                        def,
                        tune,
                        snapshot,
                    });
                }
            }
            ActiveConnection::Serial { .. } => {
                emit(ConnectionState::Failed {
                    reason: "simulate_link_drop is only available in simulator mode".to_owned(),
                });
            }
        }
    });
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use opentune_ini::{Endianness, EnvelopeFormat};

    fn plain_comms() -> CommsSettings {
        CommsSettings {
            signature: "speeduino 202504-dev".to_owned(),
            query_command: "Q".to_owned(),
            version_info: "S".to_owned(),
            och_get_command: "A".to_owned(),
            page_read_command: "p%2i%2o%2c".to_owned(),
            page_value_write: "M%2i%2o%2c%v".to_owned(),
            burn_command: "b%2i".to_owned(),
            blocking_factor: 121,
            page_activation_delay_ms: 10,
            block_read_timeout_ms: 2_000,
            inter_write_delay_ms: 10,
            endianness: Endianness::Little,
            envelope: EnvelopeFormat::Plain,
        }
    }

    /// Build a minimal page-backed definition for the simulator connect test:
    /// the plain comms above plus a single 4-byte page.
    fn plain_definition() -> Definition {
        Definition {
            comms: plain_comms(),
            pages: vec![opentune_ini::PageDef { number: 1, size: 4 }],
            constants: Vec::new(),
            pc_variables: Vec::new(),
            menus: Vec::new(),
            dialogs: Vec::new(),
            tables: Vec::new(),
            curves: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn connect_simulator_emits_connected_with_correct_signature() {
        let emitted = std::sync::Mutex::new(Vec::new());
        let def = plain_definition();
        let active = connect_simulator(&def, &|s| emitted.lock().unwrap().push(s))
            .expect("connect must succeed");
        let emitted = emitted.into_inner().unwrap();

        assert!(
            emitted
                .iter()
                .any(|s| matches!(s, ConnectionState::Connected { .. })),
            "must emit Connected; got: {emitted:?}"
        );
        let last = emitted.last().expect("at least one state emitted");
        match last {
            ConnectionState::Connected { identity } => {
                assert_eq!(identity.signature, EcuSimulator::SIGNATURE);
            }
            other => panic!("expected Connected, got {other:?}"),
        }
        let _ = active;
    }

    #[test]
    fn expand_tilde_expands_leading_home_only() {
        std::env::set_var("HOME", "/home/x");
        assert_eq!(expand_tilde("~/a/b.ini"), "/home/x/a/b.ini");
        assert_eq!(expand_tilde("~"), "/home/x");
        // Not a leading `~/` — left untouched.
        assert_eq!(expand_tilde("/abs/path"), "/abs/path");
        assert_eq!(expand_tilde("~user/x"), "~user/x");
    }

    #[test]
    fn bundled_ini_parses_correctly() {
        let ini = include_str!("../resources/speeduino.sample.ini");
        let comms = load_comms_from_str(ini).expect("bundled INI must parse");
        assert_eq!(comms.signature, EcuSimulator::SIGNATURE);
        assert_eq!(comms.query_command, "Q");
        assert_eq!(comms.envelope, EnvelopeFormat::Plain);
    }
}
