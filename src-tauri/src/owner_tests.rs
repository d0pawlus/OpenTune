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

async fn owner_state(tx: &OwnerHandle) -> DebugOwnerState {
    send(tx, |reply| Command::DebugState { reply })
        .await
        .expect("owner state")
}

/// Disable the idle-link health prober for tests that stage a reboot (or a
/// backwards-secl window) and then drive the recovery EXPLICITLY through
/// `SimulateLinkDrop`: a 1 s health tick landing inside that window would
/// otherwise read the staged secl first and start its own recovery, racing
/// the test's drop command. The health path has its own dedicated tests.
async fn suspend_health_checks(tx: &OwnerHandle) {
    send(tx, |reply| Command::DebugSuspendHealthChecks { reply })
        .await
        .expect("suspend health checks");
}

/// Deterministic wait (5 s cap) until the owner's debug state satisfies
/// `pred` — the oracle for an in-flight recovery settling.
async fn await_owner_state(
    tx: &OwnerHandle,
    pred: impl Fn(&DebugOwnerState) -> bool,
) -> DebugOwnerState {
    for _ in 0..500 {
        let state = owner_state(tx).await;
        if pred(&state) {
            return state;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("owner state never satisfied the predicate within 5 s");
}

/// Deterministic wait (5 s cap) for the first connection event past `since`
/// satisfying `pred`; returns its absolute index into the collected events.
async fn await_connection_event(
    events: &Collected,
    since: usize,
    pred: impl Fn(&ConnectionStateEvent) -> bool,
) -> usize {
    for _ in 0..500 {
        let found = events.lock().unwrap()[since..]
            .iter()
            .position(|ev| matches!(ev, OwnerEvent::Connection(e) if pred(e)));
        if let Some(i) = found {
            return since + i;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("no matching connection event within 5 s");
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

/// True when a `ConnectionStateEvent::Disconnected` was emitted in
/// `events[since..]` — the oracle for the FIX 2a/2b corrective emits.
fn emitted_disconnected_since(events: &Collected, since: usize) -> bool {
    events.lock().unwrap()[since..].iter().any(|ev| {
        matches!(
            ev,
            OwnerEvent::Connection(ConnectionStateEvent::Disconnected)
        )
    })
}

/// The dirty flags of every `TuneDirty` event in `events[since..]`.
fn dirty_events_since(events: &Collected, since: usize) -> Vec<bool> {
    events.lock().unwrap()[since..]
        .iter()
        .filter_map(|ev| match ev {
            OwnerEvent::TuneDirty(e) => Some(e.dirty),
            OwnerEvent::Connection(_) | OwnerEvent::Realtime(_) => None,
        })
        .collect()
}

/// Path to the realtime owner-test fixture INI (real-speeduino-shaped
/// `[OutputChannels]`: windowed `ochGetCommand` + `ochBlockSize = 16`).
const REALTIME_INI: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/realtime-owner.ini"
);

/// Connect to the realtime fixture simulator and load the tune.
async fn connect_realtime_and_load(tx: &OwnerHandle) {
    send(tx, |reply| Command::Connect {
        source: ConnectSource::Simulator {
            ini_path: Some(REALTIME_INI.to_owned()),
        },
        reply,
    })
    .await
    .expect("realtime fixture simulator connects");
    send(tx, |reply| Command::LoadTune { reply })
        .await
        .expect("tune loads");
}

/// Deterministic wait: block (with a generous 5 s cap) until at least one
/// `Realtime` event has been collected past `since`, then return it.
async fn await_frame_since(events: &Collected, since: usize) -> crate::events::RealtimeFrameEvent {
    await_frame_where(events, since, |_| true).await
}

/// Deterministic wait (same 5 s cap): the first collected `Realtime` event
/// past `since` that satisfies `pred`. Predicate-based so a test can wait
/// out in-flight polls that read the block *before* a `tick_engine` call —
/// matching on the post-tick value instead of racing the poll pipeline.
async fn await_frame_where(
    events: &Collected,
    since: usize,
    pred: impl Fn(&crate::events::RealtimeFrameEvent) -> bool,
) -> crate::events::RealtimeFrameEvent {
    for _ in 0..500 {
        if let Some(frame) = events.lock().unwrap()[since..]
            .iter()
            .find_map(|ev| match ev {
                OwnerEvent::Realtime(e) if pred(e) => Some(e.clone()),
                _ => None,
            })
        {
            return frame;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("no matching RealtimeFrameEvent within 5 s");
}

/// The value of channel `name` in a frame, if the frame carries it.
fn channel_value(frame: &crate::events::RealtimeFrameEvent, name: &str) -> Option<f64> {
    frame
        .channels
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| *v)
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
// `secl_reboot_triggers_reidentify`): the Task 8 bundled INI is *windowed*
// (`ochGetCommand` lifted from `[OutputChannels]`), so `read_secl` reads the
// engine's och byte 0 — connect consumes the sim's first-och zeroing
// (baseline 0), and uptime comes from `tick_engine`, not the legacy 'A'-path
// counter. A first *glitch* drop after 50 s of engine time re-seeds
// `last_secl = 50`; the reboot re-arms the first-och reset, so the second
// reconnect reads 0 < 50 → reboot detected.

#[tokio::test]
async fn reboot_on_reconnect_invalidates_and_rereads_tune() {
    let (tx, events) = test_owner();
    connect_and_load(&tx).await;
    suspend_health_checks(&tx).await;

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
    send(&tx, |reply| Command::SnapshotTune { reply })
        .await
        .expect("snapshot pre-reboot tune");
    assert!(owner_state(&tx).await.snapshot_present);

    let sim = simulator(&tx).await;

    // 50 s of simulated uptime, then a glitch drop: seeds last_secl = 50.
    sim.tick_engine(std::time::Duration::from_secs(50));
    send(&tx, |reply| Command::SimulateLinkDrop { reply })
        .await
        .expect("glitch reconnect");

    // ECU reboots: RAM restores from flash (12.5) and the boot-scoped
    // first-och request re-arms, so the reconnect's read_secl answers 0.
    sim.reboot();
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
    assert!(
        !owner_state(&tx).await.snapshot_present,
        "a reboot must always invalidate the pre-reboot snapshot"
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
    // Cable glitch: the ECU kept running, secl only advanced (10 s of
    // engine time — the windowed read_secl reads the engine's och byte 0).
    sim.tick_engine(std::time::Duration::from_secs(10));
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

// M4 final-review fix wave item 4: StartCapture with realtime polling
// stopped used to succeed and silently arm a capture ring that `poll_tick`
// never feeds (nothing sets `capturing` false or errors — the ring just
// never fills). Reject instead, with a clear error.
#[tokio::test]
async fn start_capture_requires_realtime_polling() {
    let (tx, _events) = test_owner();
    connect_and_load(&tx).await;

    let err = send(&tx, |reply| Command::StartCapture { reply })
        .await
        .expect_err("polling was never started");
    assert!(err.contains("polling"), "got: {err}");

    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start realtime");
    send(&tx, |reply| Command::StartCapture { reply })
        .await
        .expect("start capture once polling is running");

    send(&tx, |reply| Command::StopCapture { reply })
        .await
        .expect("stop capture");
    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("stop realtime");
}

// ── Task 4 regression: a FAILED attach must NOT destroy the offline tune ─────
//
// `Owner::connect`'s ATTACH branch takes the session out to move it onto the
// blocking pool. If `attach_connection` errors — the signature guard rejecting
// a mismatched ECU, or `connect_serial` failing on a bad port — the session
// must be handed back intact, never dropped. The earlier `?`-in-closure form
// dropped the owned `session` local on the error path, so `self.session`
// stayed `None`: the user's unsaved offline tune was destroyed exactly when
// the guard rejected a bad ECU. This drives the real command path (NOT
// `attach_connection` directly, which the owner_ops tests do and thus bypass
// this bug): NewTune builds the offline session, Connect with a Serial source
// pointing at a nonexistent port makes `connect_serial` error, and the tune
// must survive.

#[tokio::test]
async fn failed_attach_preserves_the_offline_tune() {
    const INI: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/speeduino.sample.ini"
    );
    let (tx, events) = test_owner();

    // Build the offline session (conn: None, tune: Some) via the real command.
    send(&tx, |reply| Command::NewTune {
        ini_path: INI.to_owned(),
        reply,
    })
    .await
    .expect("new_tune builds an offline session");

    // Attach to a bogus serial port: `connect_serial` opens the port on a
    // single attempt and errors immediately (ENOENT, no retry/hang), so the
    // ATTACH branch's `attach_connection` returns Err.
    let mark = events.lock().unwrap().len();
    let err = send(&tx, |reply| Command::Connect {
        source: ConnectSource::Serial {
            port_name: "/dev/opentune-nonexistent-bogus-port".to_owned(),
            ini_path: INI.to_owned(),
        },
        reply,
    })
    .await
    .expect_err("attach to a nonexistent serial port must fail");
    assert!(!err.is_empty(), "the failed attach reports an error");

    // FIX 2b: the refused attach must emit a corrective `Disconnected`
    // (`connect`'s `if r.is_err()` path) so the UI does not stay stuck on the
    // `Connected`/`Connecting` the attach already emitted. Removing that emit
    // makes this assertion fail.
    assert!(
        emitted_disconnected_since(&events, mark),
        "a refused attach must emit ConnectionStateEvent::Disconnected"
    );

    // The offline session SURVIVED: `GetDefinition` still succeeds, proving
    // `self.session` is still `Some`. Against the buggy code this returned
    // "not connected" because the failed attach dropped the session.
    let dto = send(&tx, |reply| Command::GetDefinition { reply })
        .await
        .expect("the offline tune must survive a failed attach");
    assert!(
        !dto.gauges.is_empty(),
        "the surviving session still exposes the loaded definition"
    );
}

// ── Task 4 (final review): Disconnect must NOT destroy an offline tune ───────
//
// Design spec §"Disconnect while editing": an offline-origin tune (created
// blank or opened from a `.msq`, then ATTACHed to a live link) must survive a
// disconnect — the link drops but the tune stays editable and saveable in
// offline mode. The old `Command::Disconnect` set `self.session = None`
// unconditionally, so an offline tune with unsaved edits was destroyed and
// every later edit/save failed "not connected". This drives the real command
// path: NewTune → Connect(ATTACH) → Disconnect → SetValue/SaveTune must still
// succeed → a later Connect re-ATTACHes with the edit intact.

#[tokio::test]
async fn disconnect_preserves_an_offline_origin_tune() {
    const INI: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/speeduino.sample.ini"
    );
    let (tx, _events) = test_owner();

    // Offline session, then ATTACH to the simulator (conn added, tune kept).
    send(&tx, |reply| Command::NewTune {
        ini_path: INI.to_owned(),
        reply,
    })
    .await
    .expect("new_tune builds an offline session");
    send(&tx, |reply| Command::Connect {
        source: ConnectSource::Simulator { ini_path: None },
        reply,
    })
    .await
    .expect("attach to the simulator");

    // Disconnect: the offline-origin tune must SURVIVE (link dropped, kept).
    send(&tx, |reply| Command::Disconnect { reply })
        .await
        .expect("disconnect");

    // Still editable offline — against the buggy code the session was gone and
    // this failed "not connected".
    send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(12.5),
        reply,
    })
    .await
    .expect("set_value must still succeed after disconnect (session survived)");

    // Still saveable. Best-effort cleanup runs before the assert so a failing
    // save never leaks the scratch file.
    let msq_path = std::env::temp_dir().join(format!(
        "opentune-disconnect-survive-{}-{:?}.msq",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&msq_path);
    let save = send(&tx, |reply| Command::SaveTune {
        path: msq_path.to_string_lossy().into_owned(),
        reply,
    })
    .await;
    let _ = std::fs::remove_file(&msq_path);
    save.expect("save_tune must still succeed after disconnect (session survived)");

    // A later Connect re-ATTACHes (offline session still present → ATTACH
    // branch), and the offline edit is intact across the whole cycle.
    send(&tx, |reply| Command::Connect {
        source: ConnectSource::Simulator { ini_path: None },
        reply,
    })
    .await
    .expect("re-attach to the simulator");
    assert_eq!(
        req_fuel(&tx).await,
        Value::Scalar(12.5),
        "the offline edit survives disconnect + re-attach"
    );
}

// The twin invariant that `offline_origin` exists to protect: an *online*
// (FRESH-read) tune must STILL be destroyed on disconnect, so a later connect
// FRESH-reads the ECU rather than ATTACHing a stale online tune. Without this
// test, weakening the Disconnect guard to `s.tune.is_some()` would leave every
// other test green while reintroducing a stale-ATTACH data-loss bug.

#[tokio::test]
async fn disconnect_destroys_an_online_tune() {
    let (tx, _events) = test_owner();
    // Online session, tune FRESH-read from the simulator (offline_origin=false).
    connect_and_load(&tx).await;
    send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(12.5),
        reply,
    })
    .await
    .expect("edit the online tune");

    // Disconnect must DESTROY an online session.
    send(&tx, |reply| Command::Disconnect { reply })
        .await
        .expect("disconnect");
    let err = send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(13.0),
        reply,
    })
    .await
    .expect_err("an online tune must NOT survive disconnect");
    assert!(err.contains("not connected"), "got: {err}");

    // A later connect takes the FRESH branch (session was destroyed, so it is
    // not the ATTACH case): the new session reads the tune straight from the
    // ECU rather than re-using a stale one.
    send(&tx, |reply| Command::Connect {
        source: ConnectSource::Simulator { ini_path: None },
        reply,
    })
    .await
    .expect("fresh reconnect");
    send(&tx, |reply| Command::LoadTune { reply })
        .await
        .expect("FRESH re-read after reconnect");
}

// FIX 2a: replacing a *connected* session with a new offline tune
// (`new_tune`/`open_tune` → `reset_session`) must emit a corrective
// `Disconnected` so the UI does not keep showing a false "Connected" after the
// live link is torn down. Removing that emit makes this assertion fail.

#[tokio::test]
async fn new_tune_while_connected_emits_disconnected() {
    const INI: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/speeduino.sample.ini"
    );
    let (tx, events) = test_owner();

    // Establish a live link first: `self.session` now has `conn: Some`.
    connect_and_load(&tx).await;

    // Create a fresh offline tune while connected — `reset_session` tears down
    // the live session (`had_link == true`) and must emit `Disconnected`.
    let mark = events.lock().unwrap().len();
    send(&tx, |reply| Command::NewTune {
        ini_path: INI.to_owned(),
        reply,
    })
    .await
    .expect("new_tune replaces the connected session with an offline one");

    assert!(
        emitted_disconnected_since(&events, mark),
        "creating an offline tune while connected must emit \
         ConnectionStateEvent::Disconnected"
    );
}

