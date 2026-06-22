// SPDX-License-Identifier: GPL-3.0-or-later
//! `SerialTransport` — USB/UART transport via the `serialport` crate.
//!
//! M1 implementation: open/close a real serial port with configurable baud and
//! timeouts.  Port enumeration lives in [`crate::enumerate_ports`].
//!
//! `SerialTransport` implements `Send` via the `serialport` crate's own
//! `Send` impl.

use std::io::{Read, Write as IoWrite};

use crate::{Result, SerialConfig, Transport, TransportError};

/// A transport backed by a real serial port.
///
/// Obtain one by constructing it with [`SerialTransport::new`] and then
/// calling [`Transport::open`].
pub struct SerialTransport {
    port_name: String,
    config: SerialConfig,
    inner: Option<Box<dyn serialport::SerialPort>>,
}

impl SerialTransport {
    /// Create a new (closed) `SerialTransport` for the given port.
    pub fn new(port_name: impl Into<String>, config: SerialConfig) -> Self {
        Self {
            port_name: port_name.into(),
            config,
            inner: None,
        }
    }
}

impl Transport for SerialTransport {
    /// Open the serial port.  Idempotent — no-ops if already open.
    fn open(&mut self) -> Result<()> {
        if self.inner.is_some() {
            return Ok(());
        }
        let port = serialport::new(&self.port_name, self.config.baud)
            .timeout(self.config.read_timeout)
            .open()
            .map_err(|e| TransportError::Open {
                port: self.port_name.clone(),
                source: std::io::Error::other(e.to_string()),
            })?;
        self.inner = Some(port);
        Ok(())
    }

    /// Close the serial port.  Idempotent — no-ops if already closed.
    fn close(&mut self) -> Result<()> {
        self.inner = None;
        Ok(())
    }

    fn is_open(&self) -> bool {
        self.inner.is_some()
    }

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let port = self.inner.as_mut().ok_or(TransportError::Disconnected)?;
        port.write_all(bytes).map_err(TransportError::from)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        let port = self.inner.as_mut().ok_or(TransportError::Disconnected)?;
        port.read_exact(buf).map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                TransportError::Timeout(self.config.read_timeout)
            } else {
                TransportError::Io(e)
            }
        })
    }

    fn flush(&mut self) -> Result<()> {
        if let Some(port) = self.inner.as_mut() {
            port.flush().map_err(TransportError::from)?;
            port.clear(serialport::ClearBuffer::All)
                .map_err(|e| TransportError::Io(std::io::Error::other(e.to_string())))?;
        }
        Ok(())
    }
}
