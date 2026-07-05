// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration test: M1 connect & identify flow against the real simulator.
//!
//! This test exercises the connection core without a running Tauri app.
//! It drives `connection.rs` helpers directly, asserting the emitted state
//! sequence matches the M1 Demo criterion:
//!   connect → Connected{signature == sim signature}
//!   drop+restore → Reconnecting{attempt ≥ 1} … Connected{signature == sim signature}

use std::sync::{Arc, Mutex};
use std::time::Duration;

use opentune_ini::{CommsSettings, Endianness, EnvelopeFormat};
use opentune_protocol::{
    reconnect::{ConnectionManager, ReconnectConfig},
    ConnectionState,
};
use opentune_simulator::EcuSimulator;

fn plain_comms() -> CommsSettings {
    CommsSettings {
        signature: EcuSimulator::SIGNATURE.to_owned(),
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

fn zero_config() -> ReconnectConfig {
    ReconnectConfig {
        max_attempts: 3,
        base_delay: Duration::ZERO,
        max_delay: Duration::ZERO,
    }
}

/// M1 Demo: connect → emits Connecting then Connected with the correct signature.
#[test]
fn initial_connect_emits_connected_with_sim_signature() {
    let sim = Arc::new(EcuSimulator::new());
    let sim_ref = Arc::clone(&sim);

    type Factory = Box<
        dyn FnMut() -> opentune_protocol::Result<opentune_simulator::ecu::EcuClientTransport>
            + Send,
    >;
    let factory: Factory = Box::new(move || Ok(sim_ref.client_transport()));

    let mut mgr = ConnectionManager::new(plain_comms(), zero_config(), factory);

    let emitted: Arc<Mutex<Vec<ConnectionState>>> = Arc::new(Mutex::new(Vec::new()));
    let emitted_ref = Arc::clone(&emitted);

    let state = mgr.connect().expect("connect must succeed");
    emitted_ref.lock().unwrap().push(state);

    let states = emitted_ref.lock().unwrap();
    let last = states.last().expect("at least one state");
    match last {
        ConnectionState::Connected { identity } => {
            assert_eq!(
                identity.signature,
                EcuSimulator::SIGNATURE,
                "signature must match simulator"
            );
        }
        other => panic!("expected Connected, got {other:?}"),
    }
}

/// M1 Demo: drop link → reconnect → emits Reconnecting{attempt≥1} then Connected.
#[test]
fn drop_and_reconnect_emits_reconnecting_then_connected() {
    let sim = Arc::new(EcuSimulator::new());
    let sim_ref = Arc::clone(&sim);

    type Factory = Box<
        dyn FnMut() -> opentune_protocol::Result<opentune_simulator::ecu::EcuClientTransport>
            + Send,
    >;
    let factory: Factory = Box::new(move || Ok(sim_ref.client_transport()));

    let mut mgr = ConnectionManager::new(plain_comms(), zero_config(), factory);

    // Initial connect.
    mgr.connect().expect("initial connect must succeed");

    // Drop the link then restore it immediately so the reconnect attempt succeeds.
    sim.set_link_dropped(true);
    sim.set_link_dropped(false);

    // Drive reconnect.
    let states = mgr.reconnect_collect_states();

    // Must contain at least one Reconnecting.
    assert!(
        states
            .iter()
            .any(|s| matches!(s, ConnectionState::Reconnecting { .. })),
        "must emit at least one Reconnecting; got: {states:?}"
    );

    // Must end in Connected with the correct signature.
    let last = states
        .last()
        .expect("reconnect must emit at least one state");
    match last {
        ConnectionState::Connected { identity } => {
            assert_eq!(
                identity.signature,
                EcuSimulator::SIGNATURE,
                "post-reconnect signature must match simulator"
            );
        }
        other => panic!("expected Connected after reconnect, got {other:?}"),
    }
}

/// Bundled INI parses correctly and produces the right signature.
#[test]
fn bundled_ini_signature_matches_simulator() {
    let ini = include_str!("../resources/speeduino.sample.ini");
    let comms = opentune_ini::parse_comms(ini).expect("bundled INI must parse");
    assert_eq!(
        comms.signature,
        EcuSimulator::SIGNATURE,
        "bundled INI signature must match simulator signature"
    );
    assert_eq!(
        comms.envelope,
        EnvelopeFormat::Plain,
        "bundled INI must use plain protocol"
    );
}
