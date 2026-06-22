// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for `parse_comms` — the M1 comms-settings extractor.
//!
//! Structure ported from `hyper-tuner/ini` (MIT, ADR-0006) and the real
//! Speeduino INI (GPL-3). Tests drive the implementation via TDD.

use opentune_ini::{parse_comms, Endianness, EnvelopeFormat};

fn speeduino_ini() -> &'static str {
    include_str!("fixtures/speeduino_comms.ini")
}

fn minimal_ini() -> &'static str {
    include_str!("fixtures/minimal_comms.ini")
}

#[test]
fn parses_signature_from_speeduino_fixture() {
    let c = parse_comms(speeduino_ini()).expect("should parse");
    assert_eq!(c.signature, "speeduino 202504-dev");
}

#[test]
fn parses_all_comms_fields_from_speeduino_fixture() {
    let c = parse_comms(speeduino_ini()).expect("should parse");
    assert_eq!(c.query_command, "Q");
    assert_eq!(c.version_info, "S");
    assert_eq!(c.och_get_command, "r");
    assert_eq!(c.page_read_command, "p%2i%2o%2c");
    assert_eq!(c.page_value_write, "M%2i%2o%2c%v");
    assert_eq!(c.burn_command, "b%2i");
    assert_eq!(c.blocking_factor, 251);
    assert_eq!(c.page_activation_delay_ms, 10);
    assert_eq!(c.block_read_timeout_ms, 2000);
    assert_eq!(c.inter_write_delay_ms, 10);
    assert_eq!(c.endianness, Endianness::Little);
    assert_eq!(c.envelope, EnvelopeFormat::MsEnvelope10);
}

#[test]
fn minimal_fixture_uses_defaults_for_optional_fields() {
    let c = parse_comms(minimal_ini()).expect("should parse minimal fixture");
    assert_eq!(c.signature, "test ECU v1.0");
    assert_eq!(c.block_read_timeout_ms, 1500);
    assert_eq!(c.blocking_factor, 121);
    // Optional fields absent → defaults
    assert_eq!(c.page_activation_delay_ms, 0);
    assert_eq!(c.inter_write_delay_ms, 0);
    assert_eq!(c.endianness, Endianness::Little);
    assert_eq!(c.envelope, EnvelopeFormat::Plain);
}

#[test]
fn returns_error_when_signature_is_absent() {
    // A [MegaTune] block with every required field except `signature`.
    let ini = "[MegaTune]\n\
               queryCommand = \"Q\"\n\
               versionInfo = \"S\"\n\
               ochGetCommand = \"A\"\n\
               pageReadCommand = \"r%2i\"\n\
               pageValueWrite = \"w%2i\"\n\
               burnCommand = \"b%2i\"\n\
               blockingFactor = 121\n\
               blockReadTimeout = 1000\n";
    let err = parse_comms(ini).unwrap_err();
    assert!(
        err.to_string().contains("signature"),
        "expected 'signature' in error: {err}"
    );
}

#[test]
fn parses_tuner_studio_section_alias() {
    // Some firmwares use [TunerStudio] instead of [MegaTune].
    let ini = "[TunerStudio]\n\
               signature = \"rusEFI v1.0\"\n\
               queryCommand = \"Q\"\n\
               versionInfo = \"S\"\n\
               ochGetCommand = \"A\"\n\
               pageReadCommand = \"r\"\n\
               pageValueWrite = \"w\"\n\
               burnCommand = \"b\"\n\
               blockingFactor = 64\n\
               blockReadTimeout = 500\n";
    let c = parse_comms(ini).expect("should parse [TunerStudio] section");
    assert_eq!(c.signature, "rusEFI v1.0");
    assert_eq!(c.blocking_factor, 64);
}
