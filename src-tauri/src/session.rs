// SPDX-License-Identifier: GPL-3.0-or-later
//! Tune operations on the single [`Session`] owner (ARCHITECTURE §9).
//!
//! Every method here runs while the caller holds the session mutex, so all
//! wire I/O is serialized — there is no path to the transport that does not go
//! through a `&mut Session`. The protocol handle is built *per operation* from
//! the connection (cheap for the simulator, whose memory persists across
//! transports); MS/TS page ops carry the page id inline and are stateless, so a
//! fresh handle per op is correct.
//!
//! **Fail-safe strategy (kept divergence-free with zero model surgery):**
//! - `set_value` validates + encodes on a *clone* of the tune first (surfacing
//!   `ModelError` before any wire I/O), writes the byte delta to the ECU, and
//!   commits to the real tune only after the write returns `Ok`. A failed write
//!   leaves both tune and ECU at the old value.
//! - `undo`/`redo` mutate the tune, write the resulting delta, and on write
//!   failure apply the exact inverse (`redo`/`undo`) so the tune snaps back to
//!   match the ECU. Undo/redo are perfect inverses, so no redo-stack wart.

use std::sync::Arc;

use opentune_ini::{CommsSettings, PageDef};
use opentune_model::{ModelError, Tune, Value};
use opentune_protocol::{MsProtocol, Protocol};
use opentune_realtime::{RealtimeError, RealtimeFrame, RealtimePoller};
use opentune_transport::Transport;

use crate::connection::{ActiveConnection, Session};
use crate::dto::{DefinitionDto, ResolvedGaugeBoundsDto};
use crate::events::TuneDirtyEvent;

pub(crate) const NO_TUNE: &str = "no tune loaded — call load_tune first";
const NO_OCH_BLOCK: &str =
    "the loaded INI declares no ochBlockSize — realtime polling is unavailable";
const SERIAL_UNSUPPORTED: &str = "live page operations are not yet wired for serial \
    connections (M3: persist MsProtocol in ConnectionManager); use the simulator for M2";

impl Session {
    /// The UI-facing projection of the parsed definition (menus, dialogs,
    /// constants, tables) for the frontend to render against.
    pub fn definition(&self) -> DefinitionDto {
        DefinitionDto::from(self.def.as_ref())
    }

    /// Read every declared page from the ECU into a fresh [`Tune`]. Loading is
    /// not an edit, so the resulting tune is clean (`dirty == false`).
    pub fn load_tune(&mut self) -> Result<TuneDirtyEvent, String> {
        let Session {
            conn, def, tune, ..
        } = self;
        let mut fresh = Tune::new(Arc::clone(def));
        let mut proto = protocol_for(conn, &def.comms)?;
        for page in &def.pages {
            let bytes = proto.read_page(*page).map_err(|e| e.to_string())?;
            fresh.load_page(page.number, bytes);
        }
        let event = dirty_event(&fresh);
        *tune = Some(fresh);
        Ok(event)
    }

    /// Read the current physical values of the named constants (for rendering).
    ///
    /// **Fails open per value** (M3 Task 6.7): one unresolvable constant
    /// (e.g. an `{ expr }` scale over a missing variable) degrades to a
    /// `NaN` sentinel instead of erroring the whole call, so every other
    /// requested value still renders. The IPC shape stays stable — one
    /// `Value` per requested name. `serde_json` serializes `f64::NAN` as
    /// `null`; the frontend renders that as "—" (Task 7.6).
    pub fn read_values(&self, names: &[String]) -> Result<Vec<Value>, String> {
        let tune = self.tune.as_ref().ok_or_else(|| NO_TUNE.to_string())?;
        Ok(names
            .iter()
            .map(|n| tune.get(n).unwrap_or(Value::Scalar(f64::NAN)))
            .collect())
    }

    /// Resolve tune-dependent gauge bounds in the backend expression engine.
    ///
    /// Bounds fail open independently. The frontend receives `None` for an
    /// unsupported/unknown expression and renders range-dependent geometry
    /// neutrally instead of inventing a misleading 0..100 scale.
    pub fn resolve_gauge_bounds(&self) -> Result<Vec<ResolvedGaugeBoundsDto>, String> {
        let tune = self.tune.as_ref().ok_or_else(|| NO_TUNE.to_string())?;
        let resolve = |owner: &str, number: &opentune_ini::Number| {
            tune.resolve_number(owner, number)
                .ok()
                .filter(|value| value.is_finite())
        };
        Ok(self
            .def
            .gauges
            .iter()
            .map(|gauge| ResolvedGaugeBoundsDto {
                name: gauge.name.clone(),
                low: resolve(&gauge.name, &gauge.low),
                high: resolve(&gauge.name, &gauge.high),
                lo_danger: resolve(&gauge.name, &gauge.lo_danger),
                lo_warn: resolve(&gauge.name, &gauge.lo_warn),
                hi_warn: resolve(&gauge.name, &gauge.hi_warn),
                hi_danger: resolve(&gauge.name, &gauge.hi_danger),
            })
            .collect())
    }

