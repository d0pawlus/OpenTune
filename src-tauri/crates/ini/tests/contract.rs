// SPDX-License-Identifier: GPL-3.0-or-later
//! Contract tests for the `opentune-ini` M1 comms slice.
//!
//! These freeze the shape of [`CommsSettings`] so the protocol/transport agents
//! build against a fixed struct. They construct the type directly rather than
//! calling the `todo!()` parser.

use opentune_ini::{CommsSettings, Endianness, EnvelopeFormat, IniError};

/// A representative Speeduino comms block, field names mirroring the real INI.
fn speeduino_comms() -> CommsSettings {
    CommsSettings {
        signature: "speeduino 202504-dev".to_string(),
        query_command: "Q".to_string(),
        version_info: "S".to_string(),
        och_get_command: "r".to_string(),
        page_read_command: "p%2i%2o%2c".to_string(),
        page_value_write: "M%2i%2o%2c%v".to_string(),
        burn_command: "b%2i".to_string(),
        blocking_factor: 251,
        page_activation_delay_ms: 10,
        block_read_timeout_ms: 2_000,
        inter_write_delay_ms: 10,
        endianness: Endianness::Little,
        envelope: EnvelopeFormat::MsEnvelope10,
    }
}

#[test]
fn comms_settings_captures_signature_and_query() {
    let c = speeduino_comms();
    assert_eq!(c.signature, "speeduino 202504-dev");
    assert_eq!(c.query_command, "Q");
    assert_eq!(c.version_info, "S");
}

#[test]
fn comms_settings_keeps_raw_command_templates() {
    // The `ini` crate must not expand %2i/%2o/%2c/%v — that is the protocol
    // crate's job. The contract is: store the template verbatim.
    let c = speeduino_comms();
    assert_eq!(c.page_read_command, "p%2i%2o%2c");
    assert_eq!(c.page_value_write, "M%2i%2o%2c%v");
    assert_eq!(c.burn_command, "b%2i");
}

#[test]
fn comms_settings_models_endianness_and_envelope() {
    let c = speeduino_comms();
    assert_eq!(c.endianness, Endianness::Little);
    assert_eq!(c.envelope, EnvelopeFormat::MsEnvelope10);
}

#[test]
fn ini_error_reports_missing_key() {
    let e = IniError::MissingKey("signature".to_string());
    assert!(e.to_string().contains("signature"));
}
