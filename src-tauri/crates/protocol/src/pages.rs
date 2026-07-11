// SPDX-License-Identifier: GPL-3.0-or-later
//! Page domain: the comms-template expander (`%2i/%2o/%2c/%v/$tsCanId/\xNN`)
//! and `read_page`/`write`/`burn` for the generic MS/TS engine
//! ([`MsProtocol`]).
//!
//! Wire semantics ported from Speeduino `comms.cpp` (CRC path) and
//! `comms_legacy.cpp` (plain path) @ `noisymime/speeduino@63fd68e9` (GPL-3),
//! per [ADR-0006](../../../../docs/adr/0006-reuse-existing-parsers.md).
//! Confirmed directly against that pinned source (not re-derived from
//! memory) — see the doc comments on [`expand_template`] and each
//! `MsProtocol` page method for the specific lines cited.

use crate::{MsProtocol, ProtocolError, Result};
use opentune_ini::{EnvelopeFormat, PageDef};
use opentune_transport::Transport;
use std::time::Duration;

/// Generous sanity ceiling for a page-domain envelope response (a 1-byte
/// write/burn ack, or a status byte + page bytes). Real Speeduino/rusEFI
/// pages are well under 1 KiB; this only guards against a corrupted length
/// prefix causing a huge allocation — the analogous bound for
/// identify/version responses is `MAX_PLAIN_RESPONSE` in `engine.rs`.
const MAX_PAGE_RESPONSE: usize = 8_192;

/// Placeholders available when expanding a comms command template
/// (`pageReadCommand`, `pageValueWrite`, `burnCommand`, `ochGetCommand`, …).
/// A given template only consults the fields its placeholders reference.
#[derive(Debug, Clone, Copy)]
pub struct TemplateParams<'a> {
    /// `%2i` — the page/index id.
    pub page: u16,
    /// `%2o` — byte offset into the page.
    pub offset: u16,
    /// `%2c` — count/length in bytes.
    pub count: u16,
    /// `%v` — raw value bytes, appended verbatim (writes only).
    pub value: &'a [u8],
    /// `$tsCanId` — CAN bus device id, substituted as a single raw byte.
    /// `CommsSettings` has no such field (M1-frozen shape) and Speeduino's
    /// legacy `'r'` handler discards the byte unconditionally in this build,
    /// so callers in this crate always pass `0`.
    pub can_id: u8,
}

