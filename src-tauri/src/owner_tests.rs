// SPDX-License-Identifier: GPL-3.0-or-later
//! Owner-task unit tests (`#[tokio::test]`), split from `owner.rs` for file
//! cohesion. Events are collected via an injected [`Emitter`] closure; the
//! simulator is reached through the test-only [`Command::DebugSimulator`].

use std::sync::Mutex;

use super::*;

type Collected = Arc<Mutex<Vec<OwnerEvent>>>;

/// Spawn an owner whose events land in a shared Vec.
fn test_owner() -> (OwnerHandle, Collected) {
    let events: Collected = Arc::default();
    let sink = Arc::clone(&events);
    let emit: Emitter = Arc::new(move |ev| sink.lock().unwrap().push(ev));
    (spawn_owner_with_emitter(emit), events)
}

/// Send one command and await its oneshot reply (the production `request`
/// path — a dead owner surfaces as an `Err`, failing the calling assert).
async fn send<T>(tx: &OwnerHandle, make: impl FnOnce(Reply<T>) -> Command) -> Result<T, String> {
    request(tx, make).await
}

/// Connect to the bundled-INI simulator and load the tune.
async fn connect_and_load(tx: &OwnerHandle) {
    send(tx, |reply| Command::Connect {
        source: ConnectSource::Simulator { ini_path: None },
        reply,
    })
    .await
    .expect("simulator connects");
    send(tx, |reply| Command::LoadTune { reply })
        .await
        .expect("tune loads");
}

/// The live simulator behind the owner's session (test-only escape hatch).
async fn simulator(tx: &OwnerHandle) -> Arc<opentune_simulator::EcuSimulator> {
    send(tx, |reply| Command::DebugSimulator { reply })
        .await
        .expect("simulator connection present")
}

/// The `reqFuel` value currently reported by the owner's tune.
async fn req_fuel(tx: &OwnerHandle) -> Value {
    let values = send(tx, |reply| Command::GetValues {
        names: vec!["reqFuel".into()],
        reply,
    })
    .await
    .expect("get_values");
    values.into_iter().next().expect("one value")
}

/// The dirty flags of every `TuneDirty` event in `events[since..]`.
fn dirty_events_since(events: &Collected, since: usize) -> Vec<bool> {
    events.lock().unwrap()[since..]
        .iter()
        .filter_map(|ev| match ev {
            OwnerEvent::TuneDirty(e) => Some(e.dirty),
            OwnerEvent::Connection(_) => None,
        })
        .collect()
}

// ── 1.1 the owner serves commands sequentially over the channel ─────────────

#[tokio::test]
async fn owner_serves_commands_sequentially() {
    let (tx, events) = test_owner();

    send(&tx, |reply| Command::Connect {
        source: ConnectSource::Simulator { ini_path: None },
        reply,
    })
    .await
    .expect("connect must succeed");

    let loaded = send(&tx, |reply| Command::LoadTune { reply })
        .await
        .expect("load_tune must succeed");
    assert!(!loaded.dirty, "freshly loaded tune is clean");

    let set = send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(12.5),
        reply,
    })
    .await
    .expect("set_value must succeed");
    assert!(set.dirty, "set_value must mark the tune dirty");
    assert_eq!(set.dirty_pages, vec![1]);

    // The owner emitted the connection lifecycle + the dirty transitions.
    let events = events.lock().unwrap();
    assert!(
        events.iter().any(|ev| matches!(
            ev,
            OwnerEvent::Connection(ConnectionStateEvent::Connected { .. })
        )),
        "owner must emit Connected"
    );
    assert!(
        events
            .iter()
            .any(|ev| matches!(ev, OwnerEvent::TuneDirty(e) if e.dirty)),
        "owner must emit the dirty event after set_value"
    );
}

