// SPDX-License-Identifier: GPL-3.0-or-later
//! `EcuSimulator` — minimal virtual ECU driven by TDD.
//!
//! Wire semantics ported from Speeduino `comms.cpp`/`comms_legacy.cpp`
//! (GPL-3), per [ADR-0006](../../../../docs/adr/0006-reuse-existing-parsers.md).
//! M1's Q/S/A handshake dispatch and M2 Task 6's page read/write/burn
//! dispatch both port that source directly (byte layout cross-checked
//! against `opentune-protocol`'s `pages.rs`, which cites the exact
//! `comms.cpp`/`comms_legacy.cpp` lines). See [`crate::memory`] for the
//! `speeduino-serial-sim` port-note covering the backing memory image this
//! module dispatches into.

use crate::memory::MemoryImage;
use opentune_ini::{Definition, PageDef};
use opentune_transport::{Transport, TransportError};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Shared state between [`EcuSimulator`] and [`EcuClientTransport`].
struct Pipe {
    cmd_buf: VecDeque<u8>,
    rsp_buf: VecDeque<u8>,
    dropped: bool,
    open: bool,
    /// Firmware second counter. Byte 0 of the `'A'` (realtime) response.
    /// `advance_secl` increments it; `reset_secl` sets it to 0 for reboot tests.
    secl: u8,
    /// Page RAM/flash images (M2 Task 6). Empty when built via
    /// [`EcuSimulator::new`] — page commands against an empty image are the
    /// documented [`MemoryImage`] no-op, so the M1 handshake-only sim keeps
    /// working unchanged.
    memory: MemoryImage,
}

impl Pipe {
    fn new(pages: &[PageDef]) -> Self {
        Self {
            cmd_buf: VecDeque::new(),
            rsp_buf: VecDeque::new(),
            dropped: false,
            open: false,
            secl: 0,
            memory: MemoryImage::new(pages),
        }
    }
}

/// Client-side transport returned by [`EcuSimulator::client_transport`].
pub struct EcuClientTransport {
    pipe: Arc<Mutex<Pipe>>,
    read_timeout: Duration,
}

impl Transport for EcuClientTransport {
    fn open(&mut self) -> opentune_transport::Result<()> {
        self.pipe.lock().unwrap().open = true;
        Ok(())
    }
    fn close(&mut self) -> opentune_transport::Result<()> {
        self.pipe.lock().unwrap().open = false;
        Ok(())
    }
    fn is_open(&self) -> bool {
        self.pipe.lock().unwrap().open
    }

