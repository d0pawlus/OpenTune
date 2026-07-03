// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for the `ConnectionManager` — the reliable-reconnect
//! feature (M1 pain point #1).

use opentune_ini::{CommsSettings, Endianness, EnvelopeFormat};
use opentune_protocol::{
    reconnect::{ConnectionManager, ReconnectConfig},
    ConnectionState, EcuIdentity,
};
use opentune_simulator::EcuSimulator;
use std::time::Duration;

fn speeduino_comms() -> CommsSettings {
    CommsSettings {
        signature: "speeduino 202504-dev".to_owned(),
        query_command: "Q".to_owned(),
        version_info: "S".to_owned(),
        och_get_command: "A".to_owned(),
        page_read_command: "p%2i%2o%2c".to_owned(),
        page_value_write: "M%2i%2o%2c%v".to_owned(),
        burn_command: "b%2i".to_owned(),
        blocking_factor: 121,
        page_activation_delay_ms: 0,
        block_read_timeout_ms: 200,
        inter_write_delay_ms: 0,
        endianness: Endianness::Little,
        envelope: EnvelopeFormat::Plain,
        och_block_size: 0,
    }
}

/// No delays so tests run fast.
fn fast_config() -> ReconnectConfig {
    ReconnectConfig {
        max_attempts: 5,
        base_delay: Duration::from_millis(0),
        max_delay: Duration::from_millis(0),
    }
}

// ── Test 1: initial connect reaches Connected ──────────────────────────────

#[test]
fn initial_connect_reaches_connected() {
    let sim = EcuSimulator::new();
    let comms = speeduino_comms();
    let mut mgr = ConnectionManager::new(comms.clone(), fast_config(), move || {
        Ok(sim.client_transport())
    });

    let state = mgr.connect().unwrap();
    assert!(
        matches!(state, ConnectionState::Connected { .. }),
        "expected Connected after initial handshake, got: {state:?}"
    );
}

// ── Test 2: drop → Reconnecting state emitted, then Connected ──────────────
//
// Uses `EcuSimulator::shared()` to share the sim across the factory closure
// and the test body. The factory gets a new client on each call; the test body
// controls the drop flag via the shared reference.

#[test]
fn drop_link_transitions_to_reconnecting_then_connected() {
    use opentune_simulator::EcuSimulator;
    use std::sync::Arc;

    let sim = Arc::new(EcuSimulator::new());
    let sim_factory = Arc::clone(&sim);
    let comms = speeduino_comms();

    let mut call_count = 0u32;
    let mut mgr = ConnectionManager::new(comms.clone(), fast_config(), move || {
        call_count += 1;
        if call_count > 1 {
            // Restore link so the reconnect attempt can succeed.
            sim_factory.set_link_dropped(false);
        }
        Ok(sim_factory.client_transport())
    });

    // Establish initial connection.
    mgr.connect().unwrap();

    // Drop the link.
    sim.set_link_dropped(true);

    // Run reconnect and collect states.
    let states = mgr.reconnect_collect_states();

    let has_reconnecting = states
        .iter()
        .any(|s| matches!(s, ConnectionState::Reconnecting { .. }));
    assert!(
        has_reconnecting,
        "expected at least one Reconnecting state, got: {states:?}"
    );

    let final_state = states.last().unwrap();
    assert!(
        matches!(final_state, ConnectionState::Connected { .. }),
        "expected final state Connected, got: {final_state:?}"
    );
}

// ── Test 3: secl resync — glitch path (secl advanced, no reboot) ───────────

#[test]
fn secl_glitch_does_not_trigger_reidentify() {
    use std::sync::Arc;

    let sim = Arc::new(EcuSimulator::new());
    let sim_factory = Arc::clone(&sim);
    let comms = speeduino_comms();

    let mut call_count = 0u32;
    let mut mgr = ConnectionManager::new(comms.clone(), fast_config(), move || {
        call_count += 1;
        if call_count > 1 {
            sim_factory.set_link_dropped(false);
        }
        Ok(sim_factory.client_transport())
    });

    mgr.connect().unwrap();

    // Advance secl on the sim (glitch — ECU kept running).
    sim.advance_secl(10);
    sim.set_link_dropped(true);

    let states = mgr.reconnect_collect_states();
    assert!(
        matches!(states.last().unwrap(), ConnectionState::Connected { .. }),
        "glitch reconnect must end Connected"
    );
    // A glitch (secl advanced) must NOT trigger re-identify.
    assert!(
        !mgr.last_reconnect_caused_reidentify(),
        "secl advancing (glitch) must not trigger re-identify"
    );
}

