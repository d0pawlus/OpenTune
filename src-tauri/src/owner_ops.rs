// SPDX-License-Identifier: GPL-3.0-or-later
//! Blocking operation bodies for the §9 owner task, split from `owner.rs`
//! for file cohesion. Everything here runs on the blocking pool (via the
//! owner's `spawn_blocking`) because the transport is synchronous.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use opentune_ini::Definition;
use opentune_model::Tune;
use opentune_protocol::ConnectionState;

use super::{Emitter, OwnerEvent, RecoveryOutcome};
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
        offline_origin: false,
    })
}

/// Blocking recovery body used after a real owner/session link error.
/// Runs the M1 reconnect loop for either transport, streaming every state to
/// the UI the moment it is produced — the full serial backoff schedule is
/// worth ~150 s, so batching would freeze the frontend's attempt counter for
/// the whole window. `cancel` is the owner's Disconnect flag: the loop
/// observes it between attempts and backoff chunks and settles promptly,
/// without a terminal `Failed` (the Disconnect already owns the UI state).
///
/// `fail_tune_reread` is the test hook forcing the post-reboot tune re-read
/// to fail (always `false` in production, same plumbing as [`link_drop`]).
pub(super) fn reconnect_session(
    mut session: Session,
    emit: &Emitter,
    cancel: &AtomicBool,
    fail_tune_reread: bool,
) -> (Option<Session>, RecoveryOutcome) {
    let Some(conn) = session.conn.as_mut() else {
        return (
            Some(session),
            RecoveryOutcome::Failed("cannot reconnect without an active connection".to_string()),
        );
    };
    let on_state = |s: &ConnectionState| {
        emit(OwnerEvent::Connection(ConnectionStateEvent::from(
            s.clone(),
        )));
    };
    let cancelled = || cancel.load(Ordering::Relaxed);
    let (states, rebooted) = match conn {
        ActiveConnection::Sim { manager, .. } => {
            let states = manager.reconnect_streaming(on_state, cancelled, |_| {});
            (states, manager.last_reconnect_caused_reidentify())
        }
        ActiveConnection::Serial { manager } => {
            let states = manager.reconnect_streaming(on_state, cancelled, |_| {});
            (states, manager.last_reconnect_caused_reidentify())
        }
    };
    finish_reconnect(session, states, rebooted, fail_tune_reread, emit)
}

/// Blocking simulator demo body: keep the link down through the first attempt,
/// restore it from the retry hook, then complete through the same recovery path
/// used for real transport errors.
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
    fail_tune_reread: bool,
) -> (Option<Session>, Result<(), String>) {
    let Some(ActiveConnection::Sim { manager, simulator }) = &mut session.conn else {
        return (Some(session), Err(SIM_ONLY.to_owned()));
    };

    simulator.set_link_dropped(true);
    let restore = Arc::clone(simulator);
    // Live-streamed states plus the retry hook restoring the deliberately
    // dropped link; never cancelled — the demo runs to a terminal state
    // inside the owner's awaited call.
    let states = manager.reconnect_streaming(
        |s| {
            emit(OwnerEvent::Connection(ConnectionStateEvent::from(
                s.clone(),
            )))
        },
        || false,
        move |attempt| {
            if attempt == 1 {
                restore.set_link_dropped(false);
            }
        },
    );
    let rebooted = manager.last_reconnect_caused_reidentify();
    let (session, outcome) = finish_reconnect(session, states, rebooted, fail_tune_reread, emit);
    (session, outcome.into_result())
}