#[tokio::test]
async fn start_realtime_errors_without_a_session() {
    let (tx, _events) = test_owner();
    let err = send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect_err("start_realtime must not silently arm without a session");
    assert!(
        err.contains("not connected") && err.contains("realtime"),
        "clear realtime/session diagnostic, got: {err}"
    );
    let state = owner_state(&tx).await;
    assert!(!state.polling);
    assert!(!state.poller_present);

    // Stop remains idempotent so UI cleanup is always safe.
    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("stop_realtime remains idempotent");
}

#[tokio::test]
async fn panicked_session_operation_disconnects_and_disarms_realtime() {
    let (tx, events) = test_owner();
    connect_realtime_and_load(&tx).await;
    let mark = events.lock().unwrap().len();
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start");
    let _ = await_frame_since(&events, mark).await;

    let before_panic = events.lock().unwrap().len();
    let err = send(&tx, |reply| Command::DebugPanicSessionOperation { reply })
        .await
        .expect_err("forced panic must reach the caller as an error");
    assert!(err.contains("panicked"), "got: {err}");

    let state = owner_state(&tx).await;
    assert!(!state.session_present, "panicked session must be discarded");
    assert!(!state.polling, "polling must be disarmed");
    assert!(!state.poller_present, "poller state must be cleared");
    assert!(
        events.lock().unwrap()[before_panic..].iter().any(|event| {
            matches!(
                event,
                OwnerEvent::Connection(ConnectionStateEvent::Disconnected)
            )
        }),
        "panic must emit Disconnected"
    );

    let after_panic = events.lock().unwrap().len();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(
        !events.lock().unwrap()[after_panic..]
            .iter()
            .any(|event| matches!(event, OwnerEvent::Realtime(_))),
        "no poll frames may survive panic cleanup"
    );
}