/// Expand a comms-settings command template into wire bytes.
///
/// Supports the MS/TS-family placeholders used across `pageReadCommand`,
/// `pageValueWrite`, `burnCommand`, and `ochGetCommand`:
///
/// - `%2i` — 2 bytes, **big-endian**. Firmware only reads the low (2nd)
///   byte as the page id and ignores the high (1st) byte.
/// - `%2o` — 2 bytes, little-endian offset.
/// - `%2c` — 2 bytes, little-endian count/length.
/// - `%v`  — `value.len()` raw bytes, appended verbatim.
/// - `$tsCanId` — 1 raw byte (`TemplateParams::can_id`).
/// - `\xNN` — 1 literal hex byte.
/// - any other character passes through as its ASCII byte value.
///
/// Source: Speeduino `comms.cpp`/`comms_legacy.cpp` @ `63fd68e9` (GPL-3):
/// - `case 'M'` (comms.cpp): `updatePageValues(serialPayload[2],
///   word(serialPayload[4], serialPayload[3]), &serialPayload[7],
///   word(serialPayload[6], serialPayload[5]))` — page id is
///   `serialPayload[2]` (2nd of its 2 bytes); Arduino `word(hi, lo)` with the
///   *earlier* stream byte as `lo` gives little-endian offset/length.
/// - `case 'M'` (comms_legacy.cpp): "First byte of the page identifier can
///   be ignored. It's always 0" — confirms the high byte of `%2i` is `0x00`.
/// - `case 'r'` (comms_legacy.cpp): `targetPort.read(); //Read the $tsCanId`
///   — a single raw byte, unconditionally discarded by this firmware build.
///
/// Scans `template.as_bytes()` end-to-end — never re-slices the source `&str`
/// at an arbitrary byte offset. Command templates are ASCII wire-protocol
/// strings (see the placeholder list above); a byte `>= 0x80` can only occur
/// as part of a stray multi-byte UTF-8 character (e.g. a mis-pasted INI), so
/// it is rejected as [`ProtocolError::MalformedTemplate`] rather than passed
/// through or silently re-encoded. The old implementation sliced `&str`
/// bytewise (`&template[i..]`), which panicked whenever a scan offset landed
/// mid-char — a poisoned-mutex risk since this runs under the session lock.
pub fn expand_template(template: &str, params: &TemplateParams<'_>) -> Result<Vec<u8>> {
    let bytes = template.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() + params.value.len());
    let mut i = 0;
    while i < bytes.len() {
        let rest = &bytes[i..];
        if rest.starts_with(b"%2i") {
            out.extend_from_slice(&params.page.to_be_bytes());
            i += 3;
        } else if rest.starts_with(b"%2o") {
            out.extend_from_slice(&params.offset.to_le_bytes());
            i += 3;
        } else if rest.starts_with(b"%2c") {
            out.extend_from_slice(&params.count.to_le_bytes());
            i += 3;
        } else if rest.starts_with(b"%v") {
            out.extend_from_slice(params.value);
            i += 2;
        } else if rest.starts_with(b"$tsCanId") {
            out.push(params.can_id);
            i += "$tsCanId".len();
        } else if rest.starts_with(b"\\$") {
            // INI string escape: `\$tsCanId` in real INI source (backslash
            // keeps the `$` literal at INI level) must expand identically to
            // `$tsCanId` — drop the backslash and let the next iteration
            // handle the `$` (placeholder or literal). Without this, the
            // stray 0x5C byte made an 8-byte 'r' request out of a 7-byte one.
            i += 1;
        } else if rest.starts_with(b"\\x") {
            if rest.len() < 4 {
                return Err(ProtocolError::MalformedTemplate(format!(
                    "truncated \\x escape in template `{template}`"
                )));
            }
            let hex = std::str::from_utf8(&rest[2..4]).map_err(|_| {
                ProtocolError::MalformedTemplate(format!(
                    "invalid hex escape in template `{template}`"
                ))
            })?;
            let byte = u8::from_str_radix(hex, 16).map_err(|_| {
                ProtocolError::MalformedTemplate(format!(
                    "invalid hex escape `\\x{hex}` in template `{template}`"
                ))
            })?;
            out.push(byte);
            i += 4;
        } else if bytes[i] < 0x80 {
            out.push(bytes[i]);
            i += 1;
        } else {
            return Err(ProtocolError::MalformedTemplate(format!(
                "non-ASCII byte 0x{:02X} at offset {i} in template `{template}`",
                bytes[i]
            )));
        }
    }
    Ok(out)
}