    /// One realtime poll tick (M3 Task 6.5): read the full och block through
    /// the connection, hand it to the coalescing `poller`
    /// ([`RealtimePoller::poll_once`] decodes + gates emission to ≤30 Hz),
    /// and keep the reconnect manager's `secl` baseline in sync.
    ///
    /// The baseline feed is the Task 6 blocker-c fix: `secl` is byte 0 of
    /// the och block by MS/TS convention, and the firmware zeroes it on the
    /// first och request (and wraps it at 255) — without feeding every
    /// successfully polled value into
    /// [`ConnectionManager::note_secl`](opentune_protocol::reconnect::ConnectionManager::note_secl),
    /// a later glitch reconnect compares against a stale baseline, falsely
    /// detects a reboot, and the owner's reboot path re-reads the tune,
    /// silently discarding unburned edits.
    pub fn poll_frame(
        &mut self,
        poller: &mut RealtimePoller,
    ) -> Result<Option<RealtimeFrame>, String> {
        let Session { conn, def, .. } = self;
        let len = u16::try_from(def.comms.och_block_size)
            .map_err(|_| format!("ochBlockSize {} exceeds u16", def.comms.och_block_size))?;
        if len == 0 {
            return Err(NO_OCH_BLOCK.to_string());
        }

        let mut proto = protocol_for(conn, &def.comms)?;
        let polled_secl = std::cell::Cell::new(None);
        let read_block = || {
            let block = proto
                .read_output_channels(0, len)
                .map_err(|e| RealtimeError::Poll(e.to_string()))?;
            if let Some(&byte0) = block.first() {
                polled_secl.set(Some(byte0));
            }
            Ok(block)
        };
        let result = poller.poll_once(read_block, def.as_ref());

        if let Some(secl) = polled_secl.get() {
            match conn {
                ActiveConnection::Sim { manager, .. } => manager.note_secl(secl),
                ActiveConnection::Serial { manager } => manager.note_secl(secl),
            }
        }

        result.map_err(|e| match e {
            RealtimeError::Poll(detail) => detail,
            RealtimeError::NotConnected => "not connected".to_string(),
        })
    }

    /// Set a constant, writing the changed bytes live to the ECU. Validated on
    /// a clone first; the model is committed only after the wire confirms.
    pub fn set_value(&mut self, name: &str, value: Value) -> Result<TuneDirtyEvent, String> {
        let Session {
            conn, def, tune, ..
        } = self;
        let tune = tune.as_mut().ok_or_else(|| NO_TUNE.to_string())?;

        // Validate + compute target bytes without touching the real tune.
        let mut probe = tune.clone();
        probe.set(name, value).map_err(fmt_model_err)?;
        let deltas = page_deltas(tune, &probe, &def.pages);

        // Reach the wire. If this fails, the real tune is still untouched.
        write_deltas(conn, &def.comms, &deltas)?;

        // Commit the already-validated probe verbatim. Re-running `set`
        // after the wire write would create a theoretical divergence window
        // if a future state-dependent validator produced a different result.
        *tune = probe;
        Ok(dirty_event(tune))
    }

    /// Undo the most recent edit, writing the reverted bytes to the ECU. An
    /// undo that does not reach the wire would be a lie, so on write failure we
    /// `redo` to keep the tune consistent with the ECU.
    pub fn undo(&mut self) -> Result<TuneDirtyEvent, String> {
        let Session {
            conn, def, tune, ..
        } = self;
        let tune = tune.as_mut().ok_or_else(|| NO_TUNE.to_string())?;
        let before = tune.clone();
        if !tune.undo() {
            return Ok(dirty_event(tune));
        }
        let deltas = page_deltas(&before, tune, &def.pages);
        if let Err(e) = write_deltas(conn, &def.comms, &deltas) {
            tune.redo(); // reverse the undo so tune matches the ECU
            return Err(e);
        }
        Ok(dirty_event(tune))
    }

