// SPDX-License-Identifier: GPL-3.0-or-later
//! `SimTransport` — in-process loopback transport for hardware-free testing.
//!
//! Bytes written to a `SimTransport` land in a FIFO buffer; `read_exact` drains
//! that buffer.  This lets the `protocol` crate and tests run end-to-end
//! without any physical serial device.
//!
//! The `simulator` crate will use this type as the transport end-point for the
//! virtual ECU.

use std::collections::VecDeque;
use std::time::Duration;

use crate::{Result, Transport, TransportError};

/// Default timeout when the buffer has fewer bytes than requested.
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_millis(100);

/// An in-process loopback [`Transport`] backed by a byte queue.
///
/// Open/close track connection state; writes enqueue bytes; `read_exact`
/// dequeues exactly the requested number of bytes or returns
/// [`TransportError::Timeout`] when fewer bytes are available.
///
/// `flush` clears all pending bytes so stale data from a prior failed
/// exchange cannot leak into the next one — matching the contract of a real
/// serial-port `flush`.
pub struct SimTransport {
    open: bool,
    buf: VecDeque<u8>,
    read_timeout: Duration,
}

impl Default for SimTransport {
    fn default() -> Self {
        Self {
            open: false,
            buf: VecDeque::new(),
            read_timeout: DEFAULT_READ_TIMEOUT,
        }
    }
}

impl SimTransport {
    /// Create a `SimTransport` with a custom read-timeout value.
    pub fn with_timeout(read_timeout: Duration) -> Self {
        Self {
            read_timeout,
            ..Default::default()
        }
    }
}

impl Transport for SimTransport {
    /// Mark the link as open (idempotent).
    fn open(&mut self) -> Result<()> {
        self.open = true;
        Ok(())
    }

    /// Mark the link as closed (idempotent; does not clear the buffer).
    fn close(&mut self) -> Result<()> {
        self.open = false;
        Ok(())
    }

    fn is_open(&self) -> bool {
        self.open
    }

    /// Enqueue `bytes` into the loopback buffer.
    ///
    /// Fails with [`TransportError::Disconnected`] if the link is not open.
    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        if !self.open {
            return Err(TransportError::Disconnected);
        }
        self.buf.extend(bytes);
        Ok(())
    }

    /// Dequeue exactly `buf.len()` bytes from the loopback buffer.
    ///
    /// Returns [`TransportError::Timeout`] when fewer bytes are available,
    /// and [`TransportError::Disconnected`] if the link is not open.
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        if !self.open {
            return Err(TransportError::Disconnected);
        }
        if self.buf.len() < buf.len() {
            return Err(TransportError::Timeout(self.read_timeout));
        }
        for slot in buf.iter_mut() {
            // We verified buf.len() bytes are present above.
            *slot = self.buf.pop_front().expect("buffer has sufficient bytes");
        }
        Ok(())
    }

    /// Discard all pending bytes in the loopback buffer.
    fn flush(&mut self) -> Result<()> {
        self.buf.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_closed() {
        let t = SimTransport::default();
        assert!(!t.is_open());
    }

    #[test]
    fn open_sets_is_open_true() {
        let mut t = SimTransport::default();
        t.open().unwrap();
        assert!(t.is_open());
    }

    #[test]
    fn close_sets_is_open_false() {
        let mut t = SimTransport::default();
        t.open().unwrap();
        t.close().unwrap();
        assert!(!t.is_open());
    }

    #[test]
    fn round_trip_bytes() {
        let mut t = SimTransport::default();
        t.open().unwrap();
        t.write(b"hello").unwrap();
        let mut buf = [0u8; 5];
        t.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"hello");
    }

    #[test]
    fn fifo_ordering() {
        let mut t = SimTransport::default();
        t.open().unwrap();
        t.write(b"AB").unwrap();
        t.write(b"CD").unwrap();
        let mut buf = [0u8; 4];
        t.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"ABCD");
    }

    #[test]
    fn read_exact_times_out_when_insufficient_bytes() {
        let mut t = SimTransport::default();
        t.open().unwrap();
        t.write(b"X").unwrap();
        let mut buf = [0u8; 4];
        let err = t.read_exact(&mut buf).unwrap_err();
        assert!(matches!(err, TransportError::Timeout(_)));
    }

    #[test]
    fn write_while_closed_returns_disconnected() {
        let mut t = SimTransport::default();
        let err = t.write(b"Q").unwrap_err();
        assert!(matches!(err, TransportError::Disconnected));
    }

    #[test]
    fn read_while_closed_returns_disconnected() {
        let mut t = SimTransport::default();
        let mut buf = [0u8];
        let err = t.read_exact(&mut buf).unwrap_err();
        assert!(matches!(err, TransportError::Disconnected));
    }

    #[test]
    fn flush_clears_pending_bytes() {
        let mut t = SimTransport::default();
        t.open().unwrap();
        t.write(b"stale").unwrap();
        t.flush().unwrap();
        let mut buf = [0u8; 5];
        let err = t.read_exact(&mut buf).unwrap_err();
        assert!(matches!(err, TransportError::Timeout(_)));
    }

    #[test]
    fn double_open_is_idempotent() {
        let mut t = SimTransport::default();
        t.open().unwrap();
        t.open().unwrap();
        assert!(t.is_open());
    }

    #[test]
    fn double_close_is_idempotent() {
        let mut t = SimTransport::default();
        t.open().unwrap();
        t.close().unwrap();
        t.close().unwrap();
        assert!(!t.is_open());
    }
}
