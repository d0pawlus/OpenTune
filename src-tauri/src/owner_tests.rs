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

#[tokio::test]
async fn realtime_flag_commands_reply_ok() {
    let (tx, _events) = test_owner();
    // Explicit start/stop stay valid without a session: the flag arms the
    // poll tick, which itself no-ops until a session exists.
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start_realtime replies Ok even without a session");
    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("stop_realtime replies Ok even without a session");
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
