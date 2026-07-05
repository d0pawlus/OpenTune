// SPDX-License-Identifier: GPL-3.0-or-later
//! Blocking operation bodies for the §9 owner task, split from `owner.rs`
//! for file cohesion. Everything here runs on the blocking pool (via the
//! owner's `spawn_blocking`) because the transport is synchronous.

use std::sync::Arc;

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
        conn,
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
    let ActiveConnection::Sim { manager, simulator } = &mut session.conn else {
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