#[tokio::test]
async fn failed_reboot_reread_invalidates_tune_and_snapshot_but_keeps_link() {
    let (tx, events) = test_owner();
    connect_and_load(&tx).await;
    suspend_health_checks(&tx).await;

    send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(12.5),
        reply,
    })
    .await
    .expect("set burned value");
    send(&tx, |reply| Command::Burn { reply })
        .await
        .expect("burn");
    send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(15.0),
        reply,
    })
    .await
    .expect("set stale unburned value");
    send(&tx, |reply| Command::SnapshotTune { reply })
        .await
        .expect("snapshot");

    let sim = simulator(&tx).await;
    sim.tick_engine(std::time::Duration::from_secs(50));
    send(&tx, |reply| Command::SimulateLinkDrop { reply })
        .await
        .expect("seed reconnect baseline");

    send(&tx, |reply| Command::DebugFailNextRebootTuneRead { reply })
        .await
        .expect("arm reread failure");
    sim.reboot();
    let before_drop = events.lock().unwrap().len();
    let err = send(&tx, |reply| Command::SimulateLinkDrop { reply })
        .await
        .expect_err("forced reboot tune reread must fail");
    assert!(err.contains("tune re-read after ECU reboot failed"));

    let state = owner_state(&tx).await;
    assert!(state.session_present, "reconnected live link must survive");
    assert!(
        !state.tune_loaded,
        "stale pre-reboot tune must be invalidated"
    );
    assert!(
        !state.snapshot_present,
        "stale pre-reboot snapshot must be invalidated"
    );
    let values_err = send(&tx, |reply| Command::GetValues {
        names: vec!["reqFuel".into()],
        reply,
    })
    .await
    .expect_err("invalidated tune must not remain readable");
    assert!(values_err.contains("no tune loaded"), "got: {values_err}");
    let _ = simulator(&tx).await;
    assert!(
        events.lock().unwrap()[before_drop..].iter().any(|event| {
            matches!(
                event,
                OwnerEvent::Connection(ConnectionStateEvent::Connected { .. })
            )
        }),
        "the reconnect itself must remain live"
    );

    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("live link still permits realtime");
    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("stop");
}