impl<T: Transport> MsProtocol<T> {
    /// [`crate::Protocol::read_page`] for the generic MS/TS engine.
    ///
    /// Expands `pageReadCommand` (e.g. `"p%2i%2o%2c"`) with `offset=0,
    /// count=page.size` and sends it via [`MsProtocol::send_page_command`].
    ///
    /// Response shape differs by framing (both confirmed against the pinned
    /// source, `case 'p'`):
    /// - **Plain** (`comms_legacy.cpp`): raw page bytes only — the firmware
    ///   writes each byte directly with no status prefix.
    /// - **`MsEnvelope10`** (`comms.cpp`): `[SERIAL_RC_OK, page bytes...]` —
    ///   `serialPayload[0] = SERIAL_RC_OK` is always set before the data, so
    ///   the leading byte is stripped here.
    pub(crate) fn do_read_page(&mut self, page: PageDef) -> Result<Vec<u8>> {
        validate_page_number(page.number)?;
        let count = u16::try_from(page.size).map_err(|_| {
            ProtocolError::InvalidRequest(format!(
                "page {} size {} exceeds the 16-bit wire length limit",
                page.number, page.size
            ))
        })?;
        let template = self.comms.page_read_command.clone();
        let envelope = self.comms.envelope;
        let params = TemplateParams {
            page: page.number,
            offset: 0,
            count,
            value: &[],
            can_id: 0,
        };
        let command = expand_template(&template, &params)?;
        let response = self.send_page_command(&command, page.size, MAX_PAGE_RESPONSE)?;

        let data = match envelope {
            EnvelopeFormat::Plain => response,
            EnvelopeFormat::MsEnvelope10 => {
                let (_status, rest) = response.split_first().ok_or_else(|| {
                    ProtocolError::MalformedResponse(
                        "empty page read response (expected a status byte)".to_string(),
                    )
                })?;
                rest.to_vec()
            }
        };

        if data.len() != page.size {
            return Err(ProtocolError::MalformedResponse(format!(
                "page {} read returned {} bytes, expected {}",
                page.number,
                data.len(),
                page.size
            )));
        }
        Ok(data)
    }

    /// [`crate::Protocol::write`] for the generic MS/TS engine.
    ///
    /// Expands `pageValueWrite` (e.g. `"M%2i%2o%2c%v"`) and sends it via
    /// [`MsProtocol::send_page_command`]. In **Plain** framing this is
    /// fire-and-forget (`comms_legacy.cpp`'s `'M'` handler sends no
    /// acknowledgement); in **`MsEnvelope10`** framing the 1-byte CRC-verified
    /// ack is read and discarded (`comms.cpp` acks via `sendReturnCodeMsg` —
    /// see [`crate::Protocol::write`]'s doc comment for exactly what `Ok(())`
    /// does and does not guarantee).
    ///
    /// Any transport error — a vanished device while sending the command or
    /// while waiting for the ack — propagates as `Err` before either delay
    /// below runs, so a failed write never gets treated as complete.
    ///
    /// Honors `interWriteDelay` (spacing between writes) and the post-write
    /// `pageActivationDelay` (settle time after a page-touching command;
    /// **not** a page-select — current Speeduino has no stateful page
    /// select).
    pub(crate) fn do_write(&mut self, page: u16, offset: u16, bytes: &[u8]) -> Result<()> {
        validate_page_number(page)?;
        let count = u16::try_from(bytes.len()).map_err(|_| {
            ProtocolError::InvalidRequest(format!(
                "write length {} exceeds the 16-bit wire length limit",
                bytes.len()
            ))
        })?;
        let end = u32::from(offset) + u32::from(count);
        if end > u32::from(u16::MAX) + 1 {
            return Err(ProtocolError::InvalidRequest(format!(
                "write offset {offset} + length {count} exceeds the 16-bit address space"
            )));
        }
        let template = self.comms.page_value_write.clone();
        let params = TemplateParams {
            page,
            offset,
            count,
            value: bytes,
            can_id: 0,
        };
        let command = expand_template(&template, &params)?;
        self.send_page_command(&command, 0, MAX_PAGE_RESPONSE)?;

        sleep_ms(self.comms.inter_write_delay_ms);
        sleep_ms(self.comms.page_activation_delay_ms);
        Ok(())
    }

    /// [`crate::Protocol::burn`] for the generic MS/TS engine.
    ///
    /// Expands `burnCommand` (`"b%2i"` — `savePage`, per-page, not
    /// whole-config) and sends it via [`MsProtocol::send_page_command`].
    /// Same framing-dependent ack behaviour as [`Self::do_write`]; no delay
    /// is applied here (the brief ties `interWriteDelay`/`pageActivationDelay`
    /// to writes only).
    pub(crate) fn do_burn(&mut self, page: u16) -> Result<()> {
        validate_page_number(page)?;
        let template = self.comms.burn_command.clone();
        let params = TemplateParams {
            page,
            offset: 0,
            count: 0,
            value: &[],
            can_id: 0,
        };
        let command = expand_template(&template, &params)?;
        self.send_page_command(&command, 0, MAX_PAGE_RESPONSE)?;
        Ok(())
    }

