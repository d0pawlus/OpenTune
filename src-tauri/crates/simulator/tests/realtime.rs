// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests: M3 Task 5 — the simulator's animated engine model and
//! its `'r'`/0x30 windowed output-channel responses, exercised through the
//! **real** `opentune_protocol::MsProtocol` (mirrors `tests/memory.rs`).
//!
//! See `crates/simulator/src/engine.rs` for the port-note / license record:
//! the animation state machine + correlations are **ported** from
//! [`askrejans/speeduino-serial-sim`](https://github.com/askrejans/speeduino-serial-sim)
//! (MIT), while the `'r'` wire dispatch and the INI-offset encoding are
//! written fresh against Speeduino `comms.cpp` per ADR-0006.
//!
//! The definition under test is the Task 2 `[OutputChannels]` fixture
//! (`crates/ini/tests/fixtures/speeduino-output-channels.ini`):
//! `ochBlockSize = 16`, with `secl` U08 @0, `engine` U08 @2 (bits `running`
//! [0:0]), `rpm` U16 @4, `coolantRaw` U08 @6, `tps` U08 @7 (0.5 %/bit).

use opentune_ini::{parse_definition, CommsSettings, Definition, EnvelopeFormat};
use opentune_protocol::{crc32_of, MsProtocol, Protocol};
use opentune_simulator::{EcuSimulator, SimEngine};
use opentune_transport::{Transport, TransportError};
use std::time::Duration;

fn fixture_definition() -> Definition {
    parse_definition(include_str!(
        "../../ini/tests/fixtures/speeduino-output-channels.ini"
    ))
    .expect("Task 2 output-channels fixture must parse")
}

/// Client-side comms: the parsed fixture's `[MegaTune]` `ochGetCommand` is
/// the bare M1 `"r"`; the full windowed template lives in `[OutputChannels]`
/// (what TunerStudio actually sends), so tests set it explicitly — the sim
/// never expands templates, it just answers the 7-byte wire form.
fn client_comms(def: &Definition, envelope: EnvelopeFormat) -> CommsSettings {
    CommsSettings {
        och_get_command: "r$tsCanId\\x30%2o%2c".to_owned(),
        envelope,
        ..def.comms.clone()
    }
}

/// One animation step = the ported firmware's 50 ms (20 Hz) update.
const STEP: Duration = Duration::from_millis(50);

// ── 5.2: sim_animates_correlated_channels ─────────────────────────────────

#[test]
fn sim_animates_correlated_channels() {
    let def = fixture_definition();
    let mut engine = SimEngine::new(&def);

    // Tick 30 × 50 ms (1.5 s of engine time) — enough to leave STARTUP
    // (exit needs > 1 s in-state and rpm above half of the 700 idle floor).
    let mut coolant_series = Vec::new();
    for _ in 0..30 {
        engine.tick(STEP);
        coolant_series.push(engine.och_block()[6]);
    }
    let block = engine.och_block();
    assert_eq!(block.len(), 16, "block must be sized to ochBlockSize");

    // (a) rpm decoded from its INI offset is in a plausible running range.
    let rpm = u16::from_le_bytes([block[4], block[5]]);
    assert!(
        (300..=7000).contains(&rpm),
        "rpm {rpm} outside plausible running range after leaving STARTUP"
    );

    // (b) coolant warms up monotonically (thermal-inertia correlation);
    // u8 quantization can plateau adjacent ticks, so assert non-decreasing
    // plus an overall strict rise from the 20 °C (raw 60) cold start.
    assert!(
        coolant_series.windows(2).all(|w| w[1] >= w[0]),
        "coolant must never fall during warm-up: {coolant_series:?}"
    );
    assert!(
        coolant_series.last().unwrap() > coolant_series.first().unwrap(),
        "coolant must rise overall during warm-up: {coolant_series:?}"
    );

    // (c) values land at their INI offsets. secl @0: 30 deterministic steps
    // at 20 steps/second → exactly 1. tps raw @7 is 0.5 %/bit → 0..=200.
    // engine byte @2: running bit (bit 0) set once out of STARTUP.
    assert_eq!(block[0], 1, "secl at byte 0 after 1.5 s of ticks");
    assert!(
        block[7] <= 200,
        "tps raw {} exceeds 100 % (0.5 %/bit)",
        block[7]
    );
    assert_eq!(block[2] & 0x01, 0x01, "running bit must be set");
}

// ── 5.4: sim_answers_r_command_windowed ────────────────────────────────────

