// SPDX-License-Identifier: GPL-3.0-or-later
//! `opentune-realtime` — the M3 realtime dashboard seam.
//!
//! Decodes raw `read_output_channels` ("och") byte blocks streamed by the ECU
//! into physical-unit channel values, against the `[OutputChannels]` entries
//! declared in an [`opentune_ini::Definition`]. This module only freezes the
//! shape and pins [`decode_frame`]'s signature with a minimal stub; the real
//! per-channel decode (scalar/bits/computed-expression) is a later M3 task.

/// One decoded channel value in physical units, or a diagnostic if it failed.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelValue {
    pub name: String,
    pub value: f64,
}

/// A fully decoded realtime frame: every successfully decoded channel.
#[derive(Debug, Clone, PartialEq)]
pub struct RealtimeFrame {
    pub channels: Vec<ChannelValue>,
    /// Names that failed to decode this frame (fail-open — never blanks the frame).
    pub diagnostics: Vec<String>,
}

/// Decode one raw och block against the definition's channels into physical values.
///
/// Fails open per channel: a bad expr/short buffer records a diagnostic and
/// skips that channel rather than failing the whole frame.
///
/// Stub for this task: always returns an empty frame. Real per-channel
/// decoding (scalar/bits/computed) is a later M3 task.
pub fn decode_frame(_def: &opentune_ini::Definition, _block: &[u8]) -> RealtimeFrame {
    RealtimeFrame {
        channels: Vec::new(),
        diagnostics: Vec::new(),
    }
}

/// Errors the realtime polling loop can produce.
#[derive(Debug, Clone, PartialEq)]
pub enum RealtimeError {
    /// No active transport/connection to poll.
    NotConnected,
    /// The poll request itself failed (transport/protocol error), with detail.
    Poll(String),
}