// ── 1.3 reboot-detected reconnect invalidates + re-reads the tune ────────────
//
// secl choreography (reboot detection compares against `last_secl`, seeded at
// connect and refreshed on every successful reconnect — see M1's
// `secl_reboot_triggers_reidentify`): the owner builds the simulator inside
// `Connect`, so secl is 0 at connect time. A first *glitch* drop after
// `advance_secl(50)` re-seeds `last_secl = 50`; the reboot (`reset_secl` → 0)
// then reads 0 < 50 on the second reconnect → reboot detected.

#[tokio::test]
async fn reboot_on_reconnect_invalidates_and_rereads_tune() {
    let (tx, events) = test_owner();
    connect_and_load(&tx).await;

    // Burn 12.5 to flash, then leave an unburned 15.0 on top: a stale tune
    // would read 15.0/dirty, a re-read reads the flash 12.5/clean.
    send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(12.5),
        reply,
    })
    .await
    .expect("set 12.5");
    send(&tx, |reply| Command::Burn { reply })
        .await
        .expect("burn 12.5 to flash");
    send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(15.0),
        reply,
    })
    .await
    .expect("unburned 15.0 on top");

    let sim = simulator(&tx).await;

    // Simulated uptime, then a glitch drop: seeds last_secl = 50.
    sim.advance_secl(50);
    send(&tx, |reply| Command::SimulateLinkDrop { reply })
        .await
        .expect("glitch reconnect");

    // ECU reboots: RAM restores from flash (12.5), secl resets to 0.
    sim.reboot();
    sim.reset_secl();
    let before_drop = events.lock().unwrap().len();
    send(&tx, |reply| Command::SimulateLinkDrop { reply })
        .await
        .expect("reboot reconnect");

    // The owner must have re-read the tune: the burned flash value is back
    // and the unburned edit is gone (a stale tune would still say 15.0).
    assert_eq!(
        req_fuel(&tx).await,
        Value::Scalar(12.5),
        "after a reboot-detected reconnect the tune must be re-read from the ECU"
    );
    assert_eq!(
        dirty_events_since(&events, before_drop),
        vec![false],
        "the re-read must emit exactly one clean dirty event"
    );
}

// ── 1.4 glitch reconnect preserves the unburned tune (the safety twin) ──────
//
// Follow-up (c) is *reboot-detected* re-read, NOT re-read-on-every-reconnect:
// on a cable glitch (secl continuous) a re-read would silently discard the
// user's unburned edits and regress M1's silent recovery. This test guards
// against that over-implementation of 1.5.

#[tokio::test]
async fn glitch_on_reconnect_preserves_unburned_tune() {
    let (tx, events) = test_owner();
    connect_and_load(&tx).await;

    // An unburned edit — the state a re-read would destroy.
    let set = send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(12.5),
        reply,
    })
    .await
    .expect("unburned edit");
    assert!(set.dirty);

    let sim = simulator(&tx).await;
    // Cable glitch: the ECU kept running, secl only advanced.
    sim.advance_secl(10);
    let before_drop = events.lock().unwrap().len();
    send(&tx, |reply| Command::SimulateLinkDrop { reply })
        .await
        .expect("glitch reconnect");

    assert_eq!(
        req_fuel(&tx).await,
        Value::Scalar(12.5),
        "a glitch reconnect must preserve the in-memory tune intact"
    );
    assert_eq!(
        dirty_events_since(&events, before_drop),
        Vec::<bool>::new(),
        "no dirty event on a glitch reconnect — the unburned edit survives (dirty stays true)"
    );
}

#[tokio::test]
async fn commands_error_when_not_connected() {
    let (tx, _events) = test_owner();
    let err = send(&tx, |reply| Command::LoadTune { reply })
        .await
        .expect_err("no session yet");
    assert!(err.contains("not connected"), "got: {err}");
}

#[tokio::test]
async fn realtime_flag_commands_reply_ok() {
    let (tx, _events) = test_owner();
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start_realtime is a flag set for now");
    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("stop_realtime is a flag clear for now");
}
