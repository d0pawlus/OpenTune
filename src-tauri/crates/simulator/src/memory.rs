// SPDX-License-Identifier: GPL-3.0-or-later
//! Backing memory image for the simulated ECU: a RAM image per declared
//! [`PageDef`], plus a separate flash image, so write→burn→reboot semantics
//! (M2 Task 6) are testable — burned bytes survive a simulated reboot,
//! un-burned writes do not.
//!
//! **Port note (ADR-0006):** the M2 plan names
//! [`speeduino-serial-sim`](https://github.com/askrejans/speeduino-serial-sim)
//! (confirmed MIT-licensed, not GPL-3 as the task brief assumed — checked
//! directly against the repo, commit reachable via its `main` branch as of
//! this writing) as the port source. Inspecting `src/SpeeduinoProtocol.cpp`
//! there shows it has **no page-write (`'M'`) handler at all**, its `'p'`
//! read always returns zero-filled bytes regardless of any prior state ("//
//! Simulator returns zeros for all pages"), and its `'b'` burn is a bare
//! acknowledgement with nothing persisted — i.e. it has no RAM/flash image
//! to port. What it *does* confirm is the wire-dispatch shape (framed
//! `[len][payload][crc]`, `case 'p'`/`case 'b'`, a `SERIAL_RC_OK`-style
//! status-byte prefix, page/offset/length at payload bytes `[2]`/`[3-4]`/
//! `[5-6]`) — which matches what `opentune-protocol`'s Task 5 already
//! confirmed directly against Speeduino's real `comms.cpp`. Per ADR-0006's
//! escape clause, the read/write/burn *state* logic below is therefore
//! **written fresh**, not ported; see [`crate::ecu`] for the wire-dispatch
//! side that consumes it.

use opentune_ini::PageDef;

/// Per-page RAM + flash images, addressed by declared page *number*
/// (`PageDef::number`) — not by array position, since page numbers are not
/// guaranteed contiguous from 0.
#[derive(Debug, Clone)]
pub struct MemoryImage {
    pages: Vec<PageDef>,
    ram: Vec<Vec<u8>>,
    flash: Vec<Vec<u8>>,
}

impl MemoryImage {
    /// Build an image with each declared page zero-filled to its size, RAM
    /// and flash starting identical (a fresh/unburned device).
    pub fn new(pages: &[PageDef]) -> Self {
        let ram: Vec<Vec<u8>> = pages.iter().map(|p| vec![0u8; p.size]).collect();
        let flash = ram.clone();
        Self {
            pages: pages.to_vec(),
            ram,
            flash,
        }
    }

    fn index_of(&self, page: u16) -> Option<usize> {
        self.pages.iter().position(|p| p.number == page)
    }

    /// Read `count` bytes at `offset` from `page`'s RAM. An unknown page, or
    /// an offset/count that runs past the page's declared size, returns
    /// zero-filled bytes rather than panicking (fail-safe — never crash the
    /// sim thread on a malformed request).
    pub fn read(&self, page: u16, offset: u16, count: u16) -> Vec<u8> {
        let count = count as usize;
        let Some(idx) = self.index_of(page) else {
            return vec![0u8; count];
        };
        let ram = &self.ram[idx];
        let start = (offset as usize).min(ram.len());
        let end = start.saturating_add(count).min(ram.len());
        let mut out = ram[start..end].to_vec();
        out.resize(count, 0);
        out
    }

    /// Write `value` at `offset` into `page`'s RAM. Mutates RAM only — call
    /// [`Self::burn`] to persist. An unknown page, or a write that would run
    /// past the page's declared size, is a silent no-op — matches Speeduino
    /// `setPageValue`'s tolerance of an out-of-range index.
    pub fn write(&mut self, page: u16, offset: u16, value: &[u8]) {
        let Some(idx) = self.index_of(page) else {
            return;
        };
        let ram = &mut self.ram[idx];
        let offset = offset as usize;
        let Some(end) = offset.checked_add(value.len()) else {
            return;
        };
        if end > ram.len() {
            return;
        }
        ram[offset..end].copy_from_slice(value);
    }

    /// Persist `page`'s current RAM to flash (`savePage`/burn semantics).
    /// Unknown page id is a silent no-op.
    pub fn burn(&mut self, page: u16) {
        if let Some(idx) = self.index_of(page) {
            self.flash[idx] = self.ram[idx].clone();
        }
    }

    /// Simulated reboot: every page's RAM is reset from its flash image.
    /// Un-burned writes are lost; burned bytes survive.
    pub fn reboot(&mut self) {
        self.ram = self.flash.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_page() -> Vec<PageDef> {
        vec![PageDef { number: 0, size: 4 }]
    }

    // ── 6.1: write_read_roundtrip (memory-image unit-level slice) ─────────

    #[test]
    fn new_image_is_zero_filled() {
        let img = MemoryImage::new(&one_page());
        assert_eq!(img.read(0, 0, 4), vec![0, 0, 0, 0]);
    }

    #[test]
    fn write_then_read_reflects_change() {
        let mut img = MemoryImage::new(&one_page());
        img.write(0, 1, &[0xAA, 0xBB]);
        assert_eq!(img.read(0, 0, 4), vec![0, 0xAA, 0xBB, 0]);
    }

    #[test]
    fn read_unknown_page_is_zero_filled_not_panic() {
        let img = MemoryImage::new(&one_page());
        assert_eq!(img.read(9, 0, 2), vec![0, 0]);
    }

    #[test]
    fn write_to_unknown_page_is_a_noop() {
        let mut img = MemoryImage::new(&one_page());
        img.write(9, 0, &[0xFF]);
        assert_eq!(img.read(9, 0, 1), vec![0]);
    }

    #[test]
    fn write_past_page_end_is_a_noop() {
        let mut img = MemoryImage::new(&one_page());
        img.write(0, 3, &[0xAA, 0xBB]); // offset 3 + len 2 > size 4
        assert_eq!(img.read(0, 0, 4), vec![0, 0, 0, 0]);
    }

    // ── 6.2: burn_persists (memory-image unit-level slice) ────────────────

    #[test]
    fn burn_then_reboot_keeps_burned_bytes() {
        let mut img = MemoryImage::new(&one_page());
        img.write(0, 0, &[0x11, 0x22]);
        img.burn(0);
        img.reboot();
        assert_eq!(img.read(0, 0, 2), vec![0x11, 0x22]);
    }

    #[test]
    fn unburned_write_is_lost_on_reboot() {
        let mut img = MemoryImage::new(&one_page());
        img.write(0, 0, &[0x11]);
        img.burn(0);
        img.write(0, 0, &[0x99]); // never burned
        img.reboot();
        assert_eq!(img.read(0, 0, 1), vec![0x11]);
    }
}
