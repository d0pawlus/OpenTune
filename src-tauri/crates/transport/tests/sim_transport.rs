// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for `SimTransport` and `enumerate_ports`.
//!
//! These tests run without any physical hardware.

use std::time::Duration;

use opentune_transport::{
    enumerate_ports, sim::SimTransport, PortInfo, SerialConfig, Transport, TransportError,
};

// ---------------------------------------------------------------------------
// enumerate_ports
// ---------------------------------------------------------------------------

#[test]
fn enumerate_ports_returns_ok_with_no_hardware() {
    let ports = enumerate_ports();
    assert!(
        ports.is_ok(),
        "enumerate_ports failed: {:?}",
        ports.unwrap_err()
    );
}

#[test]
fn enumerate_ports_entries_have_non_empty_names() {
    let ports = enumerate_ports().expect("enumerate_ports failed");
    for port in &ports {
        assert!(!port.name.is_empty(), "PortInfo has empty name: {port:?}");
    }
}

// ---------------------------------------------------------------------------
// SimTransport — open / close
// ---------------------------------------------------------------------------

#[test]
fn sim_starts_closed() {
    let t = SimTransport::default();
    assert!(!t.is_open());
}

#[test]
fn sim_open_then_close() {
    let mut t = SimTransport::default();
    t.open().expect("open should succeed");
    assert!(t.is_open());
    t.close().expect("close should succeed");
    assert!(!t.is_open());
}

#[test]
fn sim_double_open_is_idempotent() {
    let mut t = SimTransport::default();
    t.open().unwrap();
    t.open().expect("second open should not error");
    assert!(t.is_open());
}

#[test]
fn sim_double_close_is_idempotent() {
    let mut t = SimTransport::default();
    t.open().unwrap();
    t.close().unwrap();
    t.close().expect("second close should not error");
    assert!(!t.is_open());
}

// ---------------------------------------------------------------------------
// SimTransport — write / read round-trip
// ---------------------------------------------------------------------------

#[test]
fn sim_round_trip_single_byte() {
    let mut t = SimTransport::default();
    t.open().unwrap();
    t.write(b"Q").unwrap();
    let mut buf = [0u8; 1];
    t.read_exact(&mut buf).unwrap();
    assert_eq!(buf, *b"Q");
}

#[test]
fn sim_round_trip_multi_byte() {
    let mut t = SimTransport::default();
    t.open().unwrap();
    t.write(b"hello").unwrap();
    let mut buf = [0u8; 5];
    t.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"hello");
}

#[test]
fn sim_fifo_ordering() {
    let mut t = SimTransport::default();
    t.open().unwrap();
    t.write(b"AB").unwrap();
    t.write(b"CD").unwrap();
    let mut buf = [0u8; 4];
    t.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"ABCD");
}

#[test]
fn sim_read_exact_timeout_when_not_enough_bytes() {
    let mut t = SimTransport::default();
    t.open().unwrap();
    t.write(b"X").unwrap(); // only 1 byte queued
    let mut buf = [0u8; 4]; // request 4
    let err = t.read_exact(&mut buf).unwrap_err();
    assert!(
        matches!(err, TransportError::Timeout(_)),
        "expected Timeout, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// SimTransport — errors while closed
// ---------------------------------------------------------------------------

#[test]
fn sim_write_while_closed_errors() {
    let mut t = SimTransport::default();
    let err = t.write(b"Q").unwrap_err();
    assert!(
        matches!(err, TransportError::Disconnected),
        "expected Disconnected, got {err:?}"
    );
}

#[test]
fn sim_read_while_closed_errors() {
    let mut t = SimTransport::default();
    let mut buf = [0u8];
    let err = t.read_exact(&mut buf).unwrap_err();
    assert!(
        matches!(err, TransportError::Disconnected),
        "expected Disconnected, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// SimTransport — flush
// ---------------------------------------------------------------------------

#[test]
fn sim_flush_clears_buffer() {
    let mut t = SimTransport::default();
    t.open().unwrap();
    t.write(b"stale").unwrap();
    t.flush().unwrap();
    let mut buf = [0u8; 5];
    let err = t.read_exact(&mut buf).unwrap_err();
    assert!(
        matches!(err, TransportError::Timeout(_)),
        "expected Timeout after flush, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// SimTransport — object safety
// ---------------------------------------------------------------------------

#[test]
fn sim_usable_as_boxed_trait_object() {
    let mut t: Box<dyn Transport> = Box::new(SimTransport::default());
    t.open().unwrap();
    t.write(b"test").unwrap();
    let mut buf = [0u8; 4];
    t.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"test");
    t.close().unwrap();
}

// ---------------------------------------------------------------------------
// Additional type checks
// ---------------------------------------------------------------------------

#[test]
fn port_info_optional_usb_fields_default_to_none() {
    let p = PortInfo {
        name: "COM1".to_string(),
        vid: None,
        pid: None,
        product: None,
    };
    assert!(p.vid.is_none());
    assert!(p.pid.is_none());
    assert!(p.product.is_none());
}

#[test]
fn serial_config_custom_baud() {
    let cfg = SerialConfig {
        baud: 9_600,
        read_timeout: Duration::from_millis(500),
        write_timeout: Duration::from_millis(500),
    };
    assert_eq!(cfg.baud, 9_600);
}
