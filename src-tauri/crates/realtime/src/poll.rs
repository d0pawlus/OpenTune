// SPDX-License-Identifier: GPL-3.0-or-later
//! The coalescing poll tick (M3 Task 6, sub-step 6.4).
//!
//! The brief names this file `loop.rs`; `loop` is a Rust keyword, so the
//! module is `poll` instead (recorded deviation).
//!
//! [`RealtimePoller`] is driven by the owner task at the 25 Hz poll cadence
//! (40 ms interval) and gates UI emission to ≤30 Hz (33 ms): every call
//! acquires + decodes a block, but a frame is only *returned* when the
//! minimum emit interval has elapsed since the last returned one. To keep
//! this crate decode-only (no `opentune-protocol` dependency — Task 0.4),
//! the raw block arrives through a caller-supplied closure.

use std::time::{Duration, Instant};

use opentune_ini::Definition;

use crate::{decode_frame, RealtimeError, RealtimeFrame};

/// Minimum interval between two emitted frames: 33 ms ≈ 30 Hz. The owner
/// polls at 40 ms (25 Hz), so at the default cadence every poll emits; the
/// gate only coalesces if polls ever arrive faster.
pub const DEFAULT_EMIT_INTERVAL: Duration = Duration::from_millis(33);

/// The owner-held polling state: the last emit time and the emit gate.
#[derive(Debug)]
pub struct RealtimePoller {
    min_emit_interval: Duration,
    last_emit: Option<Instant>,
}

impl RealtimePoller {
    /// A poller gating emission to at most one frame per `min_emit_interval`.
    pub fn new(min_emit_interval: Duration) -> Self {
        Self {
            min_emit_interval,
            last_emit: None,
        }
    }

    /// One poll tick: acquire a raw block via `read_block`, decode it, and
    /// return the frame only if the emit interval has elapsed since the
    /// last emitted frame (`Ok(None)` = acquired but coalesced away). A
    /// failed read propagates as `Err` and leaves the gate untouched.
    pub fn poll_once(
        &mut self,
        read_block: impl FnOnce() -> Result<Vec<u8>, RealtimeError>,
        def: &Definition,
    ) -> Result<Option<RealtimeFrame>, RealtimeError> {
        let block = read_block()?;
        let frame = decode_frame(def, &block);
        let now = Instant::now();
        match self.last_emit {
            Some(last) if now.duration_since(last) < self.min_emit_interval => Ok(None),
            _ => {
                self.last_emit = Some(now);
                Ok(Some(frame))
            }
        }
    }
}

impl Default for RealtimePoller {
    fn default() -> Self {
        Self::new(DEFAULT_EMIT_INTERVAL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_definition() -> Definition {
        opentune_ini::parse_definition(include_str!(
            "../../ini/tests/fixtures/speeduino-output-channels.ini"
        ))
        .expect("Task 2 output-channels fixture must parse")
    }

    fn stub_block() -> Result<Vec<u8>, RealtimeError> {
        Ok(vec![0u8; 16])
    }

    #[test]
    fn two_polls_inside_the_emit_interval_emit_at_most_once() {
        let def = fixture_definition();
        let mut poller = RealtimePoller::new(Duration::from_millis(33));

        let first = poller.poll_once(stub_block, &def).unwrap();
        let second = poller.poll_once(stub_block, &def).unwrap();

        assert!(first.is_some(), "first poll always emits");
        assert!(
            second.is_none(),
            "a second poll < 33 ms after the first must be coalesced"
        );
    }

    #[test]
    fn zero_interval_emits_every_poll() {
        let def = fixture_definition();
        let mut poller = RealtimePoller::new(Duration::ZERO);
        assert!(poller.poll_once(stub_block, &def).unwrap().is_some());
        assert!(
            poller.poll_once(stub_block, &def).unwrap().is_some(),
            "the gate is interval-based, not once-only"
        );
    }

    #[test]
    fn read_errors_propagate_and_leave_the_gate_closed() {
        let def = fixture_definition();
        let mut poller = RealtimePoller::new(Duration::from_millis(33));
        let err = poller
            .poll_once(|| Err(RealtimeError::Poll("wire gone".into())), &def)
            .unwrap_err();
        assert_eq!(err, RealtimeError::Poll("wire gone".into()));
        // The failed poll must not have consumed the first-emit slot.
        assert!(poller.poll_once(stub_block, &def).unwrap().is_some());
    }

    #[test]
    fn emitted_frame_carries_decoded_channels() {
        let def = fixture_definition();
        let mut poller = RealtimePoller::default();
        let mut block = vec![0u8; 16];
        block[4..6].copy_from_slice(&1500u16.to_le_bytes());
        let frame = poller
            .poll_once(move || Ok(block), &def)
            .unwrap()
            .expect("first poll emits");
        assert!(frame
            .channels
            .iter()
            .any(|c| c.name == "rpm" && c.value == 1500.0));
    }
}
