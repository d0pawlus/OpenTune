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
fn comms_keys_scattered_into_constants_and_output_channels() {
    // Layout mirrors reference/speeduino.ini @ 0832dc1d l.4-10 + l.240-274 + l.5352-5353.
    let ini = r#"
[MegaTune]
   queryCommand   = "Q"
   signature      = "speeduino 202504-dev"
   versionInfo    = "S"

[Constants]
    pageSize            = 128,   288
    pageReadCommand     = "p%2i%2o%2c", "p%2i%2o%2c"
    pageValueWrite      = "M%2i%2o%2c%v", "M%2i%2o%2c%v"
    burnCommand         = "b%2i", "b%2i"
    blockingFactor      = 121
    blockReadTimeout    = 2000

page = 1
      reqFuel    = scalar, U16,  0, "ms",   0.1,  0.0,  0.0,  6553.5,  1

[OutputChannels]
  ochGetCommand    = "r\$tsCanId\x30%2o%2c"
  ochBlockSize     =  139
"#;
    let comms = opentune_ini::parse_comms(ini).expect("scattered keys must resolve");
    assert_eq!(comms.signature, "speeduino 202504-dev");
    assert_eq!(comms.page_read_command, "p%2i%2o%2c"); // first list element
    assert_eq!(comms.page_value_write, "M%2i%2o%2c%v");
    assert_eq!(comms.burn_command, "b%2i");
    assert_eq!(comms.blocking_factor, 121);
    assert_eq!(comms.block_read_timeout_ms, 2000);
    assert_eq!(comms.och_get_command, r"r\$tsCanId\x30%2o%2c");
    assert_eq!(comms.och_block_size, 139);
}

#[test]
fn megatune_value_wins_when_key_is_also_scattered_into_constants() {
    // A synthetic both-declared case (never observed in the real file — see
    // `parser::parse_comms`'s doc comment): `blockingFactor` appears in BOTH
    // `[MegaTune]` and `[Constants]` with different values. The dedicated
    // comms section must win over the scattered one, pinning the
    // find_raw/extract_scattered_comms ordering fix (M4 Task 2).
    let ini = r#"
[MegaTune]
   signature      = "speeduino 202504-dev"
   queryCommand   = "Q"
   versionInfo    = "S"
   ochGetCommand  = "r"
   pageReadCommand = "p%2i%2o%2c"
   pageValueWrite = "M%2i%2o%2c%v"
   burnCommand    = "b%2i"
   blockingFactor = 251
   blockReadTimeout = 2000

[Constants]
    pageSize = 128
    blockingFactor = 121
page = 1
      reqFuel = scalar, U16, 0, "ms", 0.1, 0.0, 0.0, 6553.5, 1
"#;
    let comms = opentune_ini::parse_comms(ini).expect("must parse");
    assert_eq!(
        comms.blocking_factor, 251,
        "the [MegaTune] value must win over the scattered [Constants] one"
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

#[test]
fn raw_real_ini_resolves_scattered_keys_from_live_if_branches() {
    // Final-review finding: called on RAW (unpreprocessed) text, parse_comms
    // used to collect scattered keys from EVERY `#if` branch and resolve
    // last-wins to the dead trailing `#if COMMS_COMPAT` value
    // (blockingFactor 121) instead of the `#else` branch's 251 that
    // parse_definition yields on the same file. parse_comms now preprocesses
    // internally (same empty symbol set as parse_definition), so both entry
    // points agree.
    let raw = include_str!("fixtures/speeduino-real-0832dc1d.ini");
    let comms = parse_comms(raw).expect("raw real INI parses");
    assert_eq!(
        comms.blocking_factor, 251,
        "raw-text entry point must match parse_definition's preprocessed value"
    );
}
