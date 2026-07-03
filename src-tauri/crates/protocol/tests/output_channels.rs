// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for M3 Task 4: `read_output_channels(offset, len)`.
//!
//! Scripted fake transport (mirrors `tests/pages.rs`'s `ScriptedTransport`) —
//! no hardware/simulator dependency, so these tests are independent of
//! Task 5's simulator `'r'` arm.
//!
//! Wire semantics ported from Speeduino `comms.cpp` @ `noisymime/speeduino@63fd68e9`
//! (GPL-3), per ADR-0006:
//! - `case 'r'` (comms.cpp:359-374): response payload is
//!   `[0]=0x00 SERIAL_RC_OK` followed by the requested block bytes.
//! - Request bytes: `ochGetCommand` template `"r$tsCanId\x30%2o%2c"` expands to
//!   `['r', tsCanId(0), 0x30, offset_LE(2), len_LE(2)]` (see
//!   `pages.rs::expand_template`'s `expands_tscanid_and_hex_literal` unit test).

use opentune_ini::{CommsSettings, Endianness, EnvelopeFormat};
use opentune_protocol::{crc32_of, MsProtocol, Protocol, ProtocolError};
use opentune_transport::{Result as TResult, Transport, TransportError};
use std::collections::VecDeque;

// ── Test transport (mirrors tests/pages.rs) ─────────────────────────────

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
            return Err(TransportError::Timeout(std::time::Duration::from_millis(
                100,
            )));
        }
        for slot in buf.iter_mut() {
            *slot = self.response.pop_front().unwrap();
        }
        Ok(())
    }
    fn flush(&mut self) -> TResult<()> {
        Ok(())
    }
}

// ── Fixtures ─────────────────────────────────────────────────────────────

