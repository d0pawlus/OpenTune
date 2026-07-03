// SPDX-License-Identifier: GPL-3.0-or-later
//! `opentune-realtime` — the M3 realtime dashboard seam.
//!
//! Decodes raw `read_output_channels` ("och") byte blocks streamed by the ECU
//! into physical-unit channel values, against the `[OutputChannels]` entries
//! declared in an [`opentune_ini::Definition`], and paces UI emission via the
//! coalescing [`RealtimePoller`]. Decode-only by design: this crate depends
//! on `opentune-ini` alone — the poller reaches the wire through a
//! caller-supplied closure, never a protocol handle.

mod decode;
mod poll;

pub use decode::decode_frame;
pub use poll::RealtimePoller;

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

/// Errors the realtime polling loop can produce.
#[derive(Debug, Clone, PartialEq)]
pub enum RealtimeError {
    /// No active transport/connection to poll.
    NotConnected,
    /// The poll request itself failed (transport/protocol error), with detail.
    Poll(String),
}