#[tokio::test]
async fn polling_and_commands_are_serialized_by_the_owner() {
    let (tx, events) = test_owner();
    connect_realtime_and_load(&tx).await;
    let mark = events.lock().unwrap().len();
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start");
    let _ = await_frame_since(&events, mark).await;

    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let held_owner = tx.clone();
    let held = tokio::spawn(async move {
        send(&held_owner, |reply| Command::DebugHoldSessionOperation {
            started: started_tx,
            release: release_rx,
            reply,
        })
        .await
    });
    started_rx.await.expect("held command started");

    let while_held = events.lock().unwrap().len();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(
        !events.lock().unwrap()[while_held..]
            .iter()
            .any(|event| matches!(event, OwnerEvent::Realtime(_))),
        "polling must not interleave with an in-flight command"
    );

    release_tx.send(()).expect("release held command");
    held.await
        .expect("held request task")
        .expect("held command completes");
    let resumed_mark = events.lock().unwrap().len();
    let _ = await_frame_since(&events, resumed_mark).await;
    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("stop");
}

#[tokio::test]
async fn request_reports_owner_gone_and_owner_queue_applies_backpressure() {
    let (dead_tx, dead_rx) = mpsc::channel(1);
    drop(dead_rx);
    let err = request(&dead_tx, |reply| Command::StopRealtime { reply })
        .await
        .expect_err("closed owner channel");
    assert_eq!(err, OWNER_GONE);

    let (tx, _events) = test_owner();
    connect_and_load(&tx).await;
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let held_owner = tx.clone();
    let held = tokio::spawn(async move {
        send(&held_owner, |reply| Command::DebugHoldSessionOperation {
            started: started_tx,
            release: release_rx,
            reply,
        })
        .await
    });
    started_rx.await.expect("held command started");

    for _ in 0..32 {
        let (reply, _rx) = oneshot::channel();
        tx.try_send(Command::StopRealtime { reply })
            .expect("bounded owner queue has advertised capacity");
    }
    let (reply, _rx) = oneshot::channel();
    assert!(
        matches!(
            tx.try_send(Command::StopRealtime { reply }),
            Err(mpsc::error::TrySendError::Full(_))
        ),
        "the 33rd queued command must see explicit backpressure"
    );

    release_tx.send(()).expect("release held command");
    held.await
        .expect("held request task")
        .expect("held command completes");
}

// ── 6.5 the poll tick: decode → coalesce → emit ─────────────────────────────

#[tokio::test]
async fn realtime_polls_decode_and_emit_frames() {
    let (tx, events) = test_owner();
    connect_realtime_and_load(&tx).await;
    let sim = simulator(&tx).await;
    // Animate the engine out of STARTUP so channels carry live values.
    sim.tick_engine(std::time::Duration::from_millis(1_500));

    let mark = events.lock().unwrap().len();
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start");
    let frame = await_frame_since(&events, mark).await;

    let names: Vec<&str> = frame.channels.iter().map(|(n, _)| n.as_str()).collect();
    for expected in [
        "secl",
        "rpm",
        "coolantRaw",
        "tps",
        "coolant",
        "throttle",
        "running",
    ] {
        assert!(
            names.contains(&expected),
            "frame must carry `{expected}`, got {names:?}"
        );
    }
    let get = |name: &str| {
        frame
            .channels
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v)
            .unwrap()
    };
    assert!(get("rpm") > 0.0, "animated rpm must be live");
    assert_eq!(
        get("coolant"),
        get("coolantRaw") - 40.0,
        "computed channel must evaluate over the decoded scalar"
    );

    // Stop is serialized after any in-flight tick on the same task, so once
    // its reply lands no further frame can ever be emitted.
    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("stop");
    let after_stop = events.lock().unwrap().len();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let frames_after = events.lock().unwrap()[after_stop..]
        .iter()
        .filter(|e| matches!(e, OwnerEvent::Realtime(_)))
        .count();
    assert_eq!(frames_after, 0, "no frames after stop_realtime");
}

// ── 6.5 blocker c: polling keeps the reboot baseline live ──────────────────
//
// The firmware zeroes secl on the FIRST och request (comms.cpp:361-365) and
// the u8 counter wraps every 256 s, so during polling secl legitimately
// moves backwards with no reboot. With the windowed read_secl, the
// first-request zeroing lands at *connect* (read_secl is itself an och
// request), so the choreography here seeds the >0 baseline via a glitch
// reconnect after engine uptime, and the backwards move is the u8 wrap —
// the same `new_secl < last_secl` shape as the brief's scenario. Without
// the owner feeding polled secl into `note_secl`, glitch #2 below would
// read 34 < 90 → false reboot → tune re-read → the unburned edit silently
// reverts (the data-loss class this task must close).

#[tokio::test]
async fn polling_glitch_reconnect_preserves_unburned_tune_after_secl_wrap() {
    let (tx, events) = test_owner();
    connect_realtime_and_load(&tx).await;
    suspend_health_checks(&tx).await;

    // The unburned edit a false-reboot re-read would destroy.
    send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(12.5),
        reply,
    })
    .await
    .expect("unburned edit");

    let sim = simulator(&tx).await;
    // 90 s of engine uptime, then a glitch reconnect: re-seeds the
    // manager's baseline to last_secl = 90 (> 0).
    sim.tick_engine(std::time::Duration::from_secs(90));
    send(&tx, |reply| Command::SimulateLinkDrop { reply })
        .await
        .expect("glitch #1 (baseline seed)");

    // 200 more seconds: secl wraps to (90 + 200) mod 256 = 34 — backwards,
    // no reboot. ≥1 successful poll must re-sync the baseline to 34.
    sim.tick_engine(std::time::Duration::from_secs(200));
    let mark = events.lock().unwrap().len();
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start");
    let _ = await_frame_since(&events, mark).await;
    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("stop");

    let before_drop = events.lock().unwrap().len();
    send(&tx, |reply| Command::SimulateLinkDrop { reply })
        .await
        .expect("glitch #2");

    assert_eq!(
        req_fuel(&tx).await,
        Value::Scalar(12.5),
        "a glitch reconnect after polling must NOT read as a reboot — \
         the unburned edit survives"
    );
    assert_eq!(
        dirty_events_since(&events, before_drop),
        Vec::<bool>::new(),
        "no tune re-read on the glitch: no dirty event at all"
    );
}

