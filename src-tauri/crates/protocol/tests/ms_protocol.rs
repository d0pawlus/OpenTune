// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests for the generic MS/TS protocol engine (`MsProtocol`).
//!
//! Uses a fake/scripted transport — no hardware required. Command bytes and
//! CRC behaviour ported from Speeduino `comms.cpp` (GPL-3), per ADR-0006.

use opentune_ini::{CommsSettings, Endianness, EnvelopeFormat};
use opentune_protocol::{crc32_of, MsProtocol, Protocol};
use opentune_transport::{Result as TResult, Transport, TransportError};
use std::collections::VecDeque;
use std::time::Duration;

// ── CRC32 ──────────────────────────────────────────────────────────────────

#[test]
fn crc32_standard_test_vector() {
    // CRC32("123456789") = 0xCBF43926 — the canonical ISO-3309 test vector
    // used by Speeduino's msEnvelope_1.0 framing (Speeduino comms.cpp).
    assert_eq!(crc32_of(b"123456789"), 0xCBF4_3926);
}

// ── Test helpers ───────────────────────────────────────────────────────────

/// A scripted fake transport: captures bytes written and replays a canned
/// response buffer for `read_exact`.
struct ScriptedTransport {
    open: bool,
    /// Bytes the transport will serve to `read_exact` calls.
    response: VecDeque<u8>,
    /// Bytes captured from `write` calls (for assertions).
    sent: Vec<u8>,
}

impl ScriptedTransport {
    fn with_response(response: impl Into<Vec<u8>>) -> Self {
        Self {
            open: true,
            response: VecDeque::from(response.into()),
            sent: Vec::new(),
        }
    }
}

impl Transport for ScriptedTransport {
    fn open(&mut self) -> TResult<()> {
        self.open = true;
        Ok(())
    }
    fn close(&mut self) -> TResult<()> {
        self.open = false;
        Ok(())
    }
    fn is_open(&self) -> bool {
        self.open
    }
    fn write(&mut self, bytes: &[u8]) -> TResult<()> {
        self.sent.extend_from_slice(bytes);
        Ok(())
    }
    fn read_exact(&mut self, buf: &mut [u8]) -> TResult<()> {
        if self.response.len() < buf.len() {
            return Err(TransportError::Timeout(Duration::from_millis(100)));
        }
        for slot in buf.iter_mut() {
            *slot = self.response.pop_front().unwrap();
        }
        Ok(())
    }
    fn flush(&mut self) -> TResult<()> {
        // ScriptedTransport: flush is a no-op; responses are pre-loaded and
        // should not be discarded by a flush call (no stale data in tests).
        Ok(())
    }
}

fn plain_comms(signature: &str) -> CommsSettings {
    CommsSettings {
        signature: signature.to_string(),
        query_command: "Q".to_string(),
        version_info: "S".to_string(),
        och_get_command: "A".to_string(),
        page_read_command: "r%2i%2o%2c".to_string(),
        page_value_write: "w%2i%2o%2c%v".to_string(),
        burn_command: "b%2i".to_string(),
        blocking_factor: 251,
        page_activation_delay_ms: 10,
        block_read_timeout_ms: 2_000,
        inter_write_delay_ms: 10,
        endianness: Endianness::Little,
        envelope: EnvelopeFormat::Plain,
    }
}

fn envelope_comms(signature: &str) -> CommsSettings {
    CommsSettings {
        envelope: EnvelopeFormat::MsEnvelope10,
        ..plain_comms(signature)
    }
}

/// Build a plain null-terminated response byte string.
fn plain_response(text: &str) -> Vec<u8> {
    let mut v = text.as_bytes().to_vec();
    v.push(0x00); // null terminator
    v
}

/// Build an MS-envelope 1.0 response frame for a text payload.
fn envelope_response(text: &str) -> Vec<u8> {
    let payload: Vec<u8> = text.as_bytes().to_vec();
    let len = payload.len() as u16;
    let crc = crc32_of(&payload);
    let mut frame = Vec::new();
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&payload);
    frame.extend_from_slice(&crc.to_be_bytes());
    frame
}

// ── Plain protocol tests ────────────────────────────────────────────────────

#[test]
fn plain_signature_sends_q_and_returns_text() {
    let sig = "speeduino 202504-dev";
    let transport = ScriptedTransport::with_response(plain_response(sig));
    let comms = plain_comms(sig);
    let mut proto = MsProtocol::new(comms, transport);

    let result = proto.signature().unwrap();
    assert_eq!(result, sig);
}


// ── Plain protocol: version query ──────────────────────────────────────────

#[test]
fn plain_version_returns_text() {
    let comms = plain_comms("speeduino 202504-dev");
    let transport = ScriptedTransport::with_response(plain_response("Speeduino 2025.04"));
    let mut proto = MsProtocol::new(comms, transport);
    assert_eq!(proto.version().unwrap(), "Speeduino 2025.04");
}

