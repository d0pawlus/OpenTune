// SPDX-License-Identifier: GPL-3.0-or-later
//! The owner-side realtime capture ring (M4 Task 8) — the `ve_analyze` data
//! seam: a bounded, column-oriented buffer tapped from the owner's poll tick.

use std::collections::VecDeque;
use std::time::Instant;

use crate::dto::CaptureStatusDto;

/// ~18 min at the 25 Hz emit rate; ~1.1 kB/row on the real INI (139 ch × 8 B).
pub const CAPTURE_CAPACITY: usize = 27_000;

/// Bounded, column-oriented realtime capture ring. Columns are pinned at
/// start (definition channel order); each emitted frame appends one f64 row
/// (missing channel → NaN — fail-open per item). Oldest rows drop first.
///
/// Rate note (recorded): the tap sits on poll_tick's EMITTED frames. The M3
/// poller coalesces to ≤30 Hz, but the owner polls at 25 Hz (40 ms) — slower
/// than the 33 ms gate — so today every acquired frame is emitted and the
/// capture sees the full poll rate. If M5 ever polls faster than 30 Hz, move
/// the tap below the coalescing gate (poll.rs) — test `capture_rate_pins_
/// the_tap_invariant` breaks loudly if the assumption rots.
pub struct CaptureBuffer {
    columns: Vec<String>,
    rows: VecDeque<(f64, Vec<f64>)>,
    start: Instant,
    capacity: usize,
    dropped: u32,
}

impl CaptureBuffer {
    /// A capture ring pinned to `columns` (declaration order), bounded to at
    /// most `capacity` rows.
    pub fn new(columns: Vec<String>, capacity: usize) -> Self {
        Self {
            columns,
            rows: VecDeque::new(),
            start: Instant::now(),
            capacity,
            dropped: 0,
        }
    }

    /// Append one row from a decoded realtime frame: `t = now - start`, one
    /// value per pinned column via linear lookup over `frame.channels` (no
    /// `HashMap` — deterministic order), NaN when the channel is absent this
    /// frame. Drops the oldest row first once at capacity.
    pub fn push(&mut self, frame: &opentune_realtime::RealtimeFrame) {
        let t_ms = self.start.elapsed().as_secs_f64() * 1000.0;
        let row: Vec<f64> = self
            .columns
            .iter()
            .map(|col| {
                frame
                    .channels
                    .iter()
                    .find(|c| c.name == *col)
                    .map_or(f64::NAN, |c| c.value)
            })
            .collect();
        if self.rows.len() >= self.capacity {
            self.rows.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.rows.push_back((t_ms, row));
    }

    /// The ring's current status. `duration_ms` is the last captured row's
    /// `t_ms` (0 when empty) — elapsed capture time, which freezes once
    /// capturing stops rather than drifting with wall-clock time between
    /// polls. Once the ring has wrapped it exceeds the span of the rows
    /// actually retained (oldest were dropped; see `dropped`).
    pub fn status(&self, capturing: bool) -> CaptureStatusDto {
        CaptureStatusDto {
            capturing,
            sample_count: saturating_u32(self.rows.len()),
            duration_ms: self.rows.back().map_or(0.0, |(t, _)| *t),
            dropped: self.dropped,
        }
    }

    /// Export the current rows as a [`opentune_analysis::SampleSet`]: clones,
    /// column/row order preserved.
    ///
    /// Task 0/8 seam: Task 11's `RunVeAnalyze` handler is the production
    /// caller (not yet wired — the owner arm still stubs `Err`); today this
    /// is exercised only by this module's unit tests.
    #[allow(dead_code)]
    pub fn to_sample_set(&self) -> opentune_analysis::SampleSet {
        let t_ms = self.rows.iter().map(|(t, _)| *t).collect();
        let rows = self.rows.iter().map(|(_, row)| row.clone()).collect();
        opentune_analysis::SampleSet {
            columns: self.columns.clone(),
            t_ms,
            rows,
        }
    }
}

/// `rows.len()`/`dropped` are bounded by `capacity`/poll count in practice,
/// but the DTO boundary is `u32` (specta 0.0.12 forbids `usize` over IPC) —
/// saturate instead of panicking on the theoretical overflow.
fn saturating_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_fills_rows_by_pinned_columns_with_nan_for_missing() {
        let mut buf = CaptureBuffer::new(vec!["rpm".into(), "afr".into()], 4);
        buf.push(&frame(&[("afr", 14.7), ("rpm", 3000.0)])); // order differs from columns
        buf.push(&frame(&[("rpm", 3100.0)])); // afr missing this frame
        let s = buf.to_sample_set();
        assert_eq!(s.columns, vec!["rpm", "afr"]);
        assert_eq!(s.rows[0], vec![3000.0, 14.7]);
        assert_eq!(s.rows[1][0], 3100.0);
        assert!(s.rows[1][1].is_nan());
        assert!(s.t_ms[1] >= s.t_ms[0]);
    }

    #[test]
    fn ring_drops_oldest_and_counts() {
        let mut buf = CaptureBuffer::new(vec!["rpm".into()], 2);
        for v in [1.0, 2.0, 3.0] {
            buf.push(&frame(&[("rpm", v)]));
        }
        let s = buf.to_sample_set();
        assert_eq!(s.rows, vec![vec![2.0], vec![3.0]]);
        assert_eq!(buf.status(true).dropped, 1);
        assert_eq!(buf.status(true).sample_count, 2);
    }

    fn frame(pairs: &[(&str, f64)]) -> opentune_realtime::RealtimeFrame {
        opentune_realtime::RealtimeFrame {
            channels: pairs
                .iter()
                .map(|(n, v)| opentune_realtime::ChannelValue {
                    name: (*n).to_string(),
                    value: *v,
                })
                .collect(),
            diagnostics: vec![],
        }
    }
}