// ── 6.5 blocker c twin: a REAL reboot while polling is still detected ──────

#[tokio::test]
async fn real_reboot_after_polling_still_detected_and_rereads_tune() {
    let (tx, events) = test_owner();
    connect_realtime_and_load(&tx).await;
    suspend_health_checks(&tx).await;

    // Burn 12.5 to flash, then an unburned 15.0 on top: a re-read lands on
    // 12.5/clean, a stale tune would keep 15.0.
    send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(12.5),
        reply,
    })
    .await
    .expect("set 12.5");
    send(&tx, |reply| Command::Burn { reply })
        .await
        .expect("burn 12.5");
    send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(15.0),
        reply,
    })
    .await
    .expect("unburned 15.0");

    let sim = simulator(&tx).await;
    sim.tick_engine(std::time::Duration::from_secs(60)); // secl 60
    let mark = events.lock().unwrap().len();
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start");
    let _ = await_frame_since(&events, mark).await; // baseline noted: 60
    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("stop");

    // Real reboot: RAM restores from flash and the boot-scoped "first och
    // request" re-arms — the reconnect's windowed read_secl is that first
    // request and answers secl = 0 → 0 < 60 → reboot detected.
    sim.reboot();
    let before_drop = events.lock().unwrap().len();
    send(&tx, |reply| Command::SimulateLinkDrop { reply })
        .await
        .expect("reboot reconnect");

    assert_eq!(
        req_fuel(&tx).await,
        Value::Scalar(12.5),
        "a real reboot must still re-read the tune from the ECU"
    );
    assert_eq!(
        dirty_events_since(&events, before_drop),
        vec![false],
        "the re-read must emit exactly one clean dirty event"
    );
}

// ── 8.1 M3 demo: bundled INI drives live, changing gauges; stop halts them ──
//
// End-to-end over the DEFAULT connect path (simulator + bundled sample INI):
// connect → loadTune → getDefinition (non-empty gauges/frontpage with
// referential integrity — proves the Task 8 INI extension parses) →
// StartRealtime → a frontpage-bound channel's value CHANGES across frames as
// simulated time advances → StopRealtime → frames stop. The frontend half
// (slot rendering, rebinding, canvas animation) is Task 7's vitest coverage.

#[tokio::test]
async fn m3_demo_bundled_ini_live_gauges_animate_and_stop() {
    let (tx, events) = test_owner();
    connect_and_load(&tx).await;

    // The bundled definition now carries the dashboard (Task 8 extension).
    let dto = send(&tx, |reply| Command::GetDefinition { reply })
        .await
        .expect("definition");
    assert!(!dto.gauges.is_empty(), "bundled INI must define gauges");
    assert!(
        !dto.frontpage.gauge_slots.is_empty(),
        "bundled INI must fill front-page slots"
    );
    assert!(
        !dto.frontpage.indicators.is_empty(),
        "bundled INI must define an indicator"
    );
    for slot in &dto.frontpage.gauge_slots {
        assert!(
            dto.gauges.iter().any(|g| &g.name == slot),
            "front-page slot `{slot}` must reference a defined gauge"
        );
    }

    // Animate the engine out of STARTUP so channels carry live values.
    let sim = simulator(&tx).await;
    sim.tick_engine(std::time::Duration::from_millis(1_500));

    let mark = events.lock().unwrap().len();
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start");
    let first = await_frame_since(&events, mark).await;

    // Slot 1's bound channel (the tachometer's rpm) must be live, and must
    // CHANGE across frames once simulated time advances — the deterministic
    // engine slews rpm by ~200 over 2 s of WARMUP, far beyond its ±10 idle
    // noise, so waiting for a frame with a different value cannot hang.
    let bound = &dto
        .gauges
        .iter()
        .find(|g| Some(&g.name) == dto.frontpage.gauge_slots.first())
        .expect("slot 1 references a defined gauge")
        .channel;
    let before = channel_value(&first, bound)
        .unwrap_or_else(|| panic!("frame must carry the bound channel `{bound}`"));
    assert!(
        before > 0.0,
        "animated `{bound}` must be live, got {before}"
    );

    sim.tick_engine(std::time::Duration::from_secs(2));
    let mark = events.lock().unwrap().len();
    let changed = await_frame_where(&events, mark, |f| {
        channel_value(f, bound).is_some_and(|v| v != before)
    })
    .await;
    assert_ne!(
        channel_value(&changed, bound),
        Some(before),
        "the bound channel must change across frames"
    );

    // Stop is serialized after any in-flight tick on the same task, so once
    // its reply lands no further frame can ever be emitted.
    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("stop");
    let after_stop = events.lock().unwrap().len();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let frames_after = events.lock().unwrap()[after_stop..]
        .iter()
        .filter(|e| matches!(e, OwnerEvent::Realtime(_)))
        .count();
    assert_eq!(frames_after, 0, "no frames after stop_realtime");
}

// ── M1 acceptance: a real poll failure enters reconnect automatically ──────