// ── Test 4: secl resync — reboot path (secl went backwards) ───────────────
//
// Scenario: the ECU is at secl=50 when we first connect (so last_secl=50 is
// captured). Then the link drops, the ECU reboots (secl resets to 0).
// After reconnect, new_secl=0 < last_secl=50 → reboot detected.

#[test]
fn secl_reboot_triggers_reidentify() {
    use std::sync::Arc;

    let sim = Arc::new(EcuSimulator::new());
    // Advance secl BEFORE the initial connect so last_secl is non-zero.
    sim.advance_secl(50);

    let sim_factory = Arc::clone(&sim);
    let comms = speeduino_comms();

    let mut call_count = 0u32;
    let mut mgr = ConnectionManager::new(comms.clone(), fast_config(), move || {
        call_count += 1;
        if call_count > 1 {
            // Simulate reboot: reset secl to 0, restore link.
            sim_factory.reset_secl();
            sim_factory.set_link_dropped(false);
        }
        Ok(sim_factory.client_transport())
    });

    // Connect while secl=50 — manager captures last_secl=50.
    mgr.connect().unwrap();
    assert_eq!(
        mgr.last_secl(),
        50,
        "last_secl must reflect secl at connect time"
    );

    // Drop the link (ECU will "reboot" on reconnect via factory closure).
    sim.set_link_dropped(true);

    let states = mgr.reconnect_collect_states();
    assert!(
        matches!(states.last().unwrap(), ConnectionState::Connected { .. }),
        "reboot reconnect must end Connected"
    );
    // new_secl=0 < last_secl=50 → reboot detected.
    assert!(
        mgr.last_reconnect_caused_reidentify(),
        "secl going backwards (reboot) must trigger re-identify"
    );
}

// ── Test 5: exhausted attempts → Failed ───────────────────────────────────

#[test]
fn exhausted_attempts_transition_to_failed() {
    use std::sync::Arc;

    let sim = Arc::new(EcuSimulator::new());
    // Drop immediately so ALL reconnect attempts fail.
    sim.set_link_dropped(true);

    let sim_factory = Arc::clone(&sim);
    let tight = ReconnectConfig {
        max_attempts: 3,
        base_delay: Duration::from_millis(0),
        max_delay: Duration::from_millis(0),
    };

    let mut mgr = ConnectionManager::new(speeduino_comms(), tight, move || {
        Ok(sim_factory.client_transport())
    });

    // Force the manager into Connected so reconnect_collect_states starts its loop.
    mgr.force_state_for_test(ConnectionState::Connected {
        identity: EcuIdentity {
            signature: EcuSimulator::SIGNATURE.to_owned(),
            version: EcuSimulator::VERSION.to_owned(),
        },
    });

    let states = mgr.reconnect_collect_states();
    let final_state = states.last().unwrap();
    assert!(
        matches!(final_state, ConnectionState::Failed { .. }),
        "expected Failed after exhausted attempts, got: {final_state:?}"
    );
}

// ── Test 6: state() observable after successful reconnect ─────────────────

#[test]
fn state_is_connected_after_successful_reconnect() {
    use std::sync::Arc;

    let sim = Arc::new(EcuSimulator::new());
    let sim_factory = Arc::clone(&sim);
    let comms = speeduino_comms();

    let mut call_count = 0u32;
    let mut mgr = ConnectionManager::new(comms.clone(), fast_config(), move || {
        call_count += 1;
        if call_count > 1 {
            sim_factory.set_link_dropped(false);
        }
        Ok(sim_factory.client_transport())
    });

    mgr.connect().unwrap();
    assert!(matches!(mgr.state(), ConnectionState::Connected { .. }));

    sim.set_link_dropped(true);
    let _ = mgr.reconnect_collect_states();

    assert!(
        matches!(mgr.state(), ConnectionState::Connected { .. }),
        "state() must return Connected after recovery, got: {:?}",
        mgr.state()
    );
}
