// SPDX-License-Identifier: GPL-3.0-or-later
//! `opentune-protocol` — the conversation with the ECU.
//!
//! Sits above [`opentune_transport`] (raw bytes) and below `realtime`/the app.
//! It is *largely data-driven* from the INI comms settings
//! ([`opentune_ini::CommsSettings`]) — see
//! [protocol.md](../../../docs/protocol.md). Per
//! [ADR-0006](../../../docs/adr/0006-reuse-existing-parsers.md), command bytes
//! and quirks are taken from the open Speeduino/rusEFI firmware sources, not
//! re-derived, and confirmed against the simulator with tests.
//!
//! # M1 contract (shared seam)
//!
//! M1 covers **connect & identify** only: handshake → signature → version, plus
//! the [`ConnectionState`] machine that makes reliable reconnect (pain point #1)
//! a first-class type. Page read/write/burn are M2 — their trait methods are
//! declared here (so the seam is stable) but stubbed with `todo!()`.

use opentune_ini::CommsSettings;
use opentune_transport::TransportError;

/// The identity an ECU reports during the handshake — the result of M1's core
/// flow. The UI shows this; the app matches [`Self::signature`] against the
/// loaded INI's [`CommsSettings::signature`] before trusting any memory layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcuIdentity {
    /// Raw signature string reported by the firmware (response to `queryCommand`).
    pub signature: String,
    /// Human-readable version string (response to `versionInfo`); may be empty
    /// if the firmware does not implement it.
    pub version: String,
}

impl EcuIdentity {
    /// True when the reported [`Self::signature`] exactly matches the one the
    /// loaded INI declares. A mismatch means the INI can misinterpret memory, so
    /// the connect flow warns/blocks (see protocol.md "Signature matching").
    pub fn matches(&self, comms: &CommsSettings) -> bool {
        self.signature == comms.signature
    }
}

/// The connection lifecycle, surfaced to the UI as a single source of truth.
///
/// `Reconnecting` is a **named, first-class state** (research pain point #1):
/// when the link drops mid-tune the engine transitions here and retries with
/// backoff + `secl` resync rather than dying — the user sees "reconnecting…",
/// not an error dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// No link; idle.
    Disconnected,
    /// Handshake in progress (opening transport, querying signature).
    Connecting,
    /// Linked and identified.
    Connected { identity: EcuIdentity },
    /// Link was lost and is being re-established. `attempt` counts retries since
    /// the drop so the backoff schedule and UI can reflect progress.
    Reconnecting { attempt: u32 },
    /// Gave up (e.g. signature mismatch, or retries exhausted). `reason` is a
    /// diagnostic the UI can surface.
    Failed { reason: String },
}

/// Errors the protocol engine can produce. Wraps lower-level transport errors
/// and adds protocol-level failures (bad CRC, signature mismatch).
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    /// The byte layer failed (timeout, disconnect, I/O).
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),
    /// A CRC-framed response failed its CRC32 check.
    #[error("CRC mismatch (expected {expected:#010x}, got {actual:#010x})")]
    CrcMismatch { expected: u32, actual: u32 },
    /// The ECU's signature did not match the loaded INI.
    #[error("signature mismatch: ECU reports `{reported}`, INI expects `{expected}`")]
    SignatureMismatch { reported: String, expected: String },
    /// The ECU sent a malformed or unexpected response.
    #[error("malformed response: {0}")]
    MalformedResponse(String),
}

/// Result alias for protocol operations.
pub type Result<T> = std::result::Result<T, ProtocolError>;

/// The conversation with an ECU. Implemented in M1 by the generic MS/TS engine
/// over a [`opentune_transport::Transport`], and (for tests/dev) by the
/// simulator. Methods beyond M1's identify slice are declared for a stable seam
/// and stubbed.
pub trait Protocol {
    /// Perform the handshake and return the ECU's identity (signature +
    /// version). This is M1's headline operation.
    fn identify(&mut self) -> Result<EcuIdentity>;

    /// Query just the signature (response to `queryCommand`).
    fn signature(&mut self) -> Result<String>;

    /// Query the version string (response to `versionInfo`).
    fn version(&mut self) -> Result<String>;

    /// Read the firmware's running second counter (`secl`). Used by reconnect to
    /// detect whether the ECU rebooted during a drop and resync accordingly.
    fn read_secl(&mut self) -> Result<u8>;
}

/// Read one config page (M2). Declared here only to keep the trait's eventual
/// shape visible; not part of the M1 [`Protocol`] trait above.
pub fn read_page_placeholder() {
    todo!("M2: page read/write/burn — out of M1 scope")
}