#[tokio::test]
async fn realtime_transport_failure_automatically_reconnects_and_resumes() {
    let (tx, events) = test_owner();
    connect_realtime_and_load(&tx).await;
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start");
    let first_mark = events.lock().unwrap().len();
    let _ = await_frame_since(&events, first_mark).await;

    let sim = simulator(&tx).await;
    let before_drop = events.lock().unwrap().len();
    sim.set_link_dropped(true);
    let restore = Arc::clone(&sim);
    std::thread::spawn(move || {
        // Keep the link down long enough for normal polling and at least one
        // reconnect attempt to observe the failure.
        std::thread::sleep(std::time::Duration::from_millis(100));
        restore.set_link_dropped(false);
    });

    for _ in 0..500 {
        let recovered = {
            let events = events.lock().unwrap();
            let slice = &events[before_drop..];
            slice.iter().any(|event| {
                matches!(
                    event,
                    OwnerEvent::Connection(ConnectionStateEvent::Reconnecting { .. })
                )
            }) && slice.iter().any(|event| {
                matches!(
                    event,
                    OwnerEvent::Connection(ConnectionStateEvent::Connected { .. })
                )
            })
        };
        if recovered {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    {
        let events = events.lock().unwrap();
        let slice = &events[before_drop..];
        let reconnecting = slice
            .iter()
            .position(|event| {
                matches!(
                    event,
                    OwnerEvent::Connection(ConnectionStateEvent::Reconnecting { .. })
                )
            })
            .expect("poll failure must emit Reconnecting");
        let connected = slice
            .iter()
            .position(|event| {
                matches!(
                    event,
                    OwnerEvent::Connection(ConnectionStateEvent::Connected { .. })
                )
            })
            .expect("restored link must emit Connected");
        assert!(reconnecting < connected);
    }

    let after_reconnect = events.lock().unwrap().len();
    let _ = await_frame_since(&events, after_reconnect).await;
    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("stop");
}

// ── M1 rereview CRITICAL: recovery must not block the owner or retry forever ─
//
// `recover_link` used to run the WHOLE reconnect schedule (serial: ~150 s of
// backoff) inside one awaited spawn_blocking — no command, not even the
// user's Disconnect, was served meanwhile — and a terminal `Failed` left
// `session.conn` populated, so the next 1 s health tick started the next
// full cycle: an infinite retry storm. Recovery now runs fire-and-forget
// (`start_recovery` → `RecoverySettled`), streams every state live, and a
// terminal failure applies the Disconnect retention rule to whatever
// session settles back.

#[tokio::test]
async fn terminal_recovery_failure_drops_online_session_and_stops_retrying() {
    let (tx, events) = test_owner();
    connect_realtime_and_load(&tx).await;
    let mark = events.lock().unwrap().len();
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start");
    let _ = await_frame_since(&events, mark).await;

    // Permanent drop: every reconnect attempt fails and the retry budget
    // (10 attempts, ≤100 ms backoff each in the sim config) exhausts.
    let sim = simulator(&tx).await;
    let before_drop = events.lock().unwrap().len();
    sim.set_link_dropped(true);

    // The terminal Failed is streamed live by the recovery task.
    let failed_at = await_connection_event(&events, before_drop, |e| {
        matches!(e, ConnectionStateEvent::Failed { .. })
    })
    .await;

    // Once settled: the FRESH online session is dropped (retention rule —
    // nothing offline to keep) and polling is disarmed.
    let state = await_owner_state(&tx, |s| !s.recovering).await;
    assert!(
        !state.session_present,
        "a FRESH online session must be dropped after terminal recovery failure"
    );
    assert!(!state.polling, "polling must be disarmed");

    // THE storm regression: with no live conn left in the seat, the health
    // tick must NOT start another cycle. A full health interval (1 s) plus
    // margin passes with no further connection events of any kind.
    let quiet_mark = events.lock().unwrap().len();
    tokio::time::sleep(std::time::Duration::from_millis(1_300)).await;
    assert!(
        !events.lock().unwrap()[quiet_mark..]
            .iter()
            .any(|ev| matches!(ev, OwnerEvent::Connection(_))),
        "no reconnect cycle may start after a terminal recovery failure"
    );
    assert!(
        !events.lock().unwrap()[failed_at + 1..].iter().any(|ev| {
            matches!(
                ev,
                OwnerEvent::Connection(ConnectionStateEvent::Reconnecting { .. })
            )
        }),
        "no Reconnecting may follow the terminal Failed"
    );

    let err = send(&tx, |reply| Command::GetDefinition { reply })
        .await
        .expect_err("session is gone after terminal failure");
    assert!(err.contains(NOT_CONNECTED), "got: {err}");
}

#[tokio::test]
async fn terminal_recovery_failure_keeps_offline_origin_tune_editable() {
    const INI: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/speeduino.sample.ini"
    );
    let (tx, events) = test_owner();

    // Offline-origin session, ATTACHed to the simulator, with an edit.
    send(&tx, |reply| Command::NewTune {
        ini_path: INI.to_owned(),
        reply,
    })
    .await
    .expect("new_tune");
    send(&tx, |reply| Command::Connect {
        source: ConnectSource::Simulator { ini_path: None },
        reply,
    })
    .await
    .expect("attach");
    send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(12.5),
        reply,
    })
    .await
    .expect("edit");
    let mark = events.lock().unwrap().len();
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start");
    let _ = await_frame_since(&events, mark).await;

    let sim = simulator(&tx).await;
    let before_drop = events.lock().unwrap().len();
    sim.set_link_dropped(true);

    let _ = await_connection_event(&events, before_drop, |e| {
        matches!(e, ConnectionStateEvent::Failed { .. })
    })
    .await;
    let state = await_owner_state(&tx, |s| !s.recovering).await;
    assert!(
        state.session_present,
        "an offline-origin session must survive terminal failure as offline"
    );
    assert!(state.tune_loaded, "the offline tune must survive");
    assert!(!state.polling);

    // Still editable offline (the surviving session's link is gone), and
    // the pre-drop edit is intact — mirrors §"Disconnect while editing".
    assert_eq!(req_fuel(&tx).await, Value::Scalar(12.5));
    send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(13.0),
        reply,
    })
    .await
    .expect("offline edit after terminal recovery failure");
    assert_eq!(req_fuel(&tx).await, Value::Scalar(13.0));
}