    /// [`crate::Protocol::read_output_channels`] for the generic MS/TS engine.
    ///
    /// Expands `ochGetCommand` (e.g. `"r$tsCanId\x30%2o%2c"`) with the given
    /// `offset`/`len` window and sends it via [`MsProtocol::send_page_command`].
    ///
    /// Response shape differs by framing (`case 'r'`, comms.cpp:359-374):
    /// - **Plain** (`comms_legacy.cpp`): `len` raw bytes, no status prefix —
    ///   no length prefix to distrust, so a short physical reply surfaces as
    ///   a transport timeout rather than silent truncation.
    /// - **`MsEnvelope10`** (`comms.cpp`): `[SERIAL_RC_OK, block bytes...]`,
    ///   status byte stripped here. Unlike [`Self::do_read_page`], a payload
    ///   shorter than `len` is **not** an error — the INI's declared
    ///   `ochBlockSize` can disagree with the firmware's actual frame size, so
    ///   this returns exactly the envelope's own bytes (`rest.to_vec()`).
    pub(crate) fn do_read_output_channels(&mut self, offset: u16, len: u16) -> Result<Vec<u8>> {
        let template = self.comms.och_get_command.clone();
        let envelope = self.comms.envelope;
        let params = TemplateParams {
            page: 0,
            offset,
            count: len,
            value: &[],
            can_id: 0,
        };
        let command = expand_template(&template, &params)?;
        let response = self.send_page_command(&command, len as usize, MAX_PAGE_RESPONSE)?;

        let data = match envelope {
            EnvelopeFormat::Plain => response,
            EnvelopeFormat::MsEnvelope10 => {
                let (_status, rest) = response.split_first().ok_or_else(|| {
                    ProtocolError::MalformedResponse(
                        "empty output-channels response (expected a status byte)".to_string(),
                    )
                })?;
                rest.to_vec()
            }
        };
        Ok(data)
    }
}

fn validate_page_number(page: u16) -> Result<()> {
    if page > u16::from(u8::MAX) {
        return Err(ProtocolError::InvalidRequest(format!(
            "page {page} exceeds the one-byte firmware page-id limit"
        )));
    }
    Ok(())
}