    /// Redo the most recently undone edit, writing the re-applied bytes to the
    /// ECU. On write failure, `undo` to stay consistent with the ECU.
    pub fn redo(&mut self) -> Result<TuneDirtyEvent, String> {
        let Session {
            conn, def, tune, ..
        } = self;
        let tune = tune.as_mut().ok_or_else(|| NO_TUNE.to_string())?;
        let before = tune.clone();
        if !tune.redo() {
            return Ok(dirty_event(tune));
        }
        let deltas = page_deltas(&before, tune, &def.pages);
        if let Err(e) = write_deltas(conn, &def.comms, &deltas) {
            tune.undo(); // reverse the redo
            return Err(e);
        }
        Ok(dirty_event(tune))
    }

    /// Burn every dirty page to flash, then clear dirty tracking. If a burn
    /// fails partway, we return the error *without* marking burned: already
    /// burned pages stay marked dirty and are simply re-burned on retry (burn
    /// is idempotent), so no page is ever falsely reported as persisted.
    pub fn burn(&mut self) -> Result<TuneDirtyEvent, String> {
        let Session {
            conn, def, tune, ..
        } = self;
        let tune = tune.as_mut().ok_or_else(|| NO_TUNE.to_string())?;
        let dirty = tune.dirty_pages();
        let mut proto = protocol_for(conn, &def.comms)?;
        for page in &dirty {
            proto.burn(*page).map_err(|e| e.to_string())?;
        }
        tune.mark_burned();
        Ok(dirty_event(tune))
    }

    /// Evaluate `visible`/`enable` expressions against the current tune values.
    /// Fails **open** (a broken expression yields `true`) so a bad INI
    /// condition never silently hides a field.
    pub fn eval_conditions(&self, exprs: &[String]) -> Result<Vec<bool>, String> {
        let tune = self.tune.as_ref().ok_or_else(|| NO_TUNE.to_string())?;
        Ok(exprs
            .iter()
            .map(|e| tune.eval_condition(e).unwrap_or(true))
            .collect())
    }

