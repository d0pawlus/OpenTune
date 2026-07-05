// SPDX-License-Identifier: GPL-3.0-or-later
//! Generic MS/TS protocol engine — `MsProtocol` and CRC32 helper.
//!
//! Wire format ported from Speeduino `comms.cpp` and rusEFI `tunerstudio.cpp`
//! (both GPL-3), per [ADR-0006](../../../../docs/adr/0006-reuse-existing-parsers.md).
//!
//! # Plain protocol (legacy / unframed)
//!
//! - Command: single ASCII byte (e.g. `b'Q'`).
//! - Response: null-terminated ASCII string; read byte-by-byte until `0x00`.
//!
//! # MS-envelope 1.0 (CRC protocol)
//!
//! Both commands **and** responses use the same symmetric frame:
//!   `[len_hi, len_lo, ...payload..., crc_b3, crc_b2, crc_b1, crc_b0]`
//!
//! The 2-byte big-endian length covers payload bytes only. The 4-byte CRC32
//! (ISO-3309, same as Ethernet/zlib) covers the payload bytes only.
//! Source: Speeduino `comms.cpp` / rusEFI `tunerstudio.cpp` (ADR-0006).

use crate::{EcuIdentity, Protocol, ProtocolError, Result};
use opentune_ini::{CommsSettings, EnvelopeFormat, PageDef};
use opentune_transport::Transport;

/// Maximum bytes to read for a null-terminated plain-protocol response.
/// Speeduino signatures are at most 32 bytes; 128 is a generous safety bound.
const MAX_PLAIN_RESPONSE: usize = 128;

/// Generic MS/TS protocol engine parameterised by an INI [`CommsSettings`]
/// and any [`Transport`] implementation.
pub struct MsProtocol<T: Transport> {
    /// Crate-visible (not just module-private) so [`crate::pages`]'s page
    /// read/write/burn methods — a separate inherent `impl` block in a
    /// different file — can read the command templates and delays without
    /// duplicating them.
    pub(crate) comms: CommsSettings,
    transport: T,
}

impl<T: Transport> MsProtocol<T> {
    /// Create a new engine. The transport should already be open before any
    /// protocol operation is called.
    pub fn new(comms: CommsSettings, transport: T) -> Self {
        Self { comms, transport }
    }

    /// Borrow the underlying transport (useful for test introspection).
    pub fn transport_ref(&self) -> &T {
        &self.transport
    }

    // ── Dispatch ────────────────────────────────────────────────────────────

    /// Send one-byte command and receive the response, dispatching on
    /// [`CommsSettings::envelope`].
    fn send_query(&mut self, cmd: u8) -> Result<String> {
        // Discard any stale bytes from a prior failed exchange.
        self.transport.flush()?;
        match self.comms.envelope {
            EnvelopeFormat::Plain => self.plain_send_recv(cmd),
            EnvelopeFormat::MsEnvelope10 => self.envelope_send_recv(cmd),
        }
    }

    // ── Plain (legacy) helpers ──────────────────────────────────────────────

    fn plain_send_recv(&mut self, cmd: u8) -> Result<String> {
        self.transport.write(&[cmd])?;
        self.plain_read_string()
    }

    fn plain_read_string(&mut self) -> Result<String> {
        let mut buf = Vec::with_capacity(32);
        let mut byte = [0u8; 1];
        loop {
            self.transport.read_exact(&mut byte)?;
            if byte[0] == 0x00 {
                break;
            }
            buf.push(byte[0]);
            if buf.len() >= MAX_PLAIN_RESPONSE {
                return Err(ProtocolError::MalformedResponse(format!(
                    "response exceeded {MAX_PLAIN_RESPONSE} bytes without NUL terminator"
                )));
            }
        }
        String::from_utf8(buf)
            .map_err(|e| ProtocolError::MalformedResponse(format!("non-UTF-8 response: {e}")))
    }

    // ── MS-envelope 1.0 (CRC) helpers ──────────────────────────────────────

    fn envelope_send_recv(&mut self, cmd: u8) -> Result<String> {
        self.envelope_write(&[cmd])?;
        self.envelope_read_string()
    }

    /// Write a symmetric msEnvelope_1.0 command frame:
    /// `[len_hi, len_lo, ...payload..., crc_b3, crc_b2, crc_b1, crc_b0]`
    ///
    /// Both command and response use the same framing format (symmetric). The
    /// 2-byte big-endian length covers payload bytes only; the 4-byte CRC32
    /// covers the payload bytes only. Ported from Speeduino `comms.cpp` and
    /// rusEFI `tunerstudio.cpp` (ADR-0006).
    pub(crate) fn envelope_write(&mut self, payload: &[u8]) -> Result<()> {
        let len = payload.len() as u16;
        let crc = crc32_of(payload);
        let mut frame = Vec::with_capacity(2 + payload.len() + 4);
        frame.extend_from_slice(&len.to_be_bytes());
        frame.extend_from_slice(payload);
        frame.extend_from_slice(&crc.to_be_bytes());
        self.transport.write(&frame)?;
        Ok(())
    }

