// SPDX-License-Identifier: GPL-3.0-or-later
//! Connection state manager — testable core, decoupled from AppHandle.
//!
//! The live [`Session`] is owned exclusively by the §9 owner task
//! ([`crate::owner`]); this module provides the pieces it is built from:
//! [`ActiveConnection`], the INI helpers, and the connect functions. Tests
//! pass a Vec-collecting emit closure directly.

use std::sync::Arc;

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

/// Everything a live tuning session owns, held exclusively by the §9 owner
/// task ([`crate::owner`]) so **all** hardware/page access is serialized
/// through its command channel (serial is inherently single-conversation).
/// The M1 [`ActiveConnection`] is the sole owner of the transport; the
/// immutable [`Definition`] (`Arc`) and the owned [`Tune`] live alongside it,
/// so a `set_value`/`burn`/`undo` never touches the wire outside the owner.
/// `tune` is `None` until [`Session::load_tune`] reads the pages.
pub struct Session {
    /// The live connection, if any. `None` = an offline session (a tune loaded
    /// from a file or created blank) that has no ECU link yet.
    pub conn: Option<ActiveConnection>,
    /// The parsed firmware definition; immutable, shared cheaply.
    pub def: Arc<Definition>,
    /// The in-memory editable tune, once loaded from the ECU.
    pub tune: Option<Tune>,
    /// The diff/merge baseline (Task 8) — an in-memory snapshot of `tune`
    /// taken by `Session::snapshot_tune`. `None` until a snapshot is taken;
    /// file-based `.msq` snapshots are M6.
    pub snapshot: Option<Tune>,
    /// `true` when this session originated offline (a tune created blank or
    /// loaded from a `.msq`, possibly later ATTACHed to a live link);
    /// `false` for an online session whose tune was FRESH-read from the ECU.
    ///
    /// The distinction drives disconnect behavior: an offline-origin tune must
    /// **survive** a disconnect (drop the link, keep the editable/saveable
    /// tune — design spec §"Disconnect while editing"), whereas an online tune
    /// is destroyed so a later connect FRESH-reads rather than ATTACHing a
    /// stale online tune.
    pub offline_origin: bool,
}

impl ActiveConnection {
    /// Defense-in-depth signature guard (design spec §Safety guards #2): the
    /// connected ECU's reported signature must match `comms` (the tune's INI).
    /// Single source of truth for both the attach-time check
    /// (`owner_ops::verify_signature`) and the pre-write re-check
    /// (`Session::write_all_to_ecu`).
    ///
    /// The simulator is built *from* the definition, so its identity is
    /// definitionally the def's signature — that arm always matches. The real
    /// check is the serial arm, comparing the manager's identified signature.
    pub fn verify_signature(&self, comms: &CommsSettings) -> Result<(), String> {
        match self {
            ActiveConnection::Sim { .. } => Ok(()),
            ActiveConnection::Serial { manager } => match manager.state() {
                ConnectionState::Connected { identity } if identity.matches(comms) => Ok(()),
                ConnectionState::Connected { identity } => Err(format!(
                    "connected ECU signature `{}` does not match your tune's INI `{}`",
                    identity.signature, comms.signature
                )),
                _ => Err(
                    "ECU did not report a signature; cannot verify tune compatibility".to_string(),
                ),
            },
        }
    }
}

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

/// Decode raw file bytes as UTF-8, falling back to Latin-1 (every byte maps
/// 1:1 onto the same code point, so the fallback never fails). TunerStudio's
/// MegaSquirt INI and `.msq` files are CP1252-encoded — unit strings like
/// `"°C"` carry a raw 0xB0 byte that strict UTF-8 reading rejects outright.
fn decode_text(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => err.into_bytes().iter().map(|&b| b as char).collect(),
    }
}

/// Read a text file as UTF-8 with a Latin-1 fallback (see [`decode_text`]).
pub(crate) fn read_text(path: &str) -> std::io::Result<String> {
    std::fs::read(path).map(decode_text)
}

/// Parse `CommsSettings` from a file path. Expands a leading `~` to the user's
/// home dir (Rust's `fs` does not do shell tilde expansion).
pub fn load_comms_from_path(path: &str) -> Result<CommsSettings, String> {
    let expanded = expand_tilde(path);
    let text = read_text(&expanded).map_err(|e| format!("cannot read INI `{expanded}`: {e}"))?;
    parse_comms(&text).map_err(|e| format!("cannot parse INI `{expanded}`: {e}"))
}