#[tokio::test]
async fn disconnect_during_recovery_cancels_and_keeps_offline_tune() {
    const INI: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/speeduino.sample.ini"
    );
    let (tx, events) = test_owner();

    send(&tx, |reply| Command::NewTune {
        ini_path: INI.to_owned(),
        reply,
    })
    .await
    .expect("new_tune");
    send(&tx, |reply| Command::Connect {
        source: ConnectSource::Simulator { ini_path: None },
        reply,
    })
    .await
    .expect("attach");
    send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(12.5),
        reply,
    })
    .await
    .expect("edit");
    let mark = events.lock().unwrap().len();
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start");
    let _ = await_frame_since(&events, mark).await;

    let sim = simulator(&tx).await;
    let before_drop = events.lock().unwrap().len();
    sim.set_link_dropped(true);

    // Recovery is in flight once the first live Reconnecting arrives.
    let _ = await_connection_event(&events, before_drop, |e| {
        matches!(e, ConnectionStateEvent::Reconnecting { .. })
    })
    .await;

    // Disconnect answers promptly — it does NOT wait out the retry budget —
    // and cancels the in-flight recovery.
    send(&tx, |reply| Command::Disconnect { reply })
        .await
        .expect("disconnect during recovery");

    // The cancelled recovery settles; the offline-origin tune survives per
    // the Disconnect retention rule.
    let state = await_owner_state(&tx, |s| !s.recovering).await;
    assert!(
        state.session_present,
        "offline tune survives the disconnect"
    );
    assert!(state.tune_loaded);

    // Exactly one Disconnected (Disconnect's own emit), and NO Failed — a
    // stale terminal Failed landing after the user's Disconnect would
    // corrupt the UI state it just displayed.
    {
        let events = events.lock().unwrap();
        let disconnects = events[before_drop..]
            .iter()
            .filter(|ev| {
                matches!(
                    ev,
                    OwnerEvent::Connection(ConnectionStateEvent::Disconnected)
                )
            })
            .count();
        assert_eq!(disconnects, 1, "exactly one Disconnected");
        assert!(
            !events[before_drop..].iter().any(|ev| {
                matches!(
                    ev,
                    OwnerEvent::Connection(ConnectionStateEvent::Failed { .. })
                )
            }),
            "a cancelled recovery must not emit a terminal Failed"
        );
    }

    // Offline editing continues on the surviving session.
    send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(13.0),
        reply,
    })
    .await
    .expect("offline edit after cancelled recovery");
    assert_eq!(req_fuel(&tx).await, Value::Scalar(13.0));
}

#[tokio::test]
async fn recovery_streams_reconnecting_live_and_commands_fail_fast() {
    let (tx, events) = test_owner();
    connect_realtime_and_load(&tx).await;
    let mark = events.lock().unwrap().len();
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start");
    let _ = await_frame_since(&events, mark).await;

    let sim = simulator(&tx).await;
    let before_drop = events.lock().unwrap().len();
    sim.set_link_dropped(true);

    // Reconnecting{1} is observable WHILE the recovery still runs — under
    // the old batched emission nothing reached the UI until the terminal
    // state, a full backoff schedule later.
    let _ = await_connection_event(&events, before_drop, |e| {
        matches!(e, ConnectionStateEvent::Reconnecting { attempt: 1 })
    })
    .await;

    // A session command sent mid-recovery answers immediately with the
    // distinct diagnostic instead of queueing behind the retry schedule.
    let err = send(&tx, |reply| Command::GetDefinition { reply })
        .await
        .expect_err("session is checked out for recovery");
    assert!(err.contains(RECOVERY_IN_PROGRESS), "got: {err}");

    // Connect is refused as a safety net while recovery is in flight (a
    // stale session must never resurrect over a fresh one).
    let err = send(&tx, |reply| Command::Connect {
        source: ConnectSource::Simulator { ini_path: None },
        reply,
    })
    .await
    .expect_err("connect during recovery is refused");
    assert!(err.contains(RECOVERY_IN_PROGRESS), "got: {err}");

    // Let the recovery settle so teardown is clean.
    let _ = await_owner_state(&tx, |s| !s.recovering).await;
}

// The health-path twin of `failed_reboot_reread_invalidates_tune_and_snapshot_
// but_keeps_link`: a REAL (non-demo) recovery that reconnects to a rebooted
// ECU whose tune re-read fails must keep the live link (session restored,
// tune/snapshot invalidated) with polling stopped — the
// `ConnectedButTuneRereadFailed` settle arm.

#[tokio::test]
async fn recovery_reboot_reread_failure_keeps_live_link_and_stops_polling() {
    let (tx, events) = test_owner();
    connect_and_load(&tx).await;

    // Seed a >0 reboot baseline: 50 s of engine time, noted by ≥1 poll.
    let sim = simulator(&tx).await;
    sim.tick_engine(std::time::Duration::from_secs(50));
    let mark = events.lock().unwrap().len();
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start");
    let _ = await_frame_where(&events, mark, |f| {
        channel_value(f, "secl").is_some_and(|v| v >= 50.0)
    })
    .await;

    send(&tx, |reply| Command::DebugFailNextRebootTuneRead { reply })
        .await
        .expect("arm reread failure");

    // Drop FIRST (so no poll can consume the rebooted secl and destroy the
    // baseline), then reboot; restore the link once recovery is under way so
    // the reconnect succeeds and detects the reboot (read_secl 0 < 50).
    sim.set_link_dropped(true);
    sim.reboot();
    let restore = Arc::clone(&sim);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        restore.set_link_dropped(false);
    });

    let state = await_owner_state(&tx, |s| {
        !s.recovering && s.session_present && !s.tune_loaded
    })
    .await;
    assert!(!state.polling, "a tune-less session must not keep polling");
    assert!(!state.snapshot_present);

    // The live link survived: the simulator is still reachable through the
    // session's connection, but the stale pre-reboot tune is not readable.
    let _ = simulator(&tx).await;
    let err = send(&tx, |reply| Command::GetValues {
        names: vec!["reqFuel".into()],
        reply,
    })
    .await
    .expect_err("invalidated tune must not remain readable");
    assert!(err.contains("no tune loaded"), "got: {err}");
}

// ── 8.2 M3 demo: link-drop recovery — reconnect, reboot re-read, frames resume
//
// THE M3 demo: while realtime runs, the ECU reboots and the link drops. The
// owner must emit Reconnecting → Connected, re-read the tune (reboot ⇒ secl
// reset, Task 1.4 semantics), and realtime frames must RESUME after the
// reconnect without an app restart — polling stays armed through a drop
// (fail-open; stopping is only ever the user's explicit command).