    fn write(&mut self, bytes: &[u8]) -> opentune_transport::Result<()> {
        {
            let mut p = self.pipe.lock().unwrap();
            if p.dropped || !p.open {
                return Err(TransportError::Disconnected);
            }
            p.cmd_buf.extend(bytes);
        }
        process(&self.pipe);
        Ok(())
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> opentune_transport::Result<()> {
        let mut p = self.pipe.lock().unwrap();
        if p.dropped || !p.open {
            return Err(TransportError::Disconnected);
        }
        if p.rsp_buf.len() < buf.len() {
            return Err(TransportError::Timeout(self.read_timeout));
        }
        for slot in buf.iter_mut() {
            *slot = p.rsp_buf.pop_front().unwrap();
        }
        Ok(())
    }

    fn flush(&mut self) -> opentune_transport::Result<()> {
        let mut p = self.pipe.lock().unwrap();
        p.cmd_buf.clear();
        p.rsp_buf.clear();
        Ok(())
    }
}

/// Drain `cmd_buf`, dispatch commands, push responses to `rsp_buf`.
fn process(pipe: &Arc<Mutex<Pipe>>) {
    let mut p = pipe.lock().unwrap();
    if p.dropped {
        return;
    }
    while !p.cmd_buf.is_empty() {
        let first = *p.cmd_buf.front().unwrap();
        if first == 0x00 {
            // CRC envelope: [len_hi, len_lo, payload..., crc(4)]
            if p.cmd_buf.len() < 2 {
                break;
            }
            let plen = u16::from_be_bytes([p.cmd_buf[0], p.cmd_buf[1]]) as usize;
            if p.cmd_buf.len() < 2 + plen + 4 {
                break;
            }
            let _ = p.cmd_buf.drain(..2);
            let payload: Vec<u8> = p.cmd_buf.drain(..plen).collect();
            let _ = p.cmd_buf.drain(..4);
            let secl = p.secl;
            let framed = respond_crc(&payload, secl, &mut p.memory);
            p.rsp_buf.extend(framed);
            return;
        } else {
            // Plain protocol: a command occupies a variable number of bytes
            // depending on its first byte ('p'/'M'/'b' carry a page/offset/
            // length/value payload) — wait for the full command before
            // dispatching, mirroring the CRC branch's "wait for the full
            // frame" handling above.
            let Some(len) = plain_command_len(&p.cmd_buf) else {
                break;
            };
            if p.cmd_buf.len() < len {
                break;
            }
            let cmd_bytes: Vec<u8> = p.cmd_buf.drain(..len).collect();
            let secl = p.secl;
            let out = respond_plain(&cmd_bytes, secl, &mut p.memory);
            p.rsp_buf.extend(out);
        }
    }
}

/// Total byte length of the plain-protocol command starting at `buf`'s
/// front, or `None` if not enough bytes have arrived yet to even compute it
/// (only `'M'` needs this — its value length is itself part of the command,
/// read from the same buffered bytes).
///
/// Layout confirmed against `comms.cpp`/`comms_legacy.cpp` — see
/// `opentune-protocol`'s `pages.rs` module doc for the exact source lines:
/// `'p'` = `cmd + %2i + %2o + %2c` (7 bytes); `'b'` = `cmd + %2i` (3 bytes);
/// `'M'` = `cmd + %2i + %2o + %2c + %v` (7 + value-length bytes).
fn plain_command_len(buf: &VecDeque<u8>) -> Option<usize> {
    match *buf.front()? {
        b'p' => Some(7),
        b'b' => Some(3),
        b'M' => {
            if buf.len() < 7 {
                return None;
            }
            let count = u16::from_le_bytes([buf[5], buf[6]]) as usize;
            Some(7 + count)
        }
        _ => Some(1), // 'Q' / 'S' / 'A' / anything unrecognized.
    }
}

/// Extract `(page, offset, count)` from a `'p'`/`'M'` command's raw bytes —
/// the same layout for plain command bytes and a CRC payload (both start
/// with the command byte). `page` is the low byte only, matching
/// `comms_legacy.cpp`'s "first byte of the page identifier ... is always
/// 0". Returns `None` on a too-short/malformed buffer rather than
/// indexing out of bounds — a corrupt CRC payload must never panic the sim
/// thread.
fn parse_page_offset_count(bytes: &[u8]) -> Option<(u16, u16, u16)> {
    let page = *bytes.get(2)? as u16;
    let offset = u16::from_le_bytes([*bytes.get(3)?, *bytes.get(4)?]);
    let count = u16::from_le_bytes([*bytes.get(5)?, *bytes.get(6)?]);
    Some((page, offset, count))
}

/// Extract the page id from a `'b'` command's raw bytes (`cmd + %2i`, low
/// byte only). `None` on a too-short buffer.
fn parse_page(bytes: &[u8]) -> Option<u16> {
    bytes.get(2).map(|&b| b as u16)
}

/// Dispatch one plain-protocol command and return the raw response bytes
/// (empty for `'M'`/`'b'` — `comms_legacy.cpp` sends no acknowledgement in
/// this framing, so the sim must not push any either or a later read would
/// consume the wrong bytes).
fn respond_plain(cmd_bytes: &[u8], secl: u8, memory: &mut MemoryImage) -> Vec<u8> {
    match cmd_bytes[0] {
        b'Q' => {
            let mut v = EcuSimulator::SIGNATURE.as_bytes().to_vec();
            v.push(0);
            v
        }
        b'S' => {
            let mut v = EcuSimulator::VERSION.as_bytes().to_vec();
            v.push(0);
            v
        }
        // First byte of realtime frame is `secl` — used by reconnect resync.
        b'A' => vec![secl],
        // comms_legacy.cpp `case 'p'`: raw page bytes, no status prefix.
        b'p' => match parse_page_offset_count(cmd_bytes) {
            Some((page, offset, count)) => memory.read(page, offset, count),
            None => Vec::new(),
        },
        // comms_legacy.cpp `case 'M'`: fire-and-forget, no ack bytes.
        b'M' => {
            if let Some((page, offset, count)) = parse_page_offset_count(cmd_bytes) {
                let value = cmd_bytes.get(7..7 + count as usize).unwrap_or(&[]);
                memory.write(page, offset, value);
            }
            Vec::new()
        }
        // comms_legacy.cpp `case 'b'`: fire-and-forget, no ack bytes.
        b'b' => {
            if let Some(page) = parse_page(cmd_bytes) {
                memory.burn(page);
            }
            Vec::new()
        }
        _ => vec![0],
    }
}

/// Dispatch one CRC-framed (`msEnvelope_1.0`) command and return the full
/// wire frame (`[len_hi, len_lo, payload..., crc32]`) ready to push to
/// `rsp_buf`.
fn respond_crc(payload: &[u8], secl: u8, memory: &mut MemoryImage) -> Vec<u8> {
    use opentune_protocol::crc32_of;
    let cmd = payload.first().copied().unwrap_or(0);
    let response: Vec<u8> = match cmd {
        b'Q' => {
            let mut v = EcuSimulator::SIGNATURE.as_bytes().to_vec();
            v.push(0);
            v
        }
        b'S' => {
            let mut v = EcuSimulator::VERSION.as_bytes().to_vec();
            v.push(0);
            v
        }
        b'A' => vec![secl],
        // comms.cpp `case 'p'`: [SERIAL_RC_OK, page bytes...].
        b'p' => match parse_page_offset_count(payload) {
            Some((page, offset, count)) => {
                let mut v = vec![0x00];
                v.extend(memory.read(page, offset, count));
                v
            }
            None => vec![0x00],
        },
        // comms.cpp acks writes via sendReturnCodeMsg(SERIAL_RC_OK).
        b'M' => {
            if let Some((page, offset, count)) = parse_page_offset_count(payload) {
                let value = payload.get(7..7 + count as usize).unwrap_or(&[]);
                memory.write(page, offset, value);
            }
            vec![0x00]
        }
        // comms.cpp acks burns via sendReturnCodeMsg(SERIAL_RC_BURN_OK).
        b'b' => {
            if let Some(page) = parse_page(payload) {
                memory.burn(page);
            }
            vec![0x04]
        }
        _ => vec![0],
    };
    let len = response.len() as u16;
    let crc = crc32_of(&response);
    let mut framed = Vec::with_capacity(2 + response.len() + 4);
    framed.extend(len.to_be_bytes());
    framed.extend(&response);
    framed.extend(crc.to_be_bytes());
    framed
}

/// The virtual ECU.
pub struct EcuSimulator {
    pipe: Arc<Mutex<Pipe>>,
}

impl EcuSimulator {
    pub const SIGNATURE: &'static str = "speeduino 202504-dev";
    pub const VERSION: &'static str = "Speeduino 2025.04-dev";