fn finish_reconnect(
    mut session: Session,
    states: Vec<ConnectionState>,
    rebooted: bool,
    fail_tune_reread: bool,
    emit: &Emitter,
) -> (Option<Session>, RecoveryOutcome) {
    // The states were already streamed to the UI by the reconnect loop's
    // observer — only the outcome is derived here. A missing terminal state
    // means the loop was cancelled: it never pushes `Failed` on cancel.
    let outcome = match states.last() {
        Some(ConnectionState::Connected { .. }) => RecoveryOutcome::Connected,
        Some(ConnectionState::Failed { reason }) => RecoveryOutcome::Failed(reason.clone()),
        _ => RecoveryOutcome::Cancelled,
    };
    if !matches!(outcome, RecoveryOutcome::Connected) {
        return (Some(session), outcome);
    }

    if rebooted {
        // A snapshot belongs to the ECU state that existed before the reboot
        // and can never remain a valid merge baseline afterward — clear it
        // even when the tune re-read below succeeds.
        session.snapshot = None;
        if session.tune.is_some() {
            let reread = if fail_tune_reread {
                Err("forced tune re-read failure".to_owned())
            } else {
                session.load_tune()
            };
            match reread {
                Ok(ev) => emit(OwnerEvent::TuneDirty(ev)),
                Err(e) => {
                    // Keep the successfully reconnected transport, but never
                    // expose the pre-reboot tune or snapshot as current.
                    session.tune = None;
                    session.snapshot = None;
                    return (
                        Some(session),
                        RecoveryOutcome::ConnectedButTuneRereadFailed(format!(
                            "tune re-read after ECU reboot failed: {e}"
                        )),
                    );
                }
            }
        }
    }
    (Some(session), RecoveryOutcome::Connected)
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
        offline_origin: true,
    })
}

/// Build an offline session and load a `.msq` into its tune (signature-checked).
///
/// `load_msq_into` applies every constant through `Tune::set` — the normal
/// edit path — so a freshly parsed `.msq` re-baselines via `mark_loaded`
/// once it succeeds: opening a file is not an edit, and the tune must read
/// clean with an empty undo/redo history, exactly like `load_page`'s
/// contract. A partial apply (`MsqReport.failed` non-empty) still returns
/// `Ok` from `load_msq_into` — the tune is re-baselined to whatever landed;
/// the report (surfaced separately) carries the per-constant failures.
pub(super) fn build_offline_session_from_msq(
    ini_path: &str,
    msq_path: &str,
) -> Result<Session, String> {
    let mut session = build_offline_session(ini_path)?;
    let xml = crate::connection::read_text(msq_path)
        .map_err(|e| format!("cannot read `{msq_path}`: {e}"))?;
    let tune = session
        .tune
        .as_mut()
        .ok_or_else(|| "internal error: offline session missing tune".to_string())?;
    opentune_project::msq::load_msq_into(tune, &xml).map_err(|e| e.to_string())?;
    tune.mark_loaded();
    Ok(session)
}

/// Attach a live link to an existing offline session **without** reading the
/// tune (which would overwrite the user's offline edits). Verifies the ECU
/// signature matches the offline tune's INI before attaching.
pub(super) fn attach_connection(
    session: &mut Session,
    source: ConnectSource,
    emit: &Emitter,
) -> Result<(), String> {
    let emit_cs =
        |cs: ConnectionState| emit(OwnerEvent::Connection(ConnectionStateEvent::from(cs)));
    let conn = match source {
        // ATTACH ignores the source's ini_path: the offline def is authoritative.
        ConnectSource::Simulator { .. } => connect_simulator(session.def.as_ref(), &emit_cs)?,
        ConnectSource::Serial { ref port_name, .. } => {
            connect_serial(port_name.clone(), session.def.comms.clone(), &emit_cs)?
        }
    };
    verify_signature(&conn, &session.def)?;
    session.conn = Some(conn);
    Ok(())
}