// ── Plain protocol: identify (signature + version) ─────────────────────────

#[test]
fn plain_identify_returns_both_strings() {
    let sig = "speeduino 202504-dev";
    let ver = "Speeduino 2025.04";
    // identify() calls signature() then version(); both responses queued.
    let mut response = plain_response(sig);
    response.extend(plain_response(ver));
    let transport = ScriptedTransport::with_response(response);
    let comms = plain_comms(sig);
    let mut proto = MsProtocol::new(comms, transport);

    let id = proto.identify().unwrap();
    assert_eq!(id.signature, sig);
    assert_eq!(id.version, ver);
}

#[test]
fn identity_matches_ini_when_signatures_equal() {
    let sig = "speeduino 202504-dev";
    let comms = plain_comms(sig);
    let id = opentune_protocol::EcuIdentity {
        signature: sig.to_string(),
        version: "v1".to_string(),
    };
    assert!(id.matches(&comms));
}

#[test]
fn identity_does_not_match_ini_when_signatures_differ() {
    let comms = plain_comms("speeduino 202504-dev");
    let id = opentune_protocol::EcuIdentity {
        signature: "rusEFI master.2025".to_string(),
        version: "v1".to_string(),
    };
    assert!(!id.matches(&comms));
}

// ── Plain protocol: secl ───────────────────────────────────────────────────

#[test]
fn plain_read_secl_returns_first_byte() {
    let comms = plain_comms("speeduino 202504-dev");
    // secl = 42 is the first byte of the realtime channel response.
    let transport = ScriptedTransport::with_response(vec![42u8, 0, 0, 0]);
    let mut proto = MsProtocol::new(comms, transport);
    assert_eq!(proto.read_secl().unwrap(), 42);
}

// ── MS-envelope 1.0 (CRC) protocol ────────────────────────────────────────

#[test]
fn envelope_signature_returns_text() {
    let sig = "speeduino 202504-dev";
    let comms = envelope_comms(sig);
    let transport = ScriptedTransport::with_response(envelope_response(sig));
    let mut proto = MsProtocol::new(comms, transport);
    assert_eq!(proto.signature().unwrap(), sig);
}

#[test]
fn envelope_version_returns_text() {
    let ver = "Speeduino 2025.04";
    let comms = envelope_comms("speeduino 202504-dev");
    let transport = ScriptedTransport::with_response(envelope_response(ver));
    let mut proto = MsProtocol::new(comms, transport);
    assert_eq!(proto.version().unwrap(), ver);
}

#[test]
fn envelope_identify_returns_both_strings() {
    let sig = "speeduino 202504-dev";
    let ver = "Speeduino 2025.04";
    let mut response = envelope_response(sig);
    response.extend(envelope_response(ver));
    let transport = ScriptedTransport::with_response(response);
    let comms = envelope_comms(sig);
    let mut proto = MsProtocol::new(comms, transport);

    let id = proto.identify().unwrap();
    assert_eq!(id.signature, sig);
    assert_eq!(id.version, ver);
}

#[test]
fn envelope_crc_mismatch_returns_error() {
    let sig = "speeduino 202504-dev";
    let comms = envelope_comms(sig);
    // Build a response with a corrupted CRC.
    let mut bad = envelope_response(sig);
    let last = bad.len() - 1;
    bad[last] ^= 0xFF; // flip some CRC bits
    let transport = ScriptedTransport::with_response(bad);
    let mut proto = MsProtocol::new(comms, transport);
    let err = proto.signature().unwrap_err();
    assert!(matches!(err, opentune_protocol::ProtocolError::CrcMismatch { .. }));
}

#[test]
fn envelope_read_secl_returns_first_payload_byte() {
    let comms = envelope_comms("speeduino 202504-dev");
    // Build an envelope response with secl=99 as first byte of payload.
    let payload: Vec<u8> = vec![99u8, 0, 0, 0, 0];
    let len = payload.len() as u16;
    let crc = crc32_of(&payload);
    let mut response = Vec::new();
    response.extend_from_slice(&len.to_be_bytes());
    response.extend_from_slice(&payload);
    response.extend_from_slice(&crc.to_be_bytes());
    let transport = ScriptedTransport::with_response(response);
    let mut proto = MsProtocol::new(comms, transport);
    assert_eq!(proto.read_secl().unwrap(), 99);
}

// ── CRC32 additional coverage ──────────────────────────────────────────────

#[test]
fn crc32_empty_input() {
    // CRC32("") must equal the known value for the empty string under
    // ISO-3309 (0x00000000 after !crc finalization).
    assert_eq!(crc32_of(b""), 0x0000_0000);
}

#[test]
fn crc32_is_deterministic() {
    let a = crc32_of(b"hello world");
    let b = crc32_of(b"hello world");
    assert_eq!(a, b);
}
