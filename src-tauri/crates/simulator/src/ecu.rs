// SPDX-License-Identifier: GPL-3.0-or-later
//! `EcuSimulator` — minimal virtual ECU driven by TDD.
//!
//! Wire semantics ported from Speeduino `comms.cpp`/`comms_legacy.cpp`
//! (GPL-3), per [ADR-0006](../../../../docs/adr/0006-reuse-existing-parsers.md).
//! M1's Q/S/A handshake dispatch, M2 Task 6's page read/write/burn
//! dispatch, and M3 Task 5's `'r'`/0x30 realtime-window dispatch all port
//! that source directly (byte layout cross-checked against
//! `opentune-protocol`'s `pages.rs`, which cites the exact
//! `comms.cpp`/`comms_legacy.cpp` lines). See [`crate::memory`] for the
//! `speeduino-serial-sim` port-note covering the backing memory image this
//! module dispatches into, and [`crate::engine`] for the animated model
//! (and its own port-note) behind the `'r'` responses.

use crate::engine::SimEngine;
use crate::memory::MemoryImage;
use crate::ve_model::{self, VeContext};
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
    /// Snapshot of the animated realtime frame the `'r'` arm windows into
    /// (M3 Task 5). Empty without an engine — every window then zero-pads.
    och_block: Vec<u8>,
    /// Whether the first `'r'` request has been answered — drives the
    /// firmware's first-request `secl` reset (comms.cpp:361-365).
    first_och_done: bool,
    /// The animated model (M3 Task 5). `None` when built via
    /// [`EcuSimulator::new`], so the M1 handshake-only sim stays unchanged.
    engine: Option<SimEngine>,
    /// M4 Task 9: the definition backing this sim, retained so each engine
    /// tick can re-resolve the current `[VeAnalyze]`-bound `veTable` via
    /// [`crate::ve_model::ve_context`] (a few small linear scans over
    /// `tables`/`constants` — cheap enough to redo every tick rather than
    /// caching a resolved binding). `None` only for [`EcuSimulator::new`]
    /// (no definition at all); [`ve_context`] itself is `None` for a
    /// `Some` definition with no `[VeAnalyze]` section.
    ///
    /// [`ve_context`]: crate::ve_model::ve_context
    definition: Option<Definition>,
    /// Whether production `'r'` requests advance the engine off the wall
    /// clock (see [`Pipe::auto_tick`]). Defaults `true`; permanently
    /// disabled the moment [`EcuSimulator::tick_engine`] is called
    /// explicitly, so deterministic tests keep driving simulated time
    /// themselves.
    auto_tick: bool,
    /// Wall-clock timestamp of the last [`Pipe::auto_tick`] step, or `None`
    /// before the first one (which then ticks by a ~zero `dt`).
    last_auto_tick: Option<std::time::Instant>,
}

impl Pipe {
    fn new(pages: &[PageDef], engine: Option<SimEngine>, definition: Option<Definition>) -> Self {
        Self {
            cmd_buf: VecDeque::new(),
            rsp_buf: VecDeque::new(),
            dropped: false,
            open: false,
            secl: 0,
            memory: MemoryImage::new(pages),
            och_block: engine
                .as_ref()
                .map(|e| e.och_block().to_vec())
                .unwrap_or_default(),
            first_och_done: false,
            engine,
            definition,
            auto_tick: true,
            last_auto_tick: None,
        }
    }

    /// Decode the currently-bound `veTable` from this pipe's memory image
    /// (M4 Task 9), or `None` when there's no definition or the loaded INI
    /// has no `[VeAnalyze]` binding.
    fn ve_context(&self) -> Option<VeContext> {
        ve_model::ve_context(self.definition.as_ref()?, &self.memory)
    }

    /// First-`'r'`-request bookkeeping, ported from `generateLiveValues`
    /// (comms.cpp:361-365): the **first** realtime request after boot
    /// resets `secl` to 0 so the tuner's stay-alive counter starts from a
    /// known origin. Both the `'A'`-path counter (`self.secl`) and the
    /// engine's frame counter reset, and the block snapshot refreshes so
    /// this very response already carries `secl = 0`.
    fn on_och_request(&mut self) {
        if self.first_och_done {
            return;
        }
        self.first_och_done = true;
        self.secl = 0;
        if let Some(engine) = self.engine.as_mut() {
            engine.reset_secl();
            self.och_block.clear();
            self.och_block.extend_from_slice(engine.och_block());
        }
    }