#[tokio::test]
async fn m3_demo_link_drop_recovery_rereads_tune_and_resumes_frames() {
    let (tx, events) = test_owner();
    connect_and_load(&tx).await;

    // Burn 12.5 to flash, then an unburned 15.0 on top: the reboot re-read
    // lands on 12.5/clean; a stale tune would keep 15.0.
    send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(12.5),
        reply,
    })
    .await
    .expect("set 12.5");
    send(&tx, |reply| Command::Burn { reply })
        .await
        .expect("burn 12.5");
    send(&tx, |reply| Command::SetValue {
        name: "reqFuel".into(),
        value: Value::Scalar(15.0),
        reply,
    })
    .await
    .expect("unburned 15.0");

    // 60 s of uptime, then live polling; waiting for a frame whose secl
    // already reads ≥ 60 proves ≥ 1 poll fed `note_secl` *after* the tick —
    // the reboot baseline is re-synced past the connect-time zero.
    let sim = simulator(&tx).await;
    sim.tick_engine(std::time::Duration::from_secs(60));
    let mark = events.lock().unwrap().len();
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start");
    let _ = await_frame_where(&events, mark, |f| {
        channel_value(f, "secl").is_some_and(|v| v >= 60.0)
    })
    .await;

    // The ECU reboots and the link drops WHILE realtime is running.
    sim.reboot();
    let before_drop = events.lock().unwrap().len();
    send(&tx, |reply| Command::SimulateLinkDrop { reply })
        .await
        .expect("link drop recovers");

    // Reconnect lifecycle: Reconnecting, then Connected, in that order.
    {
        let events = events.lock().unwrap();
        let reconnecting = events[before_drop..].iter().position(|e| {
            matches!(
                e,
                OwnerEvent::Connection(ConnectionStateEvent::Reconnecting { .. })
            )
        });
        let connected = events[before_drop..].iter().position(|e| {
            matches!(
                e,
                OwnerEvent::Connection(ConnectionStateEvent::Connected { .. })
            )
        });
        let reconnecting = reconnecting.expect("drop must emit Reconnecting");
        let connected = connected.expect("drop must end Connected");
        assert!(
            reconnecting < connected,
            "Reconnecting must precede Connected"
        );
    }

    // Reboot detected → tune re-read: flash 12.5 back, one clean dirty event.
    assert_eq!(
        req_fuel(&tx).await,
        Value::Scalar(12.5),
        "the reboot-detected reconnect must re-read the tune"
    );
    assert_eq!(
        dirty_events_since(&events, before_drop),
        vec![false],
        "the re-read must emit exactly one clean dirty event"
    );

    // Frames RESUME without an app restart. The drop reply only lands after
    // the reconnect settled, so any frame collected past this point is
    // post-recovery — and it carries the rebooted, re-zeroed secl origin.
    let after_drop = events.lock().unwrap().len();
    let resumed = await_frame_since(&events, after_drop).await;
    let secl = channel_value(&resumed, "secl").expect("frame carries secl");
    assert!(
        secl < 60.0,
        "post-reboot frames restart from the zeroed secl, got {secl}"
    );

    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("stop");
}

// ── Task 8: the capture ring pins the poll-tick tap invariant ──────────────
//
// `capture.rs`'s doc comment records the rate invariant: the ring taps
// poll_tick's EMITTED frames, and at the owner's 25 Hz cadence (40 ms) —
// slower than the realtime poller's ~30 Hz (33 ms) coalescing gate — every
// acquired frame is emitted, so the capture sees (near) the full poll rate.
// This test verifies the observable behavior that invariant depends on:
// frames flow into the ring while capturing (waited out with a generous cap,
// not a fixed window, to stay CI-safe), and rows freeze — not decay — once
// `StopCapture` clears the flag.

/// Poll `CaptureStatus` (deterministic wait, ~2 s cap) until `sample_count`
/// reaches `min`.
async fn wait_for_sample_count(tx: &OwnerHandle, min: u32) -> CaptureStatusDto {
    for _ in 0..200 {
        let status = send(tx, |reply| Command::CaptureStatus { reply })
            .await
            .expect("capture_status");
        if status.sample_count >= min {
            return status;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("sample_count never reached {min} within 2 s");
}

/// The structural half of the tap-rate invariant (review I-1): the capture
/// tap sits on poll_tick's EMITTED frames, above the realtime coalescing
/// gate. That sees the full poll rate only while the owner polls no faster
/// than the gate emits. The behavioral test below can't catch a rate change
/// (frames still flow either way) — this assertion breaks loudly instead.
#[test]
fn poll_interval_never_outpaces_the_coalesce_gate() {
    assert!(
        POLL_INTERVAL >= opentune_realtime::DEFAULT_EMIT_INTERVAL,
        "capture taps emitted frames; if the owner ever polls faster than the \
         coalescing gate, frames get discarded before the tap and capture sees \
         less than full rate — move the tap below the gate (realtime poll.rs)"
    );
}

#[tokio::test]
async fn capture_rate_pins_the_tap_invariant() {
    let (tx, _events) = test_owner();
    connect_realtime_and_load(&tx).await;

    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start realtime");
    send(&tx, |reply| Command::StartCapture { reply })
        .await
        .expect("start capture");

    // ~10 poll ticks (400 ms) elapse; every acquired frame must be captured
    // (25 Hz < 30 Hz gate ⇒ no coalescing) — expect at least 8 rows.
    let status = wait_for_sample_count(&tx, 8).await;
    assert!(
        status.capturing,
        "capture_status must report capturing while the flag is set"
    );
    assert!(
        status.sample_count >= 8,
        "expected >= 8 captured rows after ~10 poll ticks, got {}",
        status.sample_count
    );

    let stopped = send(&tx, |reply| Command::StopCapture { reply })
        .await
        .expect("stop_capture");
    assert!(!stopped.capturing, "stop_capture clears the flag");
    let frozen_count = stopped.sample_count;
    let frozen_duration = stopped.duration_ms;

    // A further tick window must not grow the ring: the flag is off, rows
    // are retained (not cleared) for a later `run_ve_analyze`.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let after = send(&tx, |reply| Command::CaptureStatus { reply })
        .await
        .expect("capture_status");
    assert_eq!(
        after.sample_count, frozen_count,
        "sample_count must not grow after stop_capture"
    );
    assert_eq!(
        after.duration_ms, frozen_duration,
        "duration_ms must freeze too — no new rows means no later t_ms"
    );
    assert!(!after.capturing, "capturing stays false after stop_capture");

    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("stop realtime");
}