    /// Handshake-only sim (M1): no declared pages, so page read/write/burn
    /// commands against it are the documented [`MemoryImage`] no-op rather
    /// than an error. Use [`Self::from_definition`] once page geometry is
    /// known.
    pub fn new() -> Self {
        Self {
            pipe: Arc::new(Mutex::new(Pipe::new(&[]))),
        }
    }

    pub fn new_crc() -> Self {
        Self::new()
    }

    /// Build a sim backed by `definition`'s page geometry (M2 Task 6): a
    /// zero-filled RAM + flash image per declared [`PageDef`], enabling
    /// read/write/burn against it via [`opentune_protocol::MsProtocol`].
    pub fn from_definition(definition: &Definition) -> Self {
        Self {
            pipe: Arc::new(Mutex::new(Pipe::new(&definition.pages))),
        }
    }

    pub fn client_transport(&self) -> EcuClientTransport {
        self.pipe.lock().unwrap().open = true;
        EcuClientTransport {
            pipe: Arc::clone(&self.pipe),
            read_timeout: Duration::from_millis(100),
        }
    }

    pub fn set_link_dropped(&self, dropped: bool) {
        self.pipe.lock().unwrap().dropped = dropped;
    }

    /// Advance the `secl` counter by `delta` (wraps at 255).
    /// Drives test `secl_glitch_does_not_trigger_reidentify`.
    pub fn advance_secl(&self, delta: u8) {
        let mut p = self.pipe.lock().unwrap();
        p.secl = p.secl.wrapping_add(delta);
    }

    /// Reset `secl` to 0, simulating an ECU reboot.
    /// Drives test `secl_reboot_triggers_reidentify`.
    pub fn reset_secl(&self) {
        self.pipe.lock().unwrap().secl = 0;
    }

    /// Simulated ECU reboot (M2 Task 6): every declared page's RAM resets
    /// from its flash image — burned bytes survive, un-burned writes are
    /// lost (see [`MemoryImage::reboot`]). Scoped to the memory image only;
    /// `secl` is a separate M1 concern with its own [`Self::reset_secl`].
    pub fn reboot(&self) {
        self.pipe.lock().unwrap().memory.reboot();
    }

    pub fn flush(&self) {
        let mut p = self.pipe.lock().unwrap();
        p.cmd_buf.clear();
        p.rsp_buf.clear();
    }
}

impl Default for EcuSimulator {
    fn default() -> Self {
        Self::new()
    }
}