fn envelope_comms() -> CommsSettings {
    CommsSettings {
        signature: "speeduino 202504-dev".to_string(),
        query_command: "Q".to_string(),
        version_info: "S".to_string(),
        och_get_command: "r$tsCanId\\x30%2o%2c".to_string(),
        page_read_command: "p%2i%2o%2c".to_string(),
        page_value_write: "M%2i%2o%2c%v".to_string(),
        burn_command: "b%2i".to_string(),
        blocking_factor: 251,
        page_activation_delay_ms: 0,
        block_read_timeout_ms: 2_000,
        inter_write_delay_ms: 0,
        endianness: Endianness::Little,
        envelope: EnvelopeFormat::MsEnvelope10,
        och_block_size: 139,
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

// ── 4.2: reads_full_och_frame ────────────────────────────────────────────

#[test]
fn reads_full_och_frame() {
    let comms = envelope_comms();
    let block: Vec<u8> = (0u8..16).collect(); // b0..b15
    let mut response_payload = vec![0x00]; // SERIAL_RC_OK
    response_payload.extend_from_slice(&block);
    let response = envelope_frame(&response_payload);
    let transport = ScriptedTransport::with_response(response);
    let mut proto = MsProtocol::new(comms, transport);

    let bytes = proto.read_output_channels(0, 16).unwrap();
    assert_eq!(bytes, block);

    // "r$tsCanId\x30%2o%2c": 'r' + tsCanId(0x00) + 0x30 + offset 0 (LE
    // [0x00,0x00]) + len 16 (LE [0x10,0x00]).
    let expected_payload = [b'r', 0x00, 0x30, 0x00, 0x00, 0x10, 0x00];
    assert_eq!(
        proto.transport_ref().sent,
        envelope_frame(&expected_payload)
    );
}

#[test]
fn reads_full_och_frame_plain_returns_raw_bytes() {
    let comms = plain_comms();
    let block: Vec<u8> = (0u8..16).collect();
    let transport = ScriptedTransport::with_response(block.clone());
    let mut proto = MsProtocol::new(comms, transport);

    let bytes = proto.read_output_channels(0, 16).unwrap();
    assert_eq!(bytes, block);
    assert_eq!(
        proto.transport_ref().sent,
        vec![b'r', 0x00, 0x30, 0x00, 0x00, 0x10, 0x00]
    );
}

#[test]
fn reads_och_frame_at_nonzero_offset() {
    let comms = envelope_comms();
    let block = vec![0xAA, 0xBB, 0xCC];
    let mut response_payload = vec![0x00];
    response_payload.extend_from_slice(&block);
    let response = envelope_frame(&response_payload);
    let transport = ScriptedTransport::with_response(response);
    let mut proto = MsProtocol::new(comms, transport);

    let bytes = proto.read_output_channels(0x0014, 3).unwrap();
    assert_eq!(bytes, block);

    // offset 20 (LE [0x14,0x00]), len 3 (LE [0x03,0x00]).
    let expected_payload = [b'r', 0x00, 0x30, 0x14, 0x00, 0x03, 0x00];
    assert_eq!(
        proto.transport_ref().sent,
        envelope_frame(&expected_payload)
    );
}

// ── Short-frame tolerance ────────────────────────────────────────────────
//
// `ochBlockSize` declared by the INI (139) can disagree with what the
// firmware actually sends (138) — never trust the declared/requested length
// over the bytes actually received. A short frame is not an error: return
// whatever arrived rather than indexing past it.

#[test]
fn tolerates_short_envelope_frame_shorter_than_requested_len() {
    let comms = envelope_comms();
    // Requested len=139 (the real ochBlockSize) but firmware only sends 10
    // data bytes after the status byte — simulates the declared-vs-actual
    // ochBlockSize mismatch (INI says 139, firmware frame is 138 in the
    // research trap; exaggerated here to 10 for a clear assertion).
    let block: Vec<u8> = (0u8..10).collect();
    let mut response_payload = vec![0x00];
    response_payload.extend_from_slice(&block);
    let response = envelope_frame(&response_payload);
    let transport = ScriptedTransport::with_response(response);
    let mut proto = MsProtocol::new(comms, transport);

    let bytes = proto.read_output_channels(0, 139).unwrap();
    assert_eq!(bytes, block);

    // len=139 -> 0x008B LE -> [0x8B, 0x00]; exercises a high-bit-set low byte.
    let expected_payload = [b'r', 0x00, 0x30, 0x00, 0x00, 0x8B, 0x00];
    assert_eq!(
        proto.transport_ref().sent,
        envelope_frame(&expected_payload)
    );
}

#[test]
fn plain_short_reply_errors_no_length_prefix_to_trust() {
    // Plain framing carries no length prefix, so there is nothing to
    // disbelieve the way the envelope path disbelieves `len`/`ochBlockSize`:
    // the only contract available is "read exactly `len` bytes". A reply
    // shorter than requested surfaces as a transport timeout, not a silent
    // truncation — this is the Plain-side counterpart to the envelope
    // short-frame tolerance above, not another instance of it.
    let comms = plain_comms();
    let transport = ScriptedTransport::with_response(vec![0x11, 0x22, 0x33]); // only 3 of 5
    let mut proto = MsProtocol::new(comms, transport);

    let err = proto.read_output_channels(0, 5).unwrap_err();
    assert!(matches!(
        err,
        ProtocolError::Transport(TransportError::Timeout(_))
    ));
}

#[test]
fn reports_disconnected_when_device_vanishes_mid_read() {
    let comms = envelope_comms();
    let mut transport = ScriptedTransport::with_response(Vec::new());
    transport.fail_read = true;
    let mut proto = MsProtocol::new(comms, transport);

    let err = proto.read_output_channels(0, 16).unwrap_err();
    assert!(matches!(
        err,
        ProtocolError::Transport(TransportError::Disconnected)
    ));
}

#[test]
fn reports_disconnected_when_device_already_gone_before_read() {
    let comms = envelope_comms();
    let mut transport = ScriptedTransport::with_response(Vec::new());
    transport.fail_write = true;
    let mut proto = MsProtocol::new(comms, transport);

    let err = proto.read_output_channels(0, 16).unwrap_err();
    assert!(matches!(
        err,
        ProtocolError::Transport(TransportError::Disconnected)
    ));
}