    /// Advance the engine by the wall-clock time elapsed since the last
    /// auto-tick, so production `'r'` polling — which has no other way to
    /// move simulated time (see [`EcuSimulator::tick_engine`]'s doc) —
    /// actually animates instead of replaying the same stale `och_block`
    /// forever. No-op once [`EcuSimulator::tick_engine`] has been called
    /// explicitly (`auto_tick == false`) or when there is no engine.
    ///
    /// No `dt` cap: an engine step is cheap fixed-point arithmetic over a
    /// 50 ms quantum ([`crate::engine::SimEngine::tick`]), so even a large
    /// gap between polls (e.g. after the process was suspended) is a
    /// bounded number of steps, not a hang.
    fn auto_tick(&mut self) {
        if !self.auto_tick {
            return;
        }
        // M4 Task 9: refresh the VE context from current page memory before
        // ticking, so this step's `afr` reflects the latest written veTable.
        let ctx = self.ve_context();
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        engine.set_ve_context(ctx);
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.last_auto_tick.unwrap_or(now));
        engine.tick(dt);
        self.och_block.clear();
        self.och_block.extend_from_slice(engine.och_block());
        self.last_auto_tick = Some(now);
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
            let framed = respond_crc(&payload, &mut p);
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
            let out = respond_plain(&cmd_bytes, &mut p);
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
/// `'M'` = `cmd + %2i + %2o + %2c + %v` (7 + value-length bytes);
/// `'r'` = `cmd + $tsCanId + subcmd + %2o + %2c` (7 bytes — same total as
/// `'p'` but a different layout, see [`parse_och_window`]).
fn plain_command_len(buf: &VecDeque<u8>) -> Option<usize> {
    match *buf.front()? {
        b'p' | b'r' => Some(7),
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

/// Extract `(offset, len)` from an `'r'` command's raw bytes:
/// `['r', tsCanId, 0x30, offset LE(2), len LE(2)]`. The layout differs
/// from `'p'` — byte `[1]` is the CAN id (discarded, as the firmware does)
/// and byte `[2]` the sub-command; offset/len follow at `[3-4]`/`[5-6]`.
/// Only sub-command 0x30 ("send output channels", comms.cpp:359-374) is
/// understood; anything else — including a truncated buffer — is `None`,
/// answered gracefully by the caller rather than panicking.
fn parse_och_window(bytes: &[u8]) -> Option<(u16, u16)> {
    if *bytes.get(2)? != 0x30 {
        return None;
    }
    let offset = u16::from_le_bytes([*bytes.get(3)?, *bytes.get(4)?]);
    let len = u16::from_le_bytes([*bytes.get(5)?, *bytes.get(6)?]);
    Some((offset, len))
}

/// Window `len` bytes at `offset` out of the och block, zero-padding past
/// the end (mirrors [`MemoryImage::read`]'s fail-safe clamping — an
/// out-of-range window must never panic the sim thread).
fn och_window(block: &[u8], offset: u16, len: u16) -> Vec<u8> {
    let len = len as usize;
    let start = (offset as usize).min(block.len());
    let end = start.saturating_add(len).min(block.len());
    let mut out = block[start..end].to_vec();
    out.resize(len, 0);
    out
}

/// Dispatch one plain-protocol command and return the raw response bytes
/// (empty for `'M'`/`'b'` — `comms_legacy.cpp` sends no acknowledgement in
/// this framing, so the sim must not push any either or a later read would
/// consume the wrong bytes).
fn respond_plain(cmd_bytes: &[u8], pipe: &mut Pipe) -> Vec<u8> {
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
        b'A' => vec![pipe.secl],
        // comms_legacy.cpp `case 'p'`: raw page bytes, no status prefix.
        b'p' => match parse_page_offset_count(cmd_bytes) {
            Some((page, offset, count)) => pipe.memory.read(page, offset, count),
            None => Vec::new(),
        },
        // comms_legacy.cpp `case 'M'`: fire-and-forget, no ack bytes.
        b'M' => {
            if let Some((page, offset, count)) = parse_page_offset_count(cmd_bytes) {
                let value = cmd_bytes.get(7..7 + count as usize).unwrap_or(&[]);
                pipe.memory.write(page, offset, value);
            }
            Vec::new()
        }
        // comms_legacy.cpp `case 'b'`: fire-and-forget, no ack bytes.
        b'b' => {
            if let Some(page) = parse_page(cmd_bytes) {
                pipe.memory.burn(page);
            }
            Vec::new()
        }
        // comms_legacy.cpp `case 'r'`: raw window bytes, no status prefix.
        b'r' => match parse_och_window(cmd_bytes) {
            Some((offset, len)) => {
                pipe.on_och_request();
                pipe.auto_tick();
                och_window(&pipe.och_block, offset, len)
            }
            None => Vec::new(),
        },
        _ => vec![0],
    }
}

/// Dispatch one CRC-framed (`msEnvelope_1.0`) command and return the full
/// wire frame (`[len_hi, len_lo, payload..., crc32]`) ready to push to
/// `rsp_buf`.
fn respond_crc(payload: &[u8], pipe: &mut Pipe) -> Vec<u8> {
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
        b'A' => vec![pipe.secl],
        // comms.cpp `case 'p'`: [SERIAL_RC_OK, page bytes...].
        b'p' => match parse_page_offset_count(payload) {
            Some((page, offset, count)) => {
                let mut v = vec![0x00];
                v.extend(pipe.memory.read(page, offset, count));
                v
            }
            None => vec![0x00],
        },
        // comms.cpp acks writes via sendReturnCodeMsg(SERIAL_RC_OK).
        b'M' => {
            if let Some((page, offset, count)) = parse_page_offset_count(payload) {
                let value = payload.get(7..7 + count as usize).unwrap_or(&[]);
                pipe.memory.write(page, offset, value);
            }
            vec![0x00]
        }
        // comms.cpp acks burns via sendReturnCodeMsg(SERIAL_RC_BURN_OK).
        b'b' => {
            if let Some(page) = parse_page(payload) {
                pipe.memory.burn(page);
            }
            vec![0x04]
        }
        // comms.cpp `case 'r'` (359-374): [SERIAL_RC_OK, window bytes...].
        b'r' => match parse_och_window(payload) {
            Some((offset, len)) => {
                pipe.on_och_request();
                pipe.auto_tick();
                let mut v = vec![0x00];
                v.extend(och_window(&pipe.och_block, offset, len));
                v
            }
            None => vec![0x00],
        },
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
    /// than an error — and no engine, so `'r'` windows are all zero-fill.
    /// Use [`Self::from_definition`] once the INI geometry is known.
    pub fn new() -> Self {
        Self {
            pipe: Arc::new(Mutex::new(Pipe::new(&[], None, None))),
        }
    }

    pub fn new_crc() -> Self {
        Self::new()
    }

    /// Build a sim backed by `definition`: a zero-filled RAM + flash image
    /// per declared [`PageDef`] (M2 Task 6) plus an animated [`SimEngine`]
    /// writing the `[OutputChannels]` frame (M3 Task 5), both spoken to via
    /// [`opentune_protocol::MsProtocol`]. Advance the animation with
    /// [`Self::tick_engine`].
    ///
    /// M4 Task 9: also retains a clone of `definition` so every subsequent
    /// tick can re-resolve its `[VeAnalyze]`-bound `veTable` (if any) — see
    /// [`crate::ve_model::ve_context`].
    pub fn from_definition(definition: &Definition) -> Self {
        Self {
            pipe: Arc::new(Mutex::new(Pipe::new(
                &definition.pages,
                Some(SimEngine::new(definition)),
                Some(definition.clone()),
            ))),
        }
    }

    /// Advance the animated engine model by `dt` and refresh the realtime
    /// (`'r'`) block snapshot. The engine itself is deterministic — it has
    /// no wall clock, so *this call* is what moves its simulated time.
    ///
    /// In production, wall-clock time moves the engine automatically on
    /// every `'r'` request instead (see [`Pipe::auto_tick`]) — there is no
    /// other driver, since the real app never calls this test-only entry
    /// point. Calling `tick_engine` explicitly, even once, hands
    /// time-keeping over to the caller and **permanently disables**
    /// auto-tick on this simulator, so a test that drives time by hand never
    /// races the wall clock.
    ///
    /// No-op for sims built without a definition ([`Self::new`]): they have
    /// no engine and keep answering `'r'` with zero-fill.
    pub fn tick_engine(&self, dt: Duration) {
        let mut guard = self.pipe.lock().unwrap();
        let p = &mut *guard;
        p.auto_tick = false;
        // M4 Task 9: refresh the VE context from current page memory before
        // ticking (same reasoning as `Pipe::auto_tick`).
        let ctx = p.ve_context();
        if let Some(engine) = p.engine.as_mut() {
            engine.set_ve_context(ctx);
            engine.tick(dt);
            p.och_block.clear();
            p.och_block.extend_from_slice(engine.och_block());
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
    /// lost (see [`MemoryImage::reboot`]). Also re-arms the first-`'r'`
    /// `secl` reset (M3 Task 5) — the firmware's `firstCommsRequest` is a
    /// boot-scoped static, so a reboot starts a fresh "first request".
    /// `secl` itself is a separate M1 concern with its own
    /// [`Self::reset_secl`].
    ///
    /// Also clears the auto-tick clock (`last_auto_tick`) so the next
    /// production `'r'` request after reboot ticks by a ~zero `dt` instead
    /// of a huge one covering the whole time the "ECU" was down. This does
    /// **not** re-enable auto-tick if [`Self::tick_engine`] had already
    /// disabled it — a reboot doesn't hand time-keeping back to the wall
    /// clock mid-test.
    pub fn reboot(&self) {
        let mut p = self.pipe.lock().unwrap();
        p.memory.reboot();
        p.first_och_done = false;
        p.last_auto_tick = None;
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
