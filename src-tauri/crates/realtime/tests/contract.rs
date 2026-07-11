// SPDX-License-Identifier: GPL-3.0-or-later
//! Contract tests for the `opentune-realtime` M3 seam.
//!
//! These pin [`decode_frame`]'s signature and the [`RealtimeFrame`] /
//! [`ChannelValue`] / [`RealtimeError`] shapes on hand-built data, so
//! downstream tasks (the real decoder, the polling loop, the IPC layer)
//! build against a fixed contract. `decode_frame` is a minimal stub for
//! now — it returns an empty frame; the real per-channel decode is a
//! later M3 task.

use opentune_ini::{
    CommsSettings, Definition, Endianness, EnvelopeFormat, FrontPageDef, OutputChannelDef, PageDef,
    ScalarType,
};
use opentune_realtime::{decode_frame, ChannelValue, RealtimeError, RealtimeFrame};

fn speeduino_comms() -> CommsSettings {
    CommsSettings {
        signature: "speeduino 202504-dev".to_string(),
        query_command: "Q".to_string(),
        version_info: "S".to_string(),
        och_get_command: "A".to_string(),
        page_read_command: "p%2i%2o%2c".to_string(),
        page_value_write: "M%2i%2o%2c%v".to_string(),
        burn_command: "b%2i".to_string(),
        blocking_factor: 121,
        page_activation_delay_ms: 10,
        block_read_timeout_ms: 2_000,
        inter_write_delay_ms: 10,
        endianness: Endianness::Little,
        envelope: EnvelopeFormat::Plain,
        och_block_size: 8,
    }
}

/// A hand-built `Definition` with one scalar output channel — enough to
/// exercise `decode_frame`'s signature without a parser.
fn hand_built_definition() -> Definition {
    Definition {
        comms: speeduino_comms(),
        pages: vec![PageDef {
            number: 1,
            size: 128,
        }],
        constants: vec![],
        pc_variables: vec![],
        menus: vec![],
        dialogs: vec![],
        tables: vec![],
        curves: vec![],
        diagnostics: vec![],
        output_channels: vec![OutputChannelDef::Scalar {
            name: "map".to_string(),
            kind: ScalarType::U16,
            offset: 4,
            units: "kpa".to_string(),
            scale: 1.0,
            translate: 0.0,
        }],
        gauges: vec![],
        frontpage: FrontPageDef {
            gauge_slots: vec![],
            indicators: vec![],
        },
        ve_analyze: None,
    }
}

#[test]
fn decode_frame_signature_accepts_definition_and_block() {
    // Pins the seam: (&Definition, &[u8]) -> RealtimeFrame. Task 6 replaced
    // the Task 0 stub (which returned an empty frame) with the real decoder,
    // so an all-zero 8-byte block now decodes the single declared channel
    // (`map`, U16 @4, scale 1.0) to 0.0 — same signature, real behavior.
    let def = hand_built_definition();
    let block = vec![0u8; def.comms.och_block_size as usize];
    let frame = decode_frame(&def, &block);
    assert_eq!(
        frame,
        RealtimeFrame {
            channels: vec![ChannelValue {
                name: "map".to_string(),
                value: 0.0,
            }],
            diagnostics: vec![],
        }
    );
}

#[test]
fn channel_value_carries_name_and_physical_value() {
    let v = ChannelValue {
        name: "rpm".to_string(),
        value: 3_500.0,
    };
    assert_eq!(v.name, "rpm");
    assert_eq!(v.value, 3_500.0);
}

#[test]
fn realtime_frame_separates_channels_from_diagnostics() {
    // Fail-open contract: failed channels land in `diagnostics` by name;
    // successful ones stay in `channels`. A bad channel never blanks a frame.
    let frame = RealtimeFrame {
        channels: vec![ChannelValue {
            name: "map".to_string(),
            value: 101.3,
        }],
        diagnostics: vec!["coolant".to_string()],
    };
    assert_eq!(frame.channels.len(), 1);
    assert_eq!(frame.diagnostics, vec!["coolant".to_string()]);
}

#[test]
fn realtime_error_models_not_connected_and_poll_failure() {
    let not_connected = RealtimeError::NotConnected;
    let poll = RealtimeError::Poll("read timed out".to_string());
    assert_eq!(not_connected, RealtimeError::NotConnected);
    assert!(matches!(poll, RealtimeError::Poll(msg) if msg == "read timed out"));
}
