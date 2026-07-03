// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for the real `decode_frame` (M3 Task 6, sub-steps
//! 6.1–6.3): raw och block → scaled physical channel values, computed
//! expressions evaluated over already-decoded siblings, fail-open per
//! channel.
//!
//! The definition under test is the Task 2 `[OutputChannels]` fixture
//! (`crates/ini/tests/fixtures/speeduino-output-channels.ini`):
//! `ochBlockSize = 16`, `secl` U08@0, `engine` U08@2 (bit `running` [0:0]),
//! `rpm` U16@4, `coolantRaw` U08@6, `tps` U08@7 (scale 0.5), plus computed
//! `coolant = { coolantRaw - 40 }` and `throttle = { tps }`.

use opentune_ini::{parse_definition, Definition, OutputChannelDef};
use opentune_realtime::decode_frame;

fn fixture_definition() -> Definition {
    parse_definition(include_str!(
        "../../ini/tests/fixtures/speeduino-output-channels.ini"
    ))
    .expect("Task 2 output-channels fixture must parse")
}

/// A 16-byte block with rpm=3000 (U16 LE @4), coolantRaw=80 (U08 @6),
/// tps raw=40 (U08 @7, 0.5 %/bit), engine byte @2 with the running bit set.
fn block() -> Vec<u8> {
    let mut b = vec![0u8; 16];
    b[0] = 7; // secl
    b[2] = 0x01; // engine: running bit [0:0]
    b[4..6].copy_from_slice(&3000u16.to_le_bytes());
    b[6] = 80; // coolantRaw
    b[7] = 40; // tps raw → 20.0 %
    b
}

fn channel(frame: &opentune_realtime::RealtimeFrame, name: &str) -> f64 {
    frame
        .channels
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("channel `{name}` missing from frame: {frame:?}"))
        .value
}

// ── 6.1: scalars scale, computed channels evaluate over decoded siblings ──

#[test]
fn decode_frame_scales_and_computes() {
    let def = fixture_definition();
    let frame = decode_frame(&def, &block());

    assert_eq!(channel(&frame, "rpm"), 3000.0);
    assert_eq!(channel(&frame, "tps"), 20.0, "raw 40 × scale 0.5");
    assert_eq!(
        channel(&frame, "coolant"),
        40.0,
        "computed: coolantRaw - 40 = 80 - 40"
    );
    assert_eq!(channel(&frame, "throttle"), 20.0, "computed: {{ tps }}");
    assert_eq!(channel(&frame, "secl"), 7.0);
    assert_eq!(channel(&frame, "running"), 1.0, "bits [0:0] of engine byte");
    assert!(
        frame.diagnostics.is_empty(),
        "nothing failed: {:?}",
        frame.diagnostics
    );
}

// ── 6.2: fail-open — one bad channel never blanks the frame ───────────────

#[test]
fn decode_frame_fails_open_on_bad_channel() {
    let mut def = fixture_definition();
    def.output_channels.push(OutputChannelDef::Computed {
        name: "broken".to_string(),
        expr: "nonexistentVar * 2".to_string(),
        units: String::new(),
    });

    let frame = decode_frame(&def, &block());

    // Every other channel is intact.
    assert_eq!(channel(&frame, "rpm"), 3000.0);
    assert_eq!(channel(&frame, "coolant"), 40.0);
    // The broken one lands in diagnostics, not in channels.
    assert!(
        frame.channels.iter().all(|c| c.name != "broken"),
        "broken channel must not carry a value"
    );
    assert!(
        frame.diagnostics.iter().any(|d| d.contains("broken")),
        "diagnostics must record the skipped channel: {:?}",
        frame.diagnostics
    );
}

// ── short buffer: out-of-range channels degrade, the rest decode ──────────

#[test]
fn decode_frame_fails_open_on_short_buffer() {
    let def = fixture_definition();
    // Only 6 bytes: rpm (U16@4) fits, coolantRaw (@6) and tps (@7) do not —
    // and the computed channels depending on them degrade too.
    let short = &block()[..6];
    let frame = decode_frame(&def, short);

    assert_eq!(channel(&frame, "rpm"), 3000.0);
    assert!(
        frame.diagnostics.iter().any(|d| d.contains("coolantRaw")),
        "short-buffer channel must be diagnosed: {:?}",
        frame.diagnostics
    );
    assert!(
        frame.diagnostics.iter().any(|d| d.contains("coolant")),
        "computed channel over a missing input must be diagnosed"
    );
    assert!(
        frame.channels.iter().all(|c| c.name != "tps"),
        "truncated channels must not report values"
    );
}

// ── computed chains resolve in file order ─────────────────────────────────

#[test]
fn computed_channels_chain_in_file_order() {
    let mut def = fixture_definition();
    // `chained` references the earlier computed `coolant` (80-40=40) — file
    // order evaluation must let later computed channels see earlier results.
    def.output_channels.push(OutputChannelDef::Computed {
        name: "chained".to_string(),
        expr: "coolant * 2".to_string(),
        units: String::new(),
    });

    let frame = decode_frame(&def, &block());
    assert_eq!(channel(&frame, "chained"), 80.0);
}
