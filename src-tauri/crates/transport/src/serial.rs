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

    fn clear_if_disconnected<T>(&mut self, result: &Result<T>) {
        if matches!(result, Err(TransportError::Disconnected)) {
            self.inner = None;
        }
    }
}

fn map_io_error(error: std::io::Error, timeout: std::time::Duration) -> TransportError {
    match error.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
            TransportError::Timeout(timeout)
        }
        std::io::ErrorKind::BrokenPipe
        | std::io::ErrorKind::NotConnected
        | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted
        | std::io::ErrorKind::UnexpectedEof => TransportError::Disconnected,
        _ => TransportError::Io(error),
    }
}

fn map_serial_error(error: serialport::Error) -> TransportError {
    match error.kind() {
        serialport::ErrorKind::NoDevice => TransportError::Disconnected,
        serialport::ErrorKind::Io(kind) => map_io_error(
            std::io::Error::new(kind, error.to_string()),
            std::time::Duration::ZERO,
        ),
        _ => TransportError::Io(std::io::Error::other(error.to_string())),
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
        let result = match self.inner.as_mut() {
            None => Err(TransportError::Disconnected),
            Some(port) => match port
                .set_timeout(self.config.write_timeout)
                .map_err(map_serial_error)
            {
                Err(error) => Err(error),
                Ok(()) => {
                    let write_result = port
                        .write_all(bytes)
                        .map_err(|e| map_io_error(e, self.config.write_timeout));
                    let restore_result = port
                        .set_timeout(self.config.read_timeout)
                        .map_err(map_serial_error);
                    write_result.and(restore_result)
                }
            },
        };
        self.clear_if_disconnected(&result);
        result
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        let result = {
            let port = self.inner.as_mut().ok_or(TransportError::Disconnected)?;
            port.read_exact(buf)
                .map_err(|e| map_io_error(e, self.config.read_timeout))
        };
        self.clear_if_disconnected(&result);
        result
    }

    fn flush(&mut self) -> Result<()> {
        let result = if let Some(port) = self.inner.as_mut() {
            port.flush()
                .map_err(|e| map_io_error(e, self.config.write_timeout))
                .and_then(|_| {
                    port.clear(serialport::ClearBuffer::All)
                        .map_err(map_serial_error)
                })
        } else {
            Ok(())
        };
        self.clear_if_disconnected(&result);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_and_would_block_map_to_timeout() {
        for kind in [std::io::ErrorKind::TimedOut, std::io::ErrorKind::WouldBlock] {
            let error = map_io_error(
                std::io::Error::new(kind, "timeout"),
                std::time::Duration::from_millis(321),
            );
            assert!(matches!(
                error,
                TransportError::Timeout(duration)
                    if duration == std::time::Duration::from_millis(321)
            ));
        }
    }

    #[test]
    fn unplug_like_io_errors_map_to_disconnected() {
        for kind in [
            std::io::ErrorKind::BrokenPipe,
            std::io::ErrorKind::NotConnected,
            std::io::ErrorKind::ConnectionReset,
            std::io::ErrorKind::ConnectionAborted,
            std::io::ErrorKind::UnexpectedEof,
        ] {
            let error = map_io_error(
                std::io::Error::new(kind, "device vanished"),
                std::time::Duration::from_secs(1),
            );
            assert!(matches!(error, TransportError::Disconnected));
        }
    }

    #[test]
    fn serial_no_device_maps_to_disconnected() {
        let error = serialport::Error::new(serialport::ErrorKind::NoDevice, "gone");
        assert!(matches!(
            map_serial_error(error),
            TransportError::Disconnected
        ));
    }

    #[test]
    fn unrelated_io_error_is_preserved() {
        let error = map_io_error(
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
            std::time::Duration::from_secs(1),
        );
        assert!(matches!(error, TransportError::Io(_)));
    }
}
