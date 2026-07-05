// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for M2 page read/write/burn (sub-steps 5.2–5.4).
//!
//! Uses a scripted fake transport (no hardware, no simulator — Task 6 grows
//! `opentune-simulator` to speak the page protocol; until then these tests
//! script `Transport` directly, mirroring `tests/ms_protocol.rs`'s
//! `ScriptedTransport` pattern).
//!
//! Wire semantics ported from Speeduino `comms.cpp` (CRC path) and
//! `comms_legacy.cpp` (plain path) @ `noisymime/speeduino@63fd68e9` (GPL-3),
//! per ADR-0006. Facts below were confirmed by fetching that pinned source
//! directly (not from memory):
//! - `case 'M'`/`case 'b'` (comms.cpp): page id is `serialPayload[2]`
//!   (2nd of its 2 bytes, high byte "always 0" per comms_legacy.cpp);
//!   offset/length via Arduino `word(hi, lo)` with the earlier stream byte
//!   as `lo` → little-endian.
//! - `case 'p'` (comms.cpp): response is `[SERIAL_RC_OK, page bytes...]`.
//! - `case 'p'`/`case 'M'`/`case 'b'` (comms_legacy.cpp): plain-framing reads
//!   return raw bytes with no status prefix; plain-framing writes/burns send
//!   no acknowledgement at all (fire-and-forget).
//! - `sendReturnCodeMsg` (comms.cpp): CRC-framed writes/burns get a 1-byte
//!   ack (`SERIAL_RC_OK` = `0x00`, `SERIAL_RC_BURN_OK` = `0x04`) wrapped in
//!   the standard envelope.

use opentune_ini::{CommsSettings, Endianness, EnvelopeFormat, PageDef};
use opentune_protocol::{crc32_of, MsProtocol, Protocol, ProtocolError};
use opentune_transport::{Result as TResult, Transport, TransportError};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

// ── Test transport ──────────────────────────────────────────────────────

/// A scripted fake transport: captures bytes written and replays a canned
/// response buffer for `read_exact`. `fail_write`/`fail_read` simulate a
/// device vanishing mid-exchange (`TransportError::Disconnected`).
struct ScriptedTransport {
    open: bool,
    response: VecDeque<u8>,
    sent: Vec<u8>,
    fail_write: bool,
    fail_read: bool,
}