#[test]
fn sim_answers_r_command_windowed() {
    let def = fixture_definition();
    for envelope in [EnvelopeFormat::Plain, EnvelopeFormat::MsEnvelope10] {
        let sim = EcuSimulator::from_definition(&def);
        sim.tick_engine(Duration::from_millis(500)); // animate 10 steps
        let mut proto = MsProtocol::new(client_comms(&def, envelope), sim.client_transport());

        // Full-block read returns exactly ochBlockSize bytes.
        let full = proto
            .read_output_channels(0, 16)
            .expect("full och read must succeed");
        assert_eq!(full.len(), 16, "({envelope:?}) full read length");

        // Windowed read returns exactly bytes 4-5 (rpm) of the same block.
        let window = proto
            .read_output_channels(4, 2)
            .expect("windowed och read must succeed");
        assert_eq!(
            window,
            &full[4..6],
            "({envelope:?}) window must match block"
        );

        // Past-the-end windows zero-pad instead of erroring/panicking.
        let tail = proto
            .read_output_channels(14, 4)
            .expect("tail read must succeed");
        assert_eq!(tail.len(), 4, "({envelope:?}) tail read length");
        assert_eq!(&tail[2..], &[0, 0], "({envelope:?}) zero-pad past end");
        let past = proto
            .read_output_channels(100, 3)
            .expect("fully out-of-range read must succeed");
        assert_eq!(past, vec![0, 0, 0], "({envelope:?}) whole window past end");
    }
}

// ── E2E proof: Task 4's real protocol sees a live, changing frame ──────────

#[test]
fn real_protocol_sees_animated_values_change_between_reads() {
    let def = fixture_definition();
    let sim = EcuSimulator::from_definition(&def);
    let mut proto = MsProtocol::new(
        client_comms(&def, EnvelopeFormat::MsEnvelope10),
        sim.client_transport(),
    );

    // 10 deterministic STARTUP steps: rpm ramps 25/step → 250.
    sim.tick_engine(Duration::from_millis(500));
    let first = proto.read_output_channels(0, 16).unwrap();
    // 10 more steps → 500. No wall-clock involved: only tick_engine moves time.
    sim.tick_engine(Duration::from_millis(500));
    let second = proto.read_output_channels(0, 16).unwrap();

    let rpm1 = u16::from_le_bytes([first[4], first[5]]);
    let rpm2 = u16::from_le_bytes([second[4], second[5]]);
    assert!(
        rpm2 > rpm1,
        "rpm must keep ramping during STARTUP: {rpm1} → {rpm2}"
    );
    assert!(
        second[6] >= first[6],
        "coolant must not fall while warming: {} → {}",
        first[6],
        second[6]
    );
}

// ── First-och secl reset (comms.cpp:361-365) ───────────────────────────────

#[test]
fn first_och_request_resets_secl_later_ones_do_not() {
    let def = fixture_definition();
    let sim = EcuSimulator::from_definition(&def);
    // Make both counters visibly non-zero first: 25 steps → engine secl = 1.
    sim.advance_secl(7);
    sim.tick_engine(Duration::from_millis(1_250));

    let mut och = MsProtocol::new(
        client_comms(&def, EnvelopeFormat::MsEnvelope10),
        sim.client_transport(),
    );
    // M1-style handle for the 'A' path (och_get_command's first byte).
    let mut a = MsProtocol::new(
        CommsSettings {
            och_get_command: "A".to_owned(),
            envelope: EnvelopeFormat::MsEnvelope10,
            ..def.comms.clone()
        },
        sim.client_transport(),
    );

    let first = och.read_output_channels(0, 16).unwrap();
    assert_eq!(first[0], 0, "first och request must reset the frame's secl");
    assert_eq!(a.read_secl().unwrap(), 0, "'A' counter must reset too");

    // Subsequent requests must NOT reset: advance both counters again.
    sim.advance_secl(3);
    sim.tick_engine(Duration::from_millis(1_000)); // steps 26..45: secl 0 → 1
    let second = och.read_output_channels(0, 16).unwrap();
    assert_eq!(second[0], 1, "later och requests must not reset secl");
    assert_eq!(a.read_secl().unwrap(), 3, "'A' counter keeps its own count");
}

// ── Graceful degradation: malformed 'r' must never panic the sim ───────────

/// msEnvelope_1.0 frame for an arbitrary payload (mirrors the sim's framing).
fn crc_frame(payload: &[u8]) -> Vec<u8> {
    let mut frame = (payload.len() as u16).to_be_bytes().to_vec();
    frame.extend_from_slice(payload);
    frame.extend_from_slice(&crc32_of(payload).to_be_bytes());
    frame
}

