// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for `parse_comms` — the M1 comms-settings extractor.
//!
//! Structure ported from `hyper-tuner/ini` (MIT, ADR-0006) and the real
//! Speeduino INI (GPL-3). Tests drive the implementation via TDD.

use opentune_ini::parse_comms;

fn speeduino_ini() -> &'static str {
    include_str!("fixtures/speeduino_comms.ini")
}

#[test]
fn parses_signature_from_speeduino_fixture() {
    let c = parse_comms(speeduino_ini()).expect("should parse");
    assert_eq!(c.signature, "speeduino 202504-dev");
}
