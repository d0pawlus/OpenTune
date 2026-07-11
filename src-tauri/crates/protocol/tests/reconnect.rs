// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for the `ConnectionManager` — the reliable-reconnect
//! feature (M1 pain point #1).

use opentune_ini::{CommsSettings, Endianness, EnvelopeFormat};
use opentune_protocol::{
    reconnect::{ConnectionManager, ReconnectConfig},
    ConnectionState, EcuIdentity,
};
use opentune_simulator::EcuSimulator;
use opentune_transport::{Transport, TransportError};
use std::collections::VecDeque;
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

// ── Test 4b: note_secl keeps the reboot baseline live while polling ────────
//
// M3 Task 6 blocker c: once the realtime poll loop sends 'r' requests, secl
// can legitimately move backwards without a reboot — the firmware zeroes it
// on the FIRST och request (comms.cpp:361-365), and the u8 counter wraps
// every 256 s. A stale connect-time baseline then makes the next glitch
// reconnect read `new_secl < last_secl` → false reboot → the owner re-reads
// the tune and silently discards unburned edits. The owner therefore feeds
// byte 0 of every successfully polled block into `note_secl`.

#[test]
fn note_secl_prevents_false_reboot_after_secl_moved_backwards_without_reboot() {
    use std::sync::Arc;

    let sim = Arc::new(EcuSimulator::new());
    // Connect while secl = 50 — baseline 50.
    sim.advance_secl(50);

    let sim_factory = Arc::clone(&sim);
    let mut call_count = 0u32;
    let mut mgr = ConnectionManager::new(speeduino_comms(), fast_config(), move || {
        call_count += 1;
        if call_count > 1 {
            sim_factory.set_link_dropped(false);
        }
        Ok(sim_factory.client_transport())
    });
    mgr.connect().unwrap();
    assert_eq!(mgr.last_secl(), 50);

    // Polling sees the counter at 10 (post-zero / post-wrap — NO reboot);
    // the owner feeds each polled block's byte 0 to the manager.
    sim.reset_secl();
    sim.advance_secl(10);
    mgr.note_secl(10);

    // A cable glitch: without note_secl the reconnect would read 10 < 50 →
    // false reboot → tune re-read → unburned edits discarded upstream.
    sim.set_link_dropped(true);
    let states = mgr.reconnect_collect_states();
    assert!(
        matches!(states.last().unwrap(), ConnectionState::Connected { .. }),
        "glitch reconnect must end Connected"
    );
    assert!(
        !mgr.last_reconnect_caused_reidentify(),
        "a polled-forward baseline must not read as a reboot"
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

#[test]
fn initial_signature_mismatch_is_rejected() {
    let sim = EcuSimulator::new();
    let mut comms = speeduino_comms();
    comms.signature = "different firmware".to_string();
    let mut mgr = ConnectionManager::new(comms, fast_config(), move || Ok(sim.client_transport()));

    let error = mgr.connect().expect_err("wrong INI must not connect");
    assert!(matches!(
        error,
        opentune_protocol::ProtocolError::SignatureMismatch { .. }
    ));
    assert!(matches!(mgr.state(), ConnectionState::Failed { .. }));
}

#[test]
fn health_check_detects_a_dropped_live_link() {
    use std::sync::Arc;

    let sim = Arc::new(EcuSimulator::new());
    let sim_factory = Arc::clone(&sim);
    let mut mgr = ConnectionManager::new(speeduino_comms(), fast_config(), move || {
        Ok(sim_factory.client_transport())
    });
    mgr.connect().unwrap();

    sim.set_link_dropped(true);
    assert!(matches!(
        mgr.check_link(),
        Err(opentune_protocol::ProtocolError::Transport(
            TransportError::Disconnected
        ))
    ));
}

#[test]
fn health_check_routes_a_backwards_counter_through_reconnect() {
    use std::sync::Arc;

    let sim = Arc::new(EcuSimulator::new());
    sim.advance_secl(50);
    let sim_factory = Arc::clone(&sim);
    let mut mgr = ConnectionManager::new(speeduino_comms(), fast_config(), move || {
        Ok(sim_factory.client_transport())
    });
    mgr.connect().unwrap();
    assert_eq!(mgr.last_secl(), 50);

    sim.reset_secl();
    let error = mgr
        .check_link()
        .expect_err("backwards secl must trigger owner recovery");
    assert!(matches!(
        error,
        opentune_protocol::ProtocolError::MalformedResponse(_)
    ));
    assert_eq!(
        mgr.last_secl(),
        50,
        "reconnect still needs the old baseline to detect reboot"
    );
}

#[test]
fn health_check_accepts_u8_counter_wrap() {
    use std::sync::Arc;

    let sim = Arc::new(EcuSimulator::new());
    sim.advance_secl(250);
    let sim_factory = Arc::clone(&sim);
    let mut mgr = ConnectionManager::new(speeduino_comms(), fast_config(), move || {
        Ok(sim_factory.client_transport())
    });
    mgr.connect().unwrap();

    sim.reset_secl();
    sim.advance_secl(1);
    assert_eq!(mgr.check_link().unwrap(), 1);
}

/// Tiny plain-protocol transport whose identity can differ between factory
/// calls. This pins the reconnect-only signature guard without hardware.
struct IdentityTransport {
    open: bool,
    signature: String,
    response: VecDeque<u8>,
}

impl IdentityTransport {
    fn new(signature: &str) -> Self {
        Self {
            open: false,
            signature: signature.to_string(),
            response: VecDeque::new(),
        }
    }
}

impl Transport for IdentityTransport {
    fn open(&mut self) -> opentune_transport::Result<()> {
        self.open = true;
        Ok(())
    }

    fn close(&mut self) -> opentune_transport::Result<()> {
        self.open = false;
        Ok(())
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn write(&mut self, bytes: &[u8]) -> opentune_transport::Result<()> {
        if !self.open {
            return Err(TransportError::Disconnected);
        }
        let reply: Vec<u8> = match bytes {
            b"Q" => self
                .signature
                .as_bytes()
                .iter()
                .copied()
                .chain(std::iter::once(0))
                .collect(),
            b"S" => b"test-version\0".to_vec(),
            b"A" => vec![1],
            _ => Vec::new(),
        };
        self.response.extend(reply);
        Ok(())
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> opentune_transport::Result<()> {
        for byte in buf {
            *byte = self.response.pop_front().ok_or_else(|| {
                TransportError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "scripted response exhausted",
                ))
            })?;
        }
        Ok(())
    }

    fn flush(&mut self) -> opentune_transport::Result<()> {
        self.response.clear();
        Ok(())
    }
}

#[test]
fn reconnect_signature_mismatch_fails_without_retrying() {
    let expected = speeduino_comms().signature.clone();
    let mut factory_calls = 0;
    let mut mgr = ConnectionManager::new(speeduino_comms(), fast_config(), move || {
        factory_calls += 1;
        let signature = if factory_calls == 1 {
            expected.as_str()
        } else {
            "different firmware"
        };
        Ok(IdentityTransport::new(signature))
    });
    mgr.connect().unwrap();

    let states = mgr.reconnect_collect_states();
    assert_eq!(states.len(), 2, "one attempt plus terminal failure");
    assert!(matches!(
        states.as_slice(),
        [
            ConnectionState::Reconnecting { attempt: 1 },
            ConnectionState::Failed { .. }
        ]
    ));
    let ConnectionState::Failed { reason } = states.last().unwrap() else {
        unreachable!()
    };
    assert!(reason.contains("signature mismatch"));
}
