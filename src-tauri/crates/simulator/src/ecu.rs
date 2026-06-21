// SPDX-License-Identifier: GPL-3.0-or-later
//! `EcuSimulator` — minimal virtual ECU driven by TDD.
//!
//! Wire semantics ported from Speeduino `comms.cpp` (GPL-3),
//! per [ADR-0006](../../../../docs/adr/0006-reuse-existing-parsers.md).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use opentune_transport::{Transport, TransportError};

/// Shared pipe between [`EcuSimulator`] and [`EcuClientTransport`].
struct Pipe {
    cmd_buf: VecDeque<u8>,
    rsp_buf: VecDeque<u8>,
    dropped: bool,
    open: bool,
}

impl Pipe {
    fn new() -> Self {
        Self { cmd_buf: VecDeque::new(), rsp_buf: VecDeque::new(), dropped: false, open: false }
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
    fn is_open(&self) -> bool { self.pipe.lock().unwrap().open }

    fn write(&mut self, bytes: &[u8]) -> opentune_transport::Result<()> {
        {
            let mut p = self.pipe.lock().unwrap();
            if p.dropped || !p.open { return Err(TransportError::Disconnected); }
            p.cmd_buf.extend(bytes);
        }
        process(&self.pipe);
        Ok(())
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> opentune_transport::Result<()> {
        let mut p = self.pipe.lock().unwrap();
        if p.dropped || !p.open { return Err(TransportError::Disconnected); }
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
    if p.dropped { return; }
    while !p.cmd_buf.is_empty() {
        let first = *p.cmd_buf.front().unwrap();
        if first == 0x00 {
            // CRC envelope: [len_hi, len_lo, payload..., crc(4)]
            if p.cmd_buf.len() < 2 { break; }
            let plen = u16::from_be_bytes([p.cmd_buf[0], p.cmd_buf[1]]) as usize;
            if p.cmd_buf.len() < 2 + plen + 4 { break; }
            let _ = p.cmd_buf.drain(..2);
            let payload: Vec<u8> = p.cmd_buf.drain(..plen).collect();
            let _ = p.cmd_buf.drain(..4);
            let cmd = *payload.first().unwrap_or(&0);
            respond_crc(cmd, &mut p.rsp_buf);
        } else {
            let cmd = p.cmd_buf.pop_front().unwrap();
            respond_plain(cmd, &mut p.rsp_buf);
        }
    }
}

fn respond_plain(cmd: u8, rsp: &mut VecDeque<u8>) {
    match cmd {
        b'Q' => { rsp.extend(EcuSimulator::SIGNATURE.as_bytes()); rsp.push_back(0); }
        b'S' => { rsp.extend(EcuSimulator::VERSION.as_bytes()); rsp.push_back(0); }
        b'A' => { rsp.push_back(0); }
        _ => { rsp.push_back(0); }
    }
}

fn respond_crc(cmd: u8, rsp: &mut VecDeque<u8>) {
    use opentune_protocol::crc32_of;
    let payload: Vec<u8> = match cmd {
        b'Q' => { let mut v = EcuSimulator::SIGNATURE.as_bytes().to_vec(); v.push(0); v }
        b'S' => { let mut v = EcuSimulator::VERSION.as_bytes().to_vec(); v.push(0); v }
        b'A' => vec![0],
        _ => vec![0],
    };
    let len = payload.len() as u16;
    let crc = crc32_of(&payload);
    rsp.extend(len.to_be_bytes());
    rsp.extend(&payload);
    rsp.extend(crc.to_be_bytes());
}

/// The virtual ECU.
pub struct EcuSimulator { pipe: Arc<Mutex<Pipe>> }

impl EcuSimulator {
    pub const SIGNATURE: &'static str = "speeduino 202504-dev";
    pub const VERSION: &'static str = "Speeduino 2025.04-dev";

    pub fn new() -> Self { Self { pipe: Arc::new(Mutex::new(Pipe::new())) } }
    pub fn new_crc() -> Self { Self::new() }

    pub fn client_transport(&self) -> EcuClientTransport {
        self.pipe.lock().unwrap().open = true;
        EcuClientTransport { pipe: Arc::clone(&self.pipe), read_timeout: Duration::from_millis(100) }
    }

    pub fn set_link_dropped(&self, dropped: bool) {
        self.pipe.lock().unwrap().dropped = dropped;
    }

    pub fn flush(&self) {
        let mut p = self.pipe.lock().unwrap();
        p.cmd_buf.clear();
        p.rsp_buf.clear();
    }
}

impl Default for EcuSimulator {
    fn default() -> Self { Self::new() }
}