    /// The tune's current dirty-state event, if a tune is loaded.
    ///
    /// Used by IPC commands to emit a truthful badge state regardless of
    /// whether the triggering operation returned `Ok` or `Err` — e.g.
    /// `merge_tune` applies picks one at a time and can abort mid-batch
    /// (a later pick's write fails) after earlier picks already committed
    /// and dirtied the tune; recomputing the event from `tune` here (rather
    /// than only returning it on the `Ok` path) reflects that actual state.
    pub fn current_dirty_event(&self) -> Option<TuneDirtyEvent> {
        self.tune.as_ref().map(dirty_event)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Build a one-shot protocol handle bound to the connection's transport.
///
/// Simulator only for M2; serial live-write is a recorded M3 follow-up.
fn protocol_for(
    conn: &ActiveConnection,
    comms: &CommsSettings,
) -> Result<Box<dyn Protocol>, String> {
    match conn {
        ActiveConnection::Sim { simulator, .. } => {
            let mut transport = simulator.client_transport();
            transport.open().map_err(|e| e.to_string())?;
            Ok(Box::new(MsProtocol::new(comms.clone(), transport)))
        }
        ActiveConnection::Serial { .. } => Err(SERIAL_UNSUPPORTED.to_string()),
    }
}

/// The contiguous changed byte span per page between two tunes: `(page number,
/// start offset, changed bytes from `after`)`. Uniform for set/undo/redo.
pub(crate) fn page_deltas(
    before: &Tune,
    after: &Tune,
    pages: &[PageDef],
) -> Vec<(u16, usize, Vec<u8>)> {
    pages
        .iter()
        .filter_map(|p| {
            let b = before.page_bytes(p.number);
            let a = after.page_bytes(p.number);
            let first = (0..a.len()).find(|&i| a[i] != b[i])?;
            let last = (first..a.len())
                .rev()
                .find(|&i| a[i] != b[i])
                .unwrap_or(first);
            Some((p.number, first, a[first..=last].to_vec()))
        })
        .collect()
}

/// Write each page delta to the ECU via a fresh protocol handle.
pub(crate) fn write_deltas(
    conn: &ActiveConnection,
    comms: &CommsSettings,
    deltas: &[(u16, usize, Vec<u8>)],
) -> Result<(), String> {
    if deltas.is_empty() {
        return Ok(());
    }
    let mut proto = protocol_for(conn, comms)?;
    for (page, offset, bytes) in deltas {
        let offset = u16::try_from(*offset).map_err(|_| format!("offset {offset} exceeds u16"))?;
        proto
            .write(*page, offset, bytes)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Snapshot the tune's dirty state into an IPC event.
pub(crate) fn dirty_event(tune: &Tune) -> TuneDirtyEvent {
    TuneDirtyEvent {
        dirty: tune.is_dirty(),
        dirty_pages: tune.dirty_pages(),
    }
}

/// Render a [`ModelError`] as a user-facing string (the IPC error channel is
/// `String`, matching the M1 command convention).
pub(crate) fn fmt_model_err(e: ModelError) -> String {
    match e {
        ModelError::UnknownConstant(n) => format!("unknown constant `{n}`"),
        ModelError::OutOfRange { name, value } => {
            format!("`{name}`: value {value} is out of range")
        }
        ModelError::TypeMismatch(m) => format!("type mismatch: {m}"),
        ModelError::UnresolvedExpr(m) => format!("unresolved expression: {m}"),
    }
}

// ── Unit tests (against the simulator) ───────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{connect_simulator, load_definition_from_str};

    const BUNDLED_INI: &str = include_str!("../resources/speeduino.sample.ini");

    fn session() -> Session {
        let def = Arc::new(load_definition_from_str(BUNDLED_INI).expect("bundled INI parses"));
        let conn = connect_simulator(&def, &|_| {}).expect("simulator connects");
        Session {
            conn,
            def,
            tune: None,
            snapshot: None,
        }
    }

    /// Read a page straight off the ECU (bypassing the tune) — this is the
    /// "reached the wire" oracle for set/undo/redo/burn.
    fn ecu_page(session: &Session, number: u16) -> Vec<u8> {
        let ActiveConnection::Sim { simulator, .. } = &session.conn else {
            panic!("expected simulator connection");
        };
        let page = *session
            .def
            .pages
            .iter()
            .find(|p| p.number == number)
            .expect("page declared");
        let mut t = simulator.client_transport();
        t.open().unwrap();
        let mut proto = MsProtocol::new(session.def.comms.clone(), t);
        proto.read_page(page).expect("read_page")
    }

    #[test]
    fn load_tune_starts_clean() {
        let mut s = session();
        let ev = s.load_tune().expect("load");
        assert!(!ev.dirty, "freshly loaded tune must be clean");
        assert!(ev.dirty_pages.is_empty());
    }

    #[test]
    fn read_values_fails_open_per_value_with_nan_sentinel() {
        // Task 6.7 (follow-up b): one unresolvable constant must not blank
        // the whole panel. `bad`'s scale is an expression over a variable
        // that resolves to nothing, so `tune.get("bad")` errors — the value
        // degrades to the NaN sentinel (JSON `null`; the UI renders "—")
        // while every other requested value still comes back intact.
        let ini = r#"
[MegaTune]
   signature            = "speeduino 202504-dev"
   queryCommand         = "Q"
   versionInfo          = "S"
   blockReadTimeout     = 2000
   blockingFactor       = 121
   endianness           = little
   ochGetCommand        = "A"
   pageReadCommand      = "p%2i%2o%2c"
   pageValueWrite       = "M%2i%2o%2c%v"
   burnCommand          = "b%2i"

[Constants]
    endianness      = little
    nPages          = 1
    pageSize        = 8

page = 1
      good  = scalar, U16,  0, "ms", 0.1,                 0.0, 0.0, 6553.5, 1
      bad   = scalar, U16,  2, "x",  { nonexistentVar },  0.0, 0.0, 100.0,  1
"#;
        let def = Arc::new(load_definition_from_str(ini).expect("test INI parses"));
        let conn = connect_simulator(&def, &|_| {}).expect("simulator connects");
        let mut s = Session {
            conn,
            def,
            tune: None,
            snapshot: None,
        };
        s.load_tune().unwrap();

        let values = s
            .read_values(&["good".into(), "bad".into()])
            .expect("one unresolvable constant must not error the whole call");
        assert_eq!(values.len(), 2, "IPC shape stays one value per name");
        assert_eq!(values[0], Value::Scalar(0.0), "resolvable value intact");
        assert!(
            matches!(values[1], Value::Scalar(v) if v.is_nan()),
            "unresolvable value degrades to the NaN sentinel, got {:?}",
            values[1]
        );
    }

    #[test]
    fn set_value_reaches_the_wire_and_dirties() {
        let mut s = session();
        s.load_tune().unwrap();

        let ev = s.set_value("reqFuel", Value::Scalar(12.5)).expect("set");
        assert!(ev.dirty, "set must mark dirty");
        assert_eq!(ev.dirty_pages, vec![1]);

        // reqFuel = U16 LE @ offset 0, scale 0.1 → raw 125 = [125, 0].
        let page = ecu_page(&s, 1);
        assert_eq!(&page[0..2], &[125, 0], "ECU RAM must hold the written raw");
    }

    #[test]
    fn set_value_out_of_range_is_rejected_and_leaves_wire_untouched() {
        let mut s = session();
        s.load_tune().unwrap();
        // reqFuel high = 6553.5; 9999 is out of range.
        let err = s.set_value("reqFuel", Value::Scalar(9999.0)).unwrap_err();
        assert!(err.contains("out of range"), "got: {err}");
        // Nothing written, tune stays clean.
        assert_eq!(&ecu_page(&s, 1)[0..2], &[0, 0]);
        assert!(!s.tune.as_ref().unwrap().is_dirty());
    }

    #[test]
    fn undo_and_redo_reach_the_wire() {
        let mut s = session();
        s.load_tune().unwrap();
        s.set_value("reqFuel", Value::Scalar(12.5)).unwrap();
        assert_eq!(&ecu_page(&s, 1)[0..2], &[125, 0]);

        let ev = s.undo().expect("undo");
        assert_eq!(&ecu_page(&s, 1)[0..2], &[0, 0], "undo must reach the wire");
        // The model tracks touched-since-burn (sticky), not byte-equality, so
        // the page stays dirty after an undo — the badge clears only on burn.
        assert!(ev.dirty);

        let ev = s.redo().expect("redo");
        assert!(ev.dirty);
        assert_eq!(
            &ecu_page(&s, 1)[0..2],
            &[125, 0],
            "redo must reach the wire"
        );
    }

    #[test]
    fn burn_persists_across_reboot_and_clears_dirty() {
        let mut s = session();
        s.load_tune().unwrap();
        s.set_value("reqFuel", Value::Scalar(12.5)).unwrap();

        let ev = s.burn().expect("burn");
        assert!(!ev.dirty, "burn clears dirty");

        // Reboot restores RAM from flash; a burned value survives.
        if let ActiveConnection::Sim { simulator, .. } = &s.conn {
            simulator.reboot();
        }
        s.load_tune().unwrap();
        assert_eq!(
            s.read_values(&["reqFuel".into()]).unwrap(),
            vec![Value::Scalar(12.5)],
            "burned value must survive reboot"
        );
    }

    #[test]
    fn unburned_edit_is_lost_on_reboot() {
        let mut s = session();
        s.load_tune().unwrap();
        s.set_value("reqFuel", Value::Scalar(12.5)).unwrap();
        // No burn.
        if let ActiveConnection::Sim { simulator, .. } = &s.conn {
            simulator.reboot();
        }
        s.load_tune().unwrap();
        assert_eq!(
            s.read_values(&["reqFuel".into()]).unwrap(),
            vec![Value::Scalar(0.0)],
            "un-burned RAM write is lost on reboot"
        );
    }

    #[test]
    fn eval_conditions_gate_on_current_values_and_fail_open() {
        let mut s = session();
        s.load_tune().unwrap();
        // injLayout starts 0 → `injLayout != 0` is false.
        let conds = vec!["injLayout != 0".to_string(), "!!broken((".to_string()];
        assert_eq!(s.eval_conditions(&conds).unwrap(), vec![false, true]);

        // Select a non-zero injector layout → condition becomes true.
        s.set_value("injLayout", Value::Enum(3)).unwrap();
        assert_eq!(
            s.eval_conditions(&["injLayout != 0".to_string()]).unwrap(),
            vec![true]
        );
    }

    #[test]
    fn page_ops_error_without_a_tune() {
        let mut s = session();
        assert!(s.set_value("reqFuel", Value::Scalar(1.0)).is_err());
        assert!(s.burn().is_err());
        assert!(s.read_values(&["reqFuel".into()]).is_err());
    }

    #[test]
    fn current_dirty_event_reflects_tune_state_regardless_of_the_last_ops_result() {
        let mut s = session();
        assert!(
            s.current_dirty_event().is_none(),
            "no tune loaded yet -- nothing to report"
        );

        s.load_tune().unwrap();
        let ev = s.current_dirty_event().expect("tune now loaded");
        assert!(!ev.dirty, "freshly loaded tune is clean");

        s.set_value("reqFuel", Value::Scalar(12.5)).unwrap();
        let ev = s.current_dirty_event().expect("tune still loaded");
        assert!(
            ev.dirty,
            "must reflect the edit even read independently of set_value's own Ok"
        );
        assert_eq!(ev.dirty_pages, vec![1]);
    }
}