/// Sleep for `ms` milliseconds, skipping the syscall entirely when `ms == 0`
/// (the common case in tests). Real sleep — matches how
/// [`crate::reconnect::ConnectionManager`] honors backoff delays.
fn sleep_ms(ms: u32) {
    let delay = Duration::from_millis(u64::from(ms));
    if !delay.is_zero() {
        std::thread::sleep(delay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 5.1: template expander ─────────────────────────────────────────

    #[test]
    fn expands_2i_2o_2c_v_placeholders() {
        // pageValueWrite = "M%2i%2o%2c%v": page=1 (BE [0x00,0x01]),
        // offset=0x0010 (LE [0x10,0x00]), count=2 (LE [0x02,0x00]), then the
        // raw value bytes. Round-trips through comms.cpp's `case 'M'` parse
        // (serialPayload[2]=page, word(serialPayload[4],serialPayload[3])=offset).
        let params = TemplateParams {
            page: 1,
            offset: 0x0010,
            count: 2,
            value: &[0xAA, 0xBB],
            can_id: 0,
        };
        let out = expand_template("M%2i%2o%2c%v", &params).unwrap();
        assert_eq!(
            out,
            vec![b'M', 0x00, 0x01, 0x10, 0x00, 0x02, 0x00, 0xAA, 0xBB]
        );
    }

    #[test]
    fn expands_tscanid_and_hex_literal() {
        // ochGetCommand = "r$tsCanId\x30%2o%2c" — real Speeduino
        // comms_legacy.cpp template for `case 'r'`: a single raw $tsCanId
        // byte (read-and-discarded by this firmware build), then the 0x30
        // SEND_OUTPUT_CHANNELS sub-command byte, then LE offset/count.
        let params = TemplateParams {
            page: 0,
            offset: 5,
            count: 10,
            value: &[],
            can_id: 0,
        };
        let out = expand_template("r$tsCanId\\x30%2o%2c", &params).unwrap();
        assert_eq!(out, vec![b'r', 0x00, 0x30, 0x05, 0x00, 0x0A, 0x00]);
    }

    #[test]
    fn backslash_dollar_tscanid_expands_identically_to_dollar_tscanid() {
        // Templates parsed from *real INI text* carry the INI's string escape:
        // `ochGetCommand = "r\$tsCanId\x30%2o%2c"` — a backslash before `$`.
        // It must expand byte-identically to the unescaped `$tsCanId` form
        // (M3 Task 6 blocker b): exactly the 7-byte plain 'r' request, not an
        // 8-byte one with a stray 0x5C.
        let params = TemplateParams {
            page: 0,
            offset: 5,
            count: 10,
            value: &[],
            can_id: 0,
        };
        let out = expand_template(r"r\$tsCanId\x30%2o%2c", &params).unwrap();
        assert_eq!(out, vec![b'r', 0x00, 0x30, 0x05, 0x00, 0x0A, 0x00]);
        assert_eq!(out.len(), 7, "no stray 0x5C byte from the INI escape");
    }

    #[test]
    fn expands_tscanid_with_nonzero_can_id() {
        let params = TemplateParams {
            page: 0,
            offset: 0,
            count: 0,
            value: &[],
            can_id: 3,
        };
        let out = expand_template("r$tsCanId", &params).unwrap();
        assert_eq!(out, vec![b'r', 0x03]);
    }

    #[test]
    fn expands_burn_template_page_only() {
        // burnCommand = "b%2i": 'b' + page 3 as big-endian [0x00, 0x03].
        let params = TemplateParams {
            page: 3,
            offset: 0,
            count: 0,
            value: &[],
            can_id: 0,
        };
        let out = expand_template("b%2i", &params).unwrap();
        assert_eq!(out, vec![b'b', 0x00, 0x03]);
    }

    #[test]
    fn rejects_truncated_hex_escape() {
        let params = TemplateParams {
            page: 0,
            offset: 0,
            count: 0,
            value: &[],
            can_id: 0,
        };
        let err = expand_template("x\\x3", &params).unwrap_err();
        assert!(matches!(err, ProtocolError::MalformedTemplate(_)));
    }

    #[test]
    fn rejects_invalid_hex_digits() {
        let params = TemplateParams {
            page: 0,
            offset: 0,
            count: 0,
            value: &[],
            can_id: 0,
        };
        let err = expand_template("\\xZZ", &params).unwrap_err();
        assert!(matches!(err, ProtocolError::MalformedTemplate(_)));
    }

    #[test]
    fn rejects_non_ascii_template_bytes_instead_of_panicking() {
        // A user-supplied INI's command template is meant to be an ASCII wire
        // protocol string; a stray multi-byte UTF-8 char (e.g. a mis-pasted
        // "±") used to make the old &str-slicing scanner panic mid-char
        // while the SessionStore mutex was held. It must now surface as a
        // normal `Err`, never a panic.
        let params = TemplateParams {
            page: 1,
            offset: 0,
            count: 0,
            value: &[],
            can_id: 0,
        };
        let err = expand_template("p±%2i", &params).unwrap_err();
        assert!(matches!(err, ProtocolError::MalformedTemplate(_)));
    }
}