#[test]
fn malformed_r_requests_answer_gracefully_without_panicking() {
    let def = fixture_definition();
    let sim = EcuSimulator::from_definition(&def);
    let mut client = sim.client_transport();

    // Truncated CRC payload — a bare 'r' with no subcmd/offset/len.
    client.write(&crc_frame(b"r")).unwrap();
    let mut rsp = [0u8; 7]; // [len(2), status, crc(4)]
    client.read_exact(&mut rsp).unwrap();
    assert_eq!(&rsp[..3], &[0x00, 0x01, 0x00], "status-only reply");

    // Unknown sub-command (0x99 instead of 0x30) in a full 7-byte request.
    client
        .write(&crc_frame(&[b'r', 0x00, 0x99, 0x00, 0x00, 0x02, 0x00]))
        .unwrap();
    let mut rsp2 = [0u8; 7];
    client.read_exact(&mut rsp2).unwrap();
    assert_eq!(&rsp2[..3], &[0x00, 0x01, 0x00], "status-only reply");

    // Truncated *plain* 'r' (3 of 7 bytes): the sim waits for the rest —
    // no partial garbage, the read simply times out.
    let sim2 = EcuSimulator::from_definition(&def);
    let mut client2 = sim2.client_transport();
    client2.write(&[b'r', 0x00, 0x30]).unwrap();
    let mut byte = [0u8; 1];
    let err = client2.read_exact(&mut byte).unwrap_err();
    assert!(matches!(err, TransportError::Timeout(_)));
}

// ── M1-style sim (no definition): 'r' answers zero-fill, never panics ──────

#[test]
fn handshake_only_sim_answers_r_with_zero_fill() {
    let def = fixture_definition();
    let sim = EcuSimulator::new(); // no definition → no engine, empty block
    let mut proto = MsProtocol::new(
        client_comms(&def, EnvelopeFormat::Plain),
        sim.client_transport(),
    );
    let bytes = proto.read_output_channels(0, 8).unwrap();
    assert_eq!(bytes, vec![0; 8]);
}

// ── Auto-tick: production 'r' requests must advance wall-clock time ────────
//
// Bug report: after Connect (simulator) → "Start live", gauges jump once
// then FREEZE — nothing ever calls `tick_engine` in the real app (it's a
// test-only entry point), so every production `'r'` poll re-read the same
// stale `och_block`. The fix: the ECU wrapper (`Pipe`) auto-ticks the engine
// off the wall clock on every production `'r'` request, unless/until
// `tick_engine` is called explicitly (which takes over and disables it
// permanently, keeping every deterministic test above untouched).

#[test]
fn auto_tick_advances_engine_between_requests() {
    let def = fixture_definition();
    let sim = EcuSimulator::from_definition(&def);
    let mut proto = MsProtocol::new(
        client_comms(&def, EnvelopeFormat::Plain),
        sim.client_transport(),
    );

    let first = proto
        .read_output_channels(0, 16)
        .expect("first read must succeed");

    // Loop a few sleep+read rounds rather than asserting after a single
    // sleep: the rpm noise (±10) changes every 50 ms engine step, and after
    // ~1.5 s of wall-clock the STARTUP→WARMUP_IDLE transition slews rpm
    // hard, so *some* round within this budget is guaranteed to differ.
    // 15 × 120 ms ≈ 1.8 s total sleep, comfortably under the ~2 s budget.
    let mut changed = false;
    for _ in 0..15 {
        std::thread::sleep(Duration::from_millis(120));
        let frame = proto
            .read_output_channels(0, 16)
            .expect("subsequent read must succeed");
        if frame != first {
            changed = true;
            break;
        }
    }
    assert!(
        changed,
        "production 'r' requests must auto-tick the engine off the wall clock — \
         live gauges must not freeze after the first frame"
    );
}

#[test]
fn tick_engine_disables_auto_tick() {
    let def = fixture_definition();
    let sim = EcuSimulator::from_definition(&def);
    // Any explicit tick — even a zero-duration one — hands time-keeping over
    // to the caller and disables auto-tick permanently.
    sim.tick_engine(Duration::ZERO);
    let mut proto = MsProtocol::new(
        client_comms(&def, EnvelopeFormat::Plain),
        sim.client_transport(),
    );

    let first = proto
        .read_output_channels(0, 16)
        .expect("first read must succeed");
    std::thread::sleep(Duration::from_millis(120));
    let second = proto
        .read_output_channels(0, 16)
        .expect("second read must succeed");

    assert_eq!(
        first, second,
        "once tick_engine is called explicitly, auto-tick must stay disabled — \
         frames must not drift on the wall clock"
    );
}
