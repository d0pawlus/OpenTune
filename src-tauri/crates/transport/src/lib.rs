// SPDX-License-Identifier: GPL-3.0-or-later
//! `opentune-transport` — moves raw bytes to and from an ECU.
//!
//! This crate owns the byte-level link only; it **knows nothing about protocol
//! semantics** (no signatures, no pages, no CRC) — see
//! [ARCHITECTURE.md §5.1](../../../docs/ARCHITECTURE.md#51-transport--talking-to-hardware).
//!
//! # M1 contract (shared seam)
//!
//! This file is the *fixed interface* the M1 component agents implement against.
//! The shapes here are the contract; the function bodies are `todo!()` stubs the
//! implementing agents replace. Downstream crates (`opentune-protocol`, the
//! simulator) depend only on the [`Transport`] trait and the error/info types —
//! never on a concrete implementation.
//!
//! Concrete implementations landed by M1:
//! - `SerialTransport` — USB/UART via the `serialport` crate.
//! - `SimTransport` — an in-process bridge to `opentune-simulator`.
//!
//! `TcpTransport` is intentionally **out of M1 scope** (YAGNI) but the trait
//! leaves room for it.

use std::time::Duration;

/// Errors a transport can surface. Byte-level only — no protocol concepts leak
/// in here. `protocol`/`realtime` map these into richer errors for the UI.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// The named port could not be opened (busy, missing, permission denied).
    #[error("failed to open port `{port}`: {source}")]
    Open {
        port: String,
        #[source]
        source: std::io::Error,
    },

    /// A read or write exceeded its configured timeout. The reconnect logic
    /// (pain point #1) keys off this to detect a dropped link.
    #[error("operation timed out after {0:?}")]
    Timeout(Duration),

    /// The device disappeared mid-conversation (USB unplug / power-save).
    /// Distinguished from [`TransportError::Timeout`] so reconnect can react
    /// immediately rather than waiting out the timeout window.
    #[error("device disconnected")]
    Disconnected,

    /// Any other underlying I/O failure.
    #[error("transport I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result alias for transport operations.
pub type Result<T> = std::result::Result<T, TransportError>;

/// A serial port discovered by [`enumerate_ports`].
///
/// `vid`/`pid` are the USB identifiers when known; the connect UI uses them to
/// flag *likely* ECUs (e.g. the Speeduino/Arduino VID) without hardcoding any
/// single device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    /// OS port name (`/dev/tty.usbserial-*`, `COM3`, …) — the handle passed to
    /// [`Transport::open`].
    pub name: String,
    /// USB vendor id, if this is a USB serial device.
    pub vid: Option<u16>,
    /// USB product id, if this is a USB serial device.
    pub pid: Option<u16>,
    /// Human-readable product string, if the OS exposes one.
    pub product: Option<String>,
}

/// Settings used to open a serial link. Baud and timeouts come from user prefs
/// and/or the INI comms settings (see the `opentune-ini`-supplied defaults).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialConfig {
    /// Baud rate (Speeduino default is 115_200).
    pub baud: u32,
    /// Per-operation read timeout. Reconnect treats expiry as a drop signal.
    pub read_timeout: Duration,
    /// Per-operation write timeout.
    pub write_timeout: Duration,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            baud: 115_200,
            read_timeout: Duration::from_millis(2_000),
            write_timeout: Duration::from_millis(2_000),
        }
    }
}

/// The byte pipe to an ECU.
///
/// **Single-conversation rule:** the protocol layer serializes all access
/// through one owner task (see
/// [ARCHITECTURE.md §9](../../../docs/ARCHITECTURE.md#9-concurrency--performance-model)),
/// so implementations need not be internally `Sync` against concurrent callers —
/// but they MUST be `Send` so the owner task can hold one across threads.
///
/// The trait is intentionally blocking + simple (`read_exact`/`write`/`flush`).
/// Async serialization lives one layer up; keeping the transport blocking
/// matches the `serialport` crate and keeps `SimTransport` trivial to test.
/// This is a deliberate, recorded choice for M1.
pub trait Transport: Send {
    /// Open the link. Idempotent implementations may no-op if already open.
    fn open(&mut self) -> Result<()>;

    /// Close the link. Must be safe to call more than once.
    fn close(&mut self) -> Result<()>;

    /// True while the link is believed to be up. Cheap; does no I/O.
    fn is_open(&self) -> bool;

    /// Write the whole buffer or fail. Partial writes are an error.
    fn write(&mut self, bytes: &[u8]) -> Result<()>;

    /// Read **exactly** `buf.len()` bytes or fail with [`TransportError::Timeout`].
    /// The protocol layer always knows how many bytes it expects (frame lengths
    /// come from the INI), so an exact read is the right primitive.
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()>;

    /// Discard any buffered input/output so a stale half-response from a prior
    /// timeout can't corrupt the next read.
    fn flush(&mut self) -> Result<()>;
}

/// Enumerate the serial ports currently visible to the OS.
///
/// M1 implements this over the `serialport` crate. Returns an empty list — not
/// an error — when no ports exist.
pub fn enumerate_ports() -> Result<Vec<PortInfo>> {
    todo!("M1: enumerate serial ports via the `serialport` crate")
}