/// Expand a leading `~` / `~/` to `$HOME`. Other `~user` forms are left as-is.
pub(crate) fn expand_tilde(path: &str) -> String {
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
    let text = read_text(&expanded).map_err(|e| format!("cannot read INI `{expanded}`: {e}"))?;
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
        // The in-process simulator needs the same bounded exponential shape,
        // but not human-scale serial delays.
        base_delay: std::time::Duration::from_millis(10),
        max_delay: std::time::Duration::from_millis(100),
    };
    let mut mgr = ConnectionManager::new(def.comms.clone(), cfg, factory);

    emit(ConnectionState::Connecting);
    let state = match mgr.connect() {
        Ok(state) => state,
        Err(error) => {
            let failed = match mgr.state() {
                state @ ConnectionState::Failed { .. } => state.clone(),
                _ => ConnectionState::Failed {
                    reason: error.to_string(),
                },
            };
            emit(failed);
            return Err(error.to_string());
        }
    };
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
    let serial_cfg = serial_config_from_comms(&comms);

    let factory: SerialFactory =
        Box::new(move || Ok(SerialTransport::new(port.clone(), serial_cfg.clone())));

    let cfg = ReconnectConfig {
        max_attempts: 10,
        base_delay: std::time::Duration::from_millis(500),
        max_delay: std::time::Duration::from_secs(30),
    };
    let mut mgr = ConnectionManager::new(comms, cfg, factory);

    emit(ConnectionState::Connecting);
    let state = match mgr.connect() {
        Ok(state) => state,
        Err(error) => {
            let failed = match mgr.state() {
                state @ ConnectionState::Failed { .. } => state.clone(),
                _ => ConnectionState::Failed {
                    reason: error.to_string(),
                },
            };
            emit(failed);
            return Err(error.to_string());
        }
    };
    emit(state);

    Ok(ActiveConnection::Serial { manager: mgr })
}

fn serial_config_from_comms(comms: &CommsSettings) -> SerialConfig {
    // A zero timeout would turn every read into an immediate false drop.
    SerialConfig {
        read_timeout: std::time::Duration::from_millis(u64::from(
            comms.block_read_timeout_ms.max(1),
        )),
        ..SerialConfig::default()
    }
}

// ── Unit tests ───────────────────────────────────────────────────────────────

/// Serializes every test (in this module and elsewhere, e.g.
/// `log_paths::tests`) that reads or mutates the process-wide `HOME`
/// env var — `cargo test` runs `#[test]` fns on multiple threads by
/// default, and `std::env::set_var`/`var` are unsynchronized process
/// globals, so two such tests running concurrently could otherwise observe
/// each other's transient value.
#[cfg(test)]
pub(crate) static HOME_ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
            och_block_size: 0,
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
            output_channels: Vec::new(),
            gauges: Vec::new(),
            frontpage: opentune_ini::FrontPageDef {
                gauge_slots: Vec::new(),
                indicators: Vec::new(),
            },
            ve_analyze: None,
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
        let _guard = HOME_ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/home/x");
        assert_eq!(expand_tilde("~/a/b.ini"), "/home/x/a/b.ini");
        assert_eq!(expand_tilde("~"), "/home/x");
        // Not a leading `~/` — left untouched.
        assert_eq!(expand_tilde("/abs/path"), "/abs/path");
        assert_eq!(expand_tilde("~user/x"), "~user/x");
        match original_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn decode_text_falls_back_to_latin1_for_ms_ini_bytes() {
        // MegaSquirt INIs are CP1252: `"°C"` is the raw byte 0xB0.
        assert_eq!(decode_text(b"\"\xB0C\"".to_vec()), "\"\u{B0}C\"");
        // Valid UTF-8 passes through unchanged (including multi-byte `°`).
        assert_eq!(
            decode_text("\"°C\" żółć".as_bytes().to_vec()),
            "\"°C\" żółć"
        );
    }

    #[test]
    fn bundled_ini_parses_correctly() {
        let ini = include_str!("../resources/speeduino.sample.ini");
        let comms = load_comms_from_str(ini).expect("bundled INI must parse");
        assert_eq!(comms.signature, EcuSimulator::SIGNATURE);
        assert_eq!(comms.query_command, "Q");
        assert_eq!(comms.envelope, EnvelopeFormat::Plain);
    }

    #[test]
    fn serial_config_uses_the_ini_block_read_timeout() {
        let mut comms = plain_comms();
        comms.block_read_timeout_ms = 3_750;
        let config = serial_config_from_comms(&comms);
        assert_eq!(config.read_timeout, std::time::Duration::from_millis(3_750));
        assert_eq!(config.write_timeout, SerialConfig::default().write_timeout);
    }

    #[test]
    fn simulator_signature_mismatch_emits_failed_instead_of_staying_connecting() {
        let mut def = plain_definition();
        def.comms.signature = "wrong firmware".to_string();
        let emitted = std::sync::Mutex::new(Vec::new());
        // `.err()` first: `ActiveConnection` (the Ok type) is a live-transport
        // enum with no `Debug`, so `expect_err` (which needs `T: Debug`) won't
        // compile — drop the Ok value before asserting on the message.
        let error = connect_simulator(&def, &|state| emitted.lock().unwrap().push(state))
            .err()
            .expect("wrong INI must fail");
        assert!(error.contains("signature mismatch"));
        assert!(matches!(
            emitted.into_inner().unwrap().as_slice(),
            [ConnectionState::Connecting, ConnectionState::Failed { .. }]
        ));
    }
}