impl ScriptedTransport {
    fn with_response(response: impl Into<Vec<u8>>) -> Self {
        Self {
            open: true,
            response: VecDeque::from(response.into()),
            sent: Vec::new(),
            fail_write: false,
            fail_read: false,
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
        if self.fail_write {
            return Err(TransportError::Disconnected);
        }
        self.sent.extend_from_slice(bytes);
        Ok(())
    }
    fn read_exact(&mut self, buf: &mut [u8]) -> TResult<()> {
        if self.fail_read {
            return Err(TransportError::Disconnected);
        }
        if self.response.len() < buf.len() {
            return Err(TransportError::Timeout(Duration::from_millis(100)));
        }
        for slot in buf.iter_mut() {
            *slot = self.response.pop_front().unwrap();
        }
        Ok(())
    }
    fn flush(&mut self) -> TResult<()> {
        // Responses are pre-loaded; a flush must not discard them (matches
        // ms_protocol.rs's ScriptedTransport — `send_page_command` flushes
        // before every command).
        Ok(())
    }
}

// ── Fixtures ─────────────────────────────────────────────────────────────

fn envelope_comms() -> CommsSettings {
    CommsSettings {
        signature: "speeduino 202504-dev".to_string(),
        query_command: "Q".to_string(),
        version_info: "S".to_string(),
        och_get_command: "A".to_string(),
        page_read_command: "p%2i%2o%2c".to_string(),
        page_value_write: "M%2i%2o%2c%v".to_string(),
        burn_command: "b%2i".to_string(),
        blocking_factor: 251,
        page_activation_delay_ms: 0,
        block_read_timeout_ms: 2_000,
        inter_write_delay_ms: 0,
        endianness: Endianness::Little,
        envelope: EnvelopeFormat::MsEnvelope10,
        och_block_size: 0,
    }
}

fn plain_comms() -> CommsSettings {
    CommsSettings {
        envelope: EnvelopeFormat::Plain,
        ..envelope_comms()
    }
}

/// Build an msEnvelope_1.0 frame for an arbitrary payload:
/// `[len_hi, len_lo, payload…, crc32(4 bytes)]`.
fn envelope_frame(payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u16;
    let crc = crc32_of(payload);
    let mut frame = Vec::new();
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(payload);
    frame.extend_from_slice(&crc.to_be_bytes());
    frame
}

// ── 5.2: reads_page ────────────────────────────────────────────────────

#[test]
fn reads_page_envelope_sends_expanded_command_and_strips_status_byte() {
    let comms = envelope_comms();
    let page = PageDef { number: 0, size: 4 };
    // Firmware response: [SERIAL_RC_OK, page bytes...] (comms.cpp `case 'p'`).
    let response = envelope_frame(&[0x00, 0xDE, 0xAD, 0xBE, 0xEF]);
    let transport = ScriptedTransport::with_response(response);
    let mut proto = MsProtocol::new(comms, transport);

    let bytes = proto.read_page(page).unwrap();
    assert_eq!(bytes, vec![0xDE, 0xAD, 0xBE, 0xEF]);

    // "p%2i%2o%2c": 'p' + page 0 (BE [0x00,0x00]) + offset 0 (LE [0x00,0x00])
    // + count 4 (LE [0x04,0x00]).
    let expected_payload = [b'p', 0x00, 0x00, 0x00, 0x00, 0x04, 0x00];
    assert_eq!(
        proto.transport_ref().sent,
        envelope_frame(&expected_payload)
    );
}

#[test]
fn reads_page_plain_returns_raw_bytes_with_no_status_prefix() {
    let comms = plain_comms();
    let page = PageDef { number: 2, size: 3 };
    // Plain framing (comms_legacy.cpp `case 'p'`): raw page bytes only.
    let transport = ScriptedTransport::with_response(vec![0x11, 0x22, 0x33]);
    let mut proto = MsProtocol::new(comms, transport);

    let bytes = proto.read_page(page).unwrap();
    assert_eq!(bytes, vec![0x11, 0x22, 0x33]);
    assert_eq!(
        proto.transport_ref().sent,
        vec![b'p', 0x00, 0x02, 0x00, 0x00, 0x03, 0x00]
    );
}

#[test]
fn reads_page_errors_on_length_mismatch() {
    let comms = envelope_comms();
    let page = PageDef { number: 0, size: 4 };
    // Only 2 data bytes after the status byte, but page.size == 4.
    let response = envelope_frame(&[0x00, 0xAA, 0xBB]);
    let transport = ScriptedTransport::with_response(response);
    let mut proto = MsProtocol::new(comms, transport);

    let err = proto.read_page(page).unwrap_err();
    assert!(matches!(err, ProtocolError::MalformedResponse(_)));
}

// ── 5.3: writes_partial ────────────────────────────────────────────────

#[test]
fn writes_partial_sends_m_page_le_offset_le_length_and_values() {
    let comms = envelope_comms();
    let ack = envelope_frame(&[0x00]); // SERIAL_RC_OK
    let transport = ScriptedTransport::with_response(ack);
    let mut proto = MsProtocol::new(comms, transport);

    proto.write(1, 0x0010, &[0xAA, 0xBB]).unwrap();

    // "M%2i%2o%2c%v": 'M' + page 1 (BE [0x00,0x01]) + offset 0x0010
    // (LE [0x10,0x00]) + count 2 (LE [0x02,0x00]) + value bytes.
    let expected_payload = [b'M', 0x00, 0x01, 0x10, 0x00, 0x02, 0x00, 0xAA, 0xBB];
    assert_eq!(
        proto.transport_ref().sent,
        envelope_frame(&expected_payload)
    );
}

#[test]
fn writes_partial_plain_sends_m_command_with_no_ack_wait() {
    let comms = plain_comms();
    // No response scripted at all — if `write()` tried to read an ack in
    // plain mode, this would time out and the test would fail.
    let transport = ScriptedTransport::with_response(Vec::new());
    let mut proto = MsProtocol::new(comms, transport);

    proto.write(1, 4, &[0x7F]).unwrap();

    assert_eq!(
        proto.transport_ref().sent,
        vec![b'M', 0x00, 0x01, 0x04, 0x00, 0x01, 0x00, 0x7F]
    );
}

#[test]
fn writes_partial_honors_inter_write_and_activation_delays() {
    let comms = CommsSettings {
        inter_write_delay_ms: 5,
        page_activation_delay_ms: 5,
        ..envelope_comms()
    };
    let ack = envelope_frame(&[0x00]);
    let transport = ScriptedTransport::with_response(ack);
    let mut proto = MsProtocol::new(comms, transport);

    let start = Instant::now();
    proto.write(0, 0, &[0x01]).unwrap();
    let elapsed = start.elapsed();

    assert!(
        elapsed >= Duration::from_millis(9),
        "expected >= 9ms combined interWriteDelay + pageActivationDelay, got {elapsed:?}"
    );
}

#[test]
fn writes_partial_reports_disconnected_when_device_vanishes_mid_write() {
    // Device accepts the command bytes but vanishes before the ack arrives —
    // the "mid-write" case the fail-safe requirement targets: bytes may have
    // reached the wire, yet the caller must not treat this as a success.
    let comms = envelope_comms();
    let mut transport = ScriptedTransport::with_response(Vec::new());
    transport.fail_read = true;
    let mut proto = MsProtocol::new(comms, transport);

    let err = proto.write(0, 0, &[0x01]).unwrap_err();
    assert!(matches!(
        err,
        ProtocolError::Transport(TransportError::Disconnected)
    ));
}

#[test]
fn writes_partial_reports_disconnected_when_device_already_gone() {
    let comms = envelope_comms();
    let mut transport = ScriptedTransport::with_response(Vec::new());
    transport.fail_write = true;
    let mut proto = MsProtocol::new(comms, transport);

    let err = proto.write(0, 0, &[0x01]).unwrap_err();
    assert!(matches!(
        err,
        ProtocolError::Transport(TransportError::Disconnected)
    ));
}

// ── 5.4: burns_page ────────────────────────────────────────────────────

#[test]
fn burns_page_envelope_sends_b_page_and_reads_ack() {
    let comms = envelope_comms();
    let ack = envelope_frame(&[0x04]); // SERIAL_RC_BURN_OK
    let transport = ScriptedTransport::with_response(ack);
    let mut proto = MsProtocol::new(comms, transport);

    proto.burn(3).unwrap();

    // "b%2i": 'b' + page 3 (BE [0x00,0x03]).
    let expected_payload = [b'b', 0x00, 0x03];
    assert_eq!(
        proto.transport_ref().sent,
        envelope_frame(&expected_payload)
    );
}

#[test]
fn burns_page_plain_sends_b_command_with_no_ack_wait() {
    let comms = plain_comms();
    let transport = ScriptedTransport::with_response(Vec::new());
    let mut proto = MsProtocol::new(comms, transport);

    proto.burn(5).unwrap();

    assert_eq!(proto.transport_ref().sent, vec![b'b', 0x00, 0x05]);
}

#[test]
fn burns_page_reports_disconnected_when_device_vanishes() {
    let comms = envelope_comms();
    let mut transport = ScriptedTransport::with_response(Vec::new());
    transport.fail_read = true;
    let mut proto = MsProtocol::new(comms, transport);

    let err = proto.burn(3).unwrap_err();
    assert!(matches!(
        err,
        ProtocolError::Transport(TransportError::Disconnected)
    ));
}