    /// Read `[len_hi, len_lo, payload..., crc32(4 bytes)]`, verify CRC,
    /// and return the payload as a UTF-8 string (trailing NUL stripped).
    fn envelope_read_string(&mut self) -> Result<String> {
        let payload = self.envelope_read_bytes(MAX_PLAIN_RESPONSE)?;
        let text_bytes = payload.strip_suffix(b"\x00").unwrap_or(&payload);
        String::from_utf8(text_bytes.to_vec())
            .map_err(|e| ProtocolError::MalformedResponse(format!("non-UTF-8 payload: {e}")))
    }

    /// Read `[len_hi, len_lo, payload..., crc32(4 bytes)]`, verify the CRC,
    /// and return the raw payload bytes — no UTF-8/NUL handling, unlike
    /// [`Self::envelope_read_string`]. Shared by the string-response callers
    /// above and the binary page-read/write-ack path in [`crate::pages`]
    /// (page data is not necessarily valid UTF-8). `max_len` bounds the
    /// accepted `payload_len` as a sanity guard against a corrupted length
    /// prefix causing a huge allocation.
    pub(crate) fn envelope_read_bytes(&mut self, max_len: usize) -> Result<Vec<u8>> {
        let mut len_buf = [0u8; 2];
        self.transport.read_exact(&mut len_buf)?;
        let payload_len = u16::from_be_bytes(len_buf) as usize;
        if payload_len > max_len {
            return Err(ProtocolError::MalformedResponse(format!(
                "envelope payload length {payload_len} exceeds limit {max_len}"
            )));
        }
        let mut payload = vec![0u8; payload_len];
        self.transport.read_exact(&mut payload)?;
        let mut crc_buf = [0u8; 4];
        self.transport.read_exact(&mut crc_buf)?;
        let received_crc = u32::from_be_bytes(crc_buf);
        let computed_crc = crc32_of(&payload);
        if received_crc != computed_crc {
            return Err(ProtocolError::CrcMismatch {
                expected: computed_crc,
                actual: received_crc,
            });
        }
        Ok(payload)
    }

    /// Send an already-expanded page-domain command (`pageReadCommand`,
    /// `pageValueWrite`, `burnCommand`) and return its response bytes,
    /// dispatching on [`CommsSettings::envelope`] like [`Self::send_query`]
    /// but working with raw bytes rather than a NUL-terminated string.
    ///
    /// - **Plain**: writes `payload`, then reads exactly `plain_response_len`
    ///   raw bytes. Pass `0` for write/burn — Speeduino's legacy handler
    ///   (`comms_legacy.cpp` `'M'`/`'b'`) sends no acknowledgement in this
    ///   framing.
    /// - **`MsEnvelope10`**: every command gets a CRC-verified,
    ///   self-describing envelope response (`comms.cpp` acks writes/burns
    ///   with a 1-byte return code via `sendReturnCodeMsg`); `envelope_max_len`
    ///   bounds the accepted response size.
    ///
    /// Any transport failure (including
    /// [`opentune_transport::TransportError::Disconnected`] if the device
    /// vanishes between the write and the ack read) propagates as `Err`.
    pub(crate) fn send_page_command(
        &mut self,
        payload: &[u8],
        plain_response_len: usize,
        envelope_max_len: usize,
    ) -> Result<Vec<u8>> {
        self.transport.flush()?;
        match self.comms.envelope {
            EnvelopeFormat::Plain => {
                self.transport.write(payload)?;
                if plain_response_len == 0 {
                    return Ok(Vec::new());
                }
                let mut buf = vec![0u8; plain_response_len];
                self.transport.read_exact(&mut buf)?;
                Ok(buf)
            }
            EnvelopeFormat::MsEnvelope10 => {
                self.envelope_write(payload)?;
                self.envelope_read_bytes(envelope_max_len)
            }
        }
    }
}

impl<T: Transport> Protocol for MsProtocol<T> {
    fn identify(&mut self) -> Result<EcuIdentity> {
        let signature = self.signature()?;
        let version = self.version()?;
        Ok(EcuIdentity { signature, version })
    }

    fn signature(&mut self) -> Result<String> {
        let cmd = self
            .comms
            .query_command
            .as_bytes()
            .first()
            .copied()
            .ok_or_else(|| ProtocolError::MalformedResponse("queryCommand is empty".to_string()))?;
        self.send_query(cmd)
    }

    fn version(&mut self) -> Result<String> {
        let cmd = self
            .comms
            .version_info
            .as_bytes()
            .first()
            .copied()
            .ok_or_else(|| ProtocolError::MalformedResponse("versionInfo is empty".to_string()))?;
        self.send_query(cmd)
    }

