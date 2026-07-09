// SPDX-License-Identifier: GPL-3.0-or-later
//! Blocking operation bodies for the §9 owner task, split from `owner.rs`
//! for file cohesion. Everything here runs on the blocking pool (via the
//! owner's `spawn_blocking`) because the transport is synchronous.

use std::sync::Arc;

use opentune_model::Tune;
use opentune_protocol::ConnectionState;

use super::{Emitter, OwnerEvent};
use crate::connection::{
    connect_serial, connect_simulator, load_definition_from_path, load_definition_from_str,
    ActiveConnection, ConnectSource, Session,
};
use crate::events::ConnectionStateEvent;

const BUNDLED_INI: &str = include_str!("../resources/speeduino.sample.ini");
const SIM_ONLY: &str = "simulate_link_drop is only available in simulator mode";

/// Blocking connect body: parse the INI, open the transport, handshake.
/// `Connecting`/`Connected` are emitted from inside the handshake so a slow
/// serial connect still shows live progress.
pub(super) fn build_session(source: ConnectSource, emit: &Emitter) -> Result<Session, String> {
    let emit_cs = |cs: ConnectionState| {
        emit(OwnerEvent::Connection(ConnectionStateEvent::from(cs)));
    };
    let (conn, def) = match source {
        ConnectSource::Simulator { ini_path } => {
            let def = Arc::new(match ini_path {
                Some(ref path) => load_definition_from_path(path)?,
                None => load_definition_from_str(BUNDLED_INI)?,
            });
            let conn = connect_simulator(def.as_ref(), &emit_cs)?;
            (conn, def)
        }
        ConnectSource::Serial {
            ref port_name,
            ref ini_path,
        } => {
            let def = Arc::new(load_definition_from_path(ini_path)?);
            let conn = connect_serial(port_name.clone(), def.comms.clone(), &emit_cs)?;
            (conn, def)
        }
    };
    Ok(Session {
        conn: Some(conn),
        def,
        tune: None,
        snapshot: None,
    })
}

/// Blocking link-drop body: drop + restore the simulator link, run the M1
/// reconnect loop, and emit every state it produced. The session is always
/// handed back, even when the reconnect ends `Failed`.
///
/// Reboot re-read (M2 follow-up c): when the reconnect detected an ECU
/// reboot (`last_reconnect_caused_reidentify` — secl went backwards), the
/// in-memory tune may diverge from the rebooted ECU (unburned RAM writes are
/// gone), so it is invalidated and re-read. On a glitch reconnect the tune —
/// including unburned edits — is deliberately preserved: re-reading would
/// silently discard the user's work.
pub(super) fn link_drop(
    mut session: Session,
    emit: &Emitter,
) -> (Option<Session>, Result<(), String>) {
    let Some(ActiveConnection::Sim { manager, simulator }) = &mut session.conn else {
        return (Some(session), Err(SIM_ONLY.to_owned()));
    };

    simulator.set_link_dropped(true);
    // Restore immediately so the first reconnect attempt succeeds (M2 note).
    simulator.set_link_dropped(false);

    let states = manager.reconnect_collect_states();
    let reconnected = matches!(states.last(), Some(ConnectionState::Connected { .. }));
    for s in states {
        emit(OwnerEvent::Connection(ConnectionStateEvent::from(s)));
    }

    let rebooted = manager.last_reconnect_caused_reidentify();
    if reconnected && rebooted && session.tune.is_some() {
        match session.load_tune() {
            Ok(ev) => emit(OwnerEvent::TuneDirty(ev)),
            Err(e) => {
                return (
                    Some(session),
                    Err(format!("tune re-read after ECU reboot failed: {e}")),
                );
            }
        }
    }
    (Some(session), Ok(()))
}

/// Build an offline session (no ECU link) around a blank tune from `ini_path`.
pub(super) fn build_offline_session(ini_path: &str) -> Result<Session, String> {
    let def = Arc::new(load_definition_from_path(ini_path)?);
    let tune = Tune::new(Arc::clone(&def));
    Ok(Session {
        conn: None,
        def,
        tune: Some(tune),
        snapshot: None,
    })
}

/// Build an offline session and load a `.msq` into its tune (signature-checked).
pub(super) fn build_offline_session_from_msq(
    ini_path: &str,
    msq_path: &str,
) -> Result<Session, String> {
    let mut session = build_offline_session(ini_path)?;
    let xml =
        std::fs::read_to_string(msq_path).map_err(|e| format!("cannot read `{msq_path}`: {e}"))?;
    let tune = session
        .tune
        .as_mut()
        .ok_or_else(|| "internal error: offline session missing tune".to_string())?;
    opentune_project::msq::load_msq_into(tune, &xml).map_err(|e| e.to_string())?;
    Ok(session)
}

#[cfg(test)]
mod offline_build_tests {
    use super::*;

    const INI: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/speeduino.sample.ini"
    );

    #[test]
    fn build_offline_session_has_no_conn_and_a_tune() {
        let s = build_offline_session(INI).unwrap();
        assert!(s.conn.is_none());
        assert!(s.tune.is_some());
        assert!(!s.def.comms.signature.is_empty());
    }

    /// A scratch file path (std-only — no tempfile dev-dependency), removed
    /// on drop so a failing assertion doesn't leak files across runs (same
    /// pattern as `layout::tests::ScratchDir`).
    struct ScratchFile(std::path::PathBuf);

    impl ScratchFile {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "opentune-offline-msq-{tag}-{}-{:?}.msq",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_file(&path);
            Self(path)
        }
    }

    impl Drop for ScratchFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// The point of the feature: an edit made offline survives a
    /// save (`tune_to_msq`) → reopen (`build_offline_session_from_msq`)
    /// round trip.
    #[test]
    fn save_then_reopen_round_trips_an_edited_value() {
        let scratch = ScratchFile::new("roundtrip");

        let mut session = build_offline_session(INI).unwrap();
        let tune = session.tune.as_mut().unwrap();
        tune.set("reqFuel", opentune_model::Value::Scalar(14.7))
            .unwrap();
        let xml = opentune_project::msq::tune_to_msq(tune);
        std::fs::write(&scratch.0, xml).unwrap();

        let reopened = build_offline_session_from_msq(INI, scratch.0.to_str().unwrap()).unwrap();
        assert!(reopened.conn.is_none());
        let reopened_tune = reopened.tune.as_ref().unwrap();
        // Round-trips through a scaled U16 raw encoding — compare with
        // tolerance rather than bit-for-bit equality.
        match reopened_tune.get("reqFuel").unwrap() {
            opentune_model::Value::Scalar(v) => assert!(
                (v - 14.7).abs() < 1e-6,
                "expected reqFuel ≈ 14.7 after round trip, got {v}"
            ),
            other => panic!("expected a scalar reqFuel, got {other:?}"),
        }
    }
}