/// Guard #2: the connected ECU's signature must match the tune's INI. Thin
/// convenience wrapper over [`ActiveConnection::verify_signature`] (the single
/// source of truth, shared with `Session::write_all_to_ecu`'s pre-write check).
fn verify_signature(conn: &ActiveConnection, def: &Definition) -> Result<(), String> {
    conn.verify_signature(&def.comms)
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

    /// M2/M3 review fix wave item 1: `load_msq_into` applies every constant
    /// via `Tune::set` — the normal edit path — so without an explicit
    /// re-baseline a freshly opened `.msq` would read dirty and carry an
    /// undo stack of pseudo-edits that silently walk values toward the
    /// INI-blank default. A reopened session's tune must read clean, and
    /// undo must be a no-op that changes nothing.
    #[test]
    fn reopened_msq_session_tune_is_clean_and_undo_is_a_no_op() {
        let scratch = ScratchFile::new("reopen-clean");

        let mut session = build_offline_session(INI).unwrap();
        let tune = session.tune.as_mut().unwrap();
        tune.set("reqFuel", opentune_model::Value::Scalar(14.7))
            .unwrap();
        let xml = opentune_project::msq::tune_to_msq(tune);
        std::fs::write(&scratch.0, xml).unwrap();

        let mut reopened =
            build_offline_session_from_msq(INI, scratch.0.to_str().unwrap()).unwrap();
        let reopened_tune = reopened.tune.as_mut().unwrap();
        assert!(
            !reopened_tune.is_dirty(),
            "a freshly opened .msq must read clean, not as a pile of pseudo-edits"
        );

        let before = reopened_tune.get("reqFuel").unwrap();
        assert!(
            !reopened_tune.undo(),
            "undo stack must be empty after opening a .msq"
        );
        assert_eq!(
            reopened_tune.get("reqFuel").unwrap(),
            before,
            "undo must not silently change a value the user never touched"
        );
    }

    #[test]
    fn attach_keeps_the_offline_tune_and_pushes_to_sim() {
        use crate::owner::Emitter;
        use std::sync::Arc;

        let mut s = build_offline_session(INI).unwrap();
        let emit: Emitter = Arc::new(|_| {});
        attach_connection(&mut s, ConnectSource::Simulator { ini_path: None }, &emit).unwrap();
        assert!(s.conn.is_some(), "attach adds a connection");
        assert!(s.tune.is_some(), "attach never drops the offline tune");
        // The simulator accepts a whole-tune write + burn.
        s.write_all_to_ecu().unwrap();
    }

    /// A `Serial` `ActiveConnection` whose manager's state is force-set (no
    /// real port is ever opened — the factory is never invoked) so
    /// `verify_signature`'s serial arm can be exercised hardware-free.
    fn serial_conn_with_state(state: ConnectionState) -> ActiveConnection {
        use opentune_protocol::reconnect::{ConnectionManager, ReconnectConfig};
        use opentune_transport::serial::SerialTransport;

        let def = build_offline_session(INI).unwrap().def;
        let factory: Box<dyn FnMut() -> opentune_protocol::Result<SerialTransport> + Send> =
            Box::new(|| unreachable!("factory must not run; state is forced for the test"));
        let mut manager =
            ConnectionManager::new(def.comms.clone(), ReconnectConfig::default(), factory);
        manager.force_state_for_test(state);
        ActiveConnection::Serial { manager }
    }

    #[test]
    fn verify_signature_rejects_a_serial_signature_mismatch() {
        use opentune_protocol::EcuIdentity;

        let def = build_offline_session(INI).unwrap().def;
        let conn = serial_conn_with_state(ConnectionState::Connected {
            identity: EcuIdentity {
                signature: "some-other-ecu".to_string(),
                version: String::new(),
            },
        });
        let err = verify_signature(&conn, &def).unwrap_err();
        assert!(
            err.contains("does not match"),
            "expected a signature-mismatch error, got: {err}"
        );
    }

    #[test]
    fn verify_signature_accepts_a_matching_serial_signature() {
        use opentune_protocol::EcuIdentity;

        let def = build_offline_session(INI).unwrap().def;
        let conn = serial_conn_with_state(ConnectionState::Connected {
            identity: EcuIdentity {
                signature: def.comms.signature.clone(),
                version: String::new(),
            },
        });
        verify_signature(&conn, &def).expect("matching signature must verify");
    }

    #[test]
    fn verify_signature_rejects_a_serial_connection_with_no_reported_identity() {
        let def = build_offline_session(INI).unwrap().def;
        let conn = serial_conn_with_state(ConnectionState::Connecting);
        let err = verify_signature(&conn, &def).unwrap_err();
        assert!(
            err.contains("did not report a signature"),
            "expected the no-identity error, got: {err}"
        );
    }
}