    /// Read the ECU's `secl` (second counter) for reconnect-detection.
    ///
    /// `secl` is byte 0 of the output-channel block in both Speeduino's and
    /// rusEFI's layout. Two `ochGetCommand` shapes exist:
    ///
    /// - **Single-byte command** (M1 fixtures / bundled INI, e.g. `"A"`):
    ///   the response carries `secl` at byte 0 with *no* status prefix in
    ///   either framing — the original M1 path, preserved verbatim below.
    /// - **Windowed template** (real INIs post-`[OutputChannels]` lift, e.g.
    ///   `r\$tsCanId\x30%2o%2c`): sending only the first byte would leave
    ///   the firmware waiting for the rest of the request (Plain: timeout)
    ///   or return a status-only envelope whose byte 0 is `SERIAL_RC_OK`,
    ///   not `secl` (M3 Task 6 blocker c — a constant-zero secl silently
    ///   breaks reboot detection). Such templates are expanded as a proper
    ///   1-byte window at offset 0 via `do_read_output_channels`, which
    ///   also strips the envelope status byte correctly.
    fn read_secl(&mut self) -> Result<u8> {
        let template = self.comms.och_get_command.clone();
        let params = crate::TemplateParams {
            page: 0,
            offset: 0,
            count: 1,
            value: &[],
            can_id: 0,
        };
        let command = crate::expand_template(&template, &params)?;
        let Some((&cmd, rest)) = command.split_first() else {
            return Err(ProtocolError::MalformedResponse(
                "ochGetCommand is empty".to_string(),
            ));
        };
        if !rest.is_empty() {
            // Windowed template: secl is byte 0 of a 1-byte och window.
            let block = self.do_read_output_channels(0, 1)?;
            return block.first().copied().ok_or_else(|| {
                ProtocolError::MalformedResponse(
                    "empty output-channels window for secl".to_string(),
                )
            });
        }
        self.transport.flush()?;
        match self.comms.envelope {
            EnvelopeFormat::Plain => {
                self.transport.write(&[cmd])?;
                let mut buf = [0u8; 1];
                self.transport.read_exact(&mut buf)?;
                // Flush remaining channel bytes — M1 only needs byte 0.
                self.transport.flush()?;
                Ok(buf[0])
            }
            EnvelopeFormat::MsEnvelope10 => {
                self.envelope_write(&[cmd])?;
                let mut len_buf = [0u8; 2];
                self.transport.read_exact(&mut len_buf)?;
                let payload_len = u16::from_be_bytes(len_buf) as usize;
                if payload_len == 0 {
                    return Err(ProtocolError::MalformedResponse(
                        "empty realtime payload".to_string(),
                    ));
                }
                let mut payload = vec![0u8; payload_len];
                self.transport.read_exact(&mut payload)?;
                // CRC bytes — read and discard for M1.
                let mut _crc = [0u8; 4];
                self.transport.read_exact(&mut _crc)?;
                Ok(payload[0])
            }
        }
    }

    /// Delegates to [`MsProtocol::do_read_page`] (defined in
    /// [`crate::pages`] — see there for the comms.cpp/comms_legacy.cpp
    /// framing citations).
    fn read_page(&mut self, page: PageDef) -> Result<Vec<u8>> {
        self.do_read_page(page)
    }

    /// Delegates to [`MsProtocol::do_write`] (defined in [`crate::pages`]).
    fn write(&mut self, page: u16, offset: u16, bytes: &[u8]) -> Result<()> {
        self.do_write(page, offset, bytes)
    }

    /// Delegates to [`MsProtocol::do_burn`] (defined in [`crate::pages`]).
    fn burn(&mut self, page: u16) -> Result<()> {
        self.do_burn(page)
    }

    /// Delegates to [`MsProtocol::do_read_output_channels`] (defined in
    /// [`crate::pages`]).
    fn read_output_channels(&mut self, offset: u16, len: u16) -> Result<Vec<u8>> {
        self.do_read_output_channels(offset, len)
    }
}

// ---------------------------------------------------------------------------
// CRC32 (ISO-3309 / ITU-T V.42 / Ethernet / zlib polynomial)
//
// Ported from Speeduino comms.cpp (GPL-3, ADR-0006).
// Reflected polynomial: 0xEDB88320 (reverse of 0x04C11DB7).
// ---------------------------------------------------------------------------

/// Compute the CRC32 checksum of `data` using the standard ISO-3309 polynomial.
///
/// This matches Speeduino's `msEnvelope_1.0` CRC computation — same polynomial
/// as Ethernet and zlib. The standard test vector `CRC32("123456789")` must
/// equal `0xCBF43926`.
pub fn crc32_of(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}
