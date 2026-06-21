// SPDX-License-Identifier: GPL-3.0-or-later
//! Contract tests for the `opentune-transport` shared seam.
//!
//! These pin the *shape* of the M1 transport interface so the parallel
//! component agents implement against a fixed contract. They assert the public
//! types exist and compose as documented; they do not exercise `todo!()` bodies.

use std::time::Duration;

use opentune_transport::{PortInfo, SerialConfig, Transport, TransportError};

#[test]
fn serial_config_default_uses_speeduino_baud() {
    let cfg = SerialConfig::default();
    assert_eq!(cfg.baud, 115_200);
    assert!(cfg.read_timeout > Duration::ZERO);
    assert!(cfg.write_timeout > Duration::ZERO);
}

#[test]
fn transport_error_distinguishes_timeout_from_disconnect() {
    let timeout = TransportError::Timeout(Duration::from_millis(2_000));
    let dropped = TransportError::Disconnected;
    assert!(matches!(timeout, TransportError::Timeout(_)));
    assert!(matches!(dropped, TransportError::Disconnected));
    // Distinct messages so reconnect logic and the UI can tell them apart.
    assert_ne!(timeout.to_string(), dropped.to_string());
}

#[test]
fn port_info_carries_usb_identity() {
    let port = PortInfo {
        name: "/dev/tty.usbserial-1".to_string(),
        vid: Some(0x2341),
        pid: Some(0x0043),
        product: Some("Arduino Mega 2560".to_string()),
    };
    assert_eq!(port.name, "/dev/tty.usbserial-1");
    assert_eq!(port.vid, Some(0x2341));
}

/// A trivial in-test implementation proves the trait is object-safe and
/// implementable without touching hardware — the property `SimTransport` and
/// `SerialTransport` agents rely on.
#[derive(Default)]
struct NullTransport {
    open: bool,
}

impl Transport for NullTransport {
    fn open(&mut self) -> opentune_transport::Result<()> {
        self.open = true;
        Ok(())
    }
    fn close(&mut self) -> opentune_transport::Result<()> {
        self.open = false;
        Ok(())
    }
    fn is_open(&self) -> bool {
        self.open
    }
    fn write(&mut self, _bytes: &[u8]) -> opentune_transport::Result<()> {
        Ok(())
    }
    fn read_exact(&mut self, _buf: &mut [u8]) -> opentune_transport::Result<()> {
        Ok(())
    }
    fn flush(&mut self) -> opentune_transport::Result<()> {
        Ok(())
    }
}

#[test]
fn transport_trait_is_implementable_and_object_safe() {
    let mut t: Box<dyn Transport> = Box::new(NullTransport::default());
    assert!(!t.is_open());
    t.open().unwrap();
    assert!(t.is_open());
    t.write(b"Q").unwrap();
    t.close().unwrap();
    assert!(!t.is_open());
}
