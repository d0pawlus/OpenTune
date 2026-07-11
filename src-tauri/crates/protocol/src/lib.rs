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
//! a first-class type.
//!
//! # M2: page read / write / burn
//!
//! [`Protocol::read_page`]/[`Protocol::write`]/[`Protocol::burn`] extend the
//! M1 seam with inline-page-id operations (no stateful page-select — see
//! [`pages`]), implemented for the generic engine in
//! [`pages`]/[`MsProtocol`] and confirmed against Speeduino `comms.cpp` /
//! `comms_legacy.cpp` @ `63fd68e9`.

mod engine;
pub mod pages;
pub mod reconnect;

pub use engine::{crc32_of, MsProtocol};
pub use pages::{expand_template, TemplateParams};

use opentune_ini::{CommsSettings, PageDef};
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
    /// A comms-settings command template (`pageReadCommand`, `pageValueWrite`,
    /// `burnCommand`, …) could not be expanded — e.g. a truncated/invalid
    /// `\xNN` hex escape. Indicates a bad INI, not a wire/runtime failure.
    /// Additive M2 variant: `ProtocolError` is not `Serialize` (doesn't cross
    /// the specta/IPC seam) and no existing code matches it exhaustively, so
    /// this does not change the M1-frozen shape's observable contract.
    #[error("malformed command template: {0}")]
    MalformedTemplate(String),
    /// A page operation cannot be represented by the MS/TS wire format
    /// (for example, a page id above 255 or a byte count above `u16::MAX`).
    #[error("invalid protocol request: {0}")]
    InvalidRequest(String),
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

    /// Read one full memory page. `page` supplies both the page id and its
    /// expected byte size ([`PageDef::size`]) so implementations know how
    /// many bytes to read back.
    ///
    /// No page-select step: the page id travels inline in every command
    /// (Speeduino `comms.cpp`/`comms_legacy.cpp` — the legacy `'P'`
    /// select is unused by current firmware and is not implemented here).
    fn read_page(&mut self, page: PageDef) -> Result<Vec<u8>>;

    /// Write `bytes` at `offset` within `page`. Applies live to ECU RAM;
    /// [`Protocol::burn`] persists to flash separately.
    ///
    /// **Guarantee:** `Ok(())` means the command bytes were sent and — in
    /// CRC-framed comms — a CRC-valid acknowledgement was received (the
    /// firmware's specific return code, e.g. a range-rejection, is not
    /// decoded). Any error — including
    /// [`opentune_transport::TransportError::Disconnected`] surfacing
    /// mid-exchange as [`ProtocolError::Transport`] — means the caller MUST
    /// treat the write as **not confirmed applied**. There is no partial
    /// "some bytes landed" state visible to the caller: either this returns
    /// `Ok(())`, or the caller must assume the ECU's state relative to this
    /// write is unknown and re-verify (e.g. by re-reading the page).
    fn write(&mut self, page: u16, offset: u16, bytes: &[u8]) -> Result<()>;

    /// Persist `page`'s current RAM contents to flash (`savePage` semantics —
    /// per-page, not whole-config).
    fn burn(&mut self, page: u16) -> Result<()>;

    /// Read `len` bytes of the output-channel block starting at `offset` by
    /// expanding `ochGetCommand` (which carries `%2o`/`%2c` windows) and reading the
    /// response. In MsEnvelope10 the payload is `[SERIAL_RC_OK, ...len bytes]`; the
    /// leading status byte is stripped. In Plain the response is `len` raw bytes.
    fn read_output_channels(&mut self, offset: u16, len: u16) -> Result<Vec<u8>>;
}
