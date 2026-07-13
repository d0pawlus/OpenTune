// SPDX-License-Identifier: GPL-3.0-or-later
//! Task 8 tune diff/merge — `Session` methods, split out of `session.rs` for
//! file-size cohesion. Shares `session.rs`'s fail-safe helpers
//! (`page_deltas`/`write_deltas`/`dirty_event`/`fmt_model_err`) and its
//! documented strategy: validate on a *clone* before touching the wire,
//! write, then commit to the real tune only once the write confirms.
//!
//! `merge_tune` applies picks **one at a time**, not as a single batched
//! delta: a multi-pick merge could span several pages, and only a per-pick
//! commit keeps every already-applied pick's tune state matching the ECU if
//! a later pick's write fails. Picks that don't resolve on the snapshot, or
//! that `Tune::set` rejects, are silently skipped — mirroring
//! `opentune_model::merge`'s fail-open contract (used directly by the pure
//! 8.3 model test; this ECU-aware variant is the session/command path).

use crate::connection::Session;
use crate::dto::FieldDiffDto;
use crate::events::TuneDirtyEvent;
use crate::session::{dirty_event, fmt_model_err, page_deltas, write_deltas, NO_TUNE};
use opentune_model::{MergePick, Value};

const NO_SNAPSHOT: &str = "no snapshot to diff against — call snapshot_tune first";

impl Session {
    /// Snapshot the current tune as the diff/merge baseline (the "other"
    /// side). M2 has no file import (M6); this lets the diff/merge UI
    /// compare the live tune against a saved-in-memory copy of itself taken
    /// at an earlier point (e.g. right after `load_tune`, before edits).
    pub fn snapshot_tune(&mut self) -> Result<(), String> {
        let tune = self.tune.as_ref().ok_or_else(|| NO_TUNE.to_string())?;
        self.snapshot = Some(tune.clone());
        Ok(())
    }

    /// Diff the current tune against the snapshot baseline.
    pub fn diff_tune(&self) -> Result<Vec<FieldDiffDto>, String> {
        let tune = self.tune.as_ref().ok_or_else(|| NO_TUNE.to_string())?;
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| NO_SNAPSHOT.to_string())?;
        Ok(opentune_model::diff(tune, snapshot)
            .into_iter()
            .map(FieldDiffDto::from)
            .collect())
    }

    /// Merge picked constants from the snapshot baseline into the current
    /// tune, writing each accepted pick live to the ECU before committing it
    /// to the model (see module doc for why this is per-pick, not batched).
    ///
    /// A pick whose *write* to the ECU fails halts the merge: prior picks in
    /// this call are already committed (tune == ECU for them) and are left
    /// as-is; the failing pick and any remaining ones are not applied.
    pub fn merge_tune(&mut self, picks: &[String]) -> Result<TuneDirtyEvent, String> {
        let picks: Vec<_> = picks.iter().cloned().map(MergePick::All).collect();
        self.merge_picks(&picks)
    }

    /// Merge complete fields or selected array cells from the snapshot.
    ///
    /// Each pick is independently validated and committed to the wire before
    /// the in-memory tune changes. A `Cells` pick becomes one
    /// `Tune::set_cells` call, so one UI gesture remains one undo step and
    /// one contiguous wire delta — and, because `set_cells` validates only
    /// the touched indices, an untouched cell that is legitimately outside
    /// the constant's current `[low, high]` range (stale tune vs. a newer/
    /// tighter INI; `Tune::load_page` never range-checks) can never block a
    /// pick that edits a *different*, in-range cell. Mirrors
    /// `opentune_model::merge_picks`'s model-level fix for the same reason.
    pub fn merge_picks(&mut self, picks: &[MergePick]) -> Result<TuneDirtyEvent, String> {
        let Session {
            conn,
            def,
            tune,
            snapshot,
            ..
        } = self;
        let tune = tune.as_mut().ok_or_else(|| NO_TUNE.to_string())?;
        let snapshot = snapshot.as_ref().ok_or_else(|| NO_SNAPSHOT.to_string())?;

        for pick in picks {
            let name = pick.name();
            match pick {
                MergePick::All(_) => {
                    let Ok(value) = snapshot.get(name) else {
                        continue; // unresolvable on the snapshot -- nothing to merge
                    };
                    let mut probe = tune.clone();
                    if probe.set(name, value.clone()).is_err() {
                        continue; // rejected pick -- skip rather than abort the batch
                    }
                    // Wire the pick only when connected; offline sessions
                    // merge into the model alone (same rule as
                    // `Session::set_value`).
                    if let Some(conn) = conn.as_ref() {
                        let deltas = page_deltas(tune, &probe, &def.pages);
                        write_deltas(conn, &def.comms, &deltas)?;
                    }
                    tune.set(name, value).map_err(fmt_model_err)?;
                }
                MergePick::Cells { indices, .. } => {
                    let (Ok(Value::Array(current)), Ok(Value::Array(incoming))) =
                        (tune.get(name), snapshot.get(name))
                    else {
                        continue;
                    };
                    let cells: Vec<(u32, f64)> = indices
                        .iter()
                        .filter_map(|&index| {
                            let &src = incoming.get(index)?;
                            let &dst = current.get(index)?;
                            if dst == src {
                                return None;
                            }
                            u32::try_from(index).ok().map(|i| (i, src))
                        })
                        .collect();
                    if cells.is_empty() {
                        continue; // no in-bounds, changed cell -- nothing to merge
                    }
                    let mut probe = tune.clone();
                    if probe.set_cells(name, &cells).is_err() {
                        continue; // rejected pick -- skip rather than abort the batch
                    }
                    if let Some(conn) = conn.as_ref() {
                        let deltas = page_deltas(tune, &probe, &def.pages);
                        write_deltas(conn, &def.comms, &deltas)?;
                    }
                    tune.set_cells(name, &cells).map_err(fmt_model_err)?;
                }
            }
        }
        Ok(dirty_event(tune))
    }
}

// ── Unit tests (against the simulator) ───────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{connect_simulator, load_definition_from_str, ActiveConnection};
    use opentune_model::Value;
    use opentune_protocol::{MsProtocol, Protocol};
    use opentune_transport::Transport;
    use std::sync::Arc;

    const BUNDLED_INI: &str = include_str!("../resources/speeduino.sample.ini");

    fn session() -> Session {
        let def = Arc::new(load_definition_from_str(BUNDLED_INI).expect("bundled INI parses"));
        let conn = connect_simulator(&def, &|_| {}).expect("simulator connects");
        Session {
            conn: Some(conn),
            def,
            tune: None,
            snapshot: None,
            offline_origin: false,
        }
    }

    /// Read a page straight off the ECU (bypassing the tune) — the "reached
    /// the wire" oracle, same pattern as `session.rs`'s own tests.
    fn ecu_page(session: &Session, number: u16) -> Vec<u8> {
        let Some(ActiveConnection::Sim { simulator, .. }) = &session.conn else {
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
    fn snapshot_diff_and_merge_reach_the_ecu() {
        let mut s = session();
        s.load_tune().unwrap();
        s.snapshot_tune().expect("snapshot the freshly loaded tune");

        // Edit the live tune away from the snapshot baseline.
        s.set_value("reqFuel", Value::Scalar(12.5)).unwrap();
        // reqFuel = U16 LE @ offset 0, scale 0.1 -> raw 125.
        assert_eq!(&ecu_page(&s, 1)[0..2], &[125, 0]);

        let diffs = s.diff_tune().expect("diff against the snapshot");
        let req = diffs
            .iter()
            .find(|d| d.name == "reqFuel")
            .expect("reqFuel differs");
        assert_eq!(req.a, Value::Scalar(12.5), "current side is the live edit");
        assert_eq!(
            req.b,
            Value::Scalar(0.0),
            "other side is the snapshot baseline"
        );

        // Merge the pick back to the snapshot's (pre-edit) value. That value
        // equals the flash baseline (0,0) here — nothing was burned — so the
        // merge returns the page to baseline and the tune reads clean. Dirty
        // is byte-equality against flash, not a sticky edit flag.
        let ev = s
            .merge_tune(&["reqFuel".to_string()])
            .expect("merge reqFuel");
        assert!(!ev.dirty);
        assert_eq!(
            &ecu_page(&s, 1)[0..2],
            &[0, 0],
            "merge wrote the snapshot's raw bytes to the ECU RAM"
        );
        assert_eq!(
            s.read_values(&["reqFuel".to_string()]).unwrap(),
            vec![Value::Scalar(0.0)],
            "the tune model reflects the merged value"
        );

        // Diffing again shows nothing left for the merged constant.
        let diffs = s.diff_tune().unwrap();
        assert!(
            !diffs.iter().any(|d| d.name == "reqFuel"),
            "the merged constant no longer differs"
        );
    }

    #[test]
    fn merge_leaves_un_picked_differences_untouched() {
        let mut s = session();
        s.load_tune().unwrap();
        s.snapshot_tune().unwrap();

        s.set_value("reqFuel", Value::Scalar(12.5)).unwrap();
        s.set_value("injLayout", Value::Enum(3)).unwrap();

        s.merge_tune(&["reqFuel".to_string()]).unwrap();

        let diffs = s.diff_tune().unwrap();
        assert!(
            !diffs.iter().any(|d| d.name == "reqFuel"),
            "picked constant merged away"
        );
        assert!(
            diffs.iter().any(|d| d.name == "injLayout"),
            "un-picked constant still differs"
        );
    }

    #[test]
    fn merged_edit_is_undoable() {
        let mut s = session();
        s.load_tune().unwrap();
        s.snapshot_tune().unwrap();
        s.set_value("reqFuel", Value::Scalar(12.5)).unwrap();

        s.merge_tune(&["reqFuel".to_string()]).unwrap();
        assert_eq!(
            s.read_values(&["reqFuel".to_string()]).unwrap(),
            vec![Value::Scalar(0.0)]
        );

        let ev = s.undo().expect("undo the merge");
        assert_eq!(
            s.read_values(&["reqFuel".to_string()]).unwrap(),
            vec![Value::Scalar(12.5)],
            "undo reverts the merge exactly like a manual edit"
        );
        assert!(ev.dirty);
    }

    /// M2/M3 review fix wave item 2, session layer. `afrTable`'s declared
    /// range is `[7.0, 25.5]` (`resources/speeduino.sample.ini`) — an
    /// all-zero page decodes every cell to `0.0`, well outside that range.
    /// `merge_picks`'s `Cells` arm must not let an untouched out-of-range
    /// cell block a pick that only touches a different, in-range cell —
    /// mirrors the model-level `merge_cells_pick_lands_despite_an_untouched
    /// _out_of_range_cell` test (`crates/model/tests/diff.rs`).
    #[test]
    fn merge_cells_pick_lands_despite_an_untouched_out_of_range_snapshot_cell() {
        let mut s = session();
        s.load_tune().unwrap();

        // Force every afrTable cell to 0.0 (out of range) directly on the
        // page bytes — a stale tune vs. a newer/tighter INI is a legitimate
        // real-world state, since `Tune::load_page` never range-checks.
        s.tune.as_mut().unwrap().load_page(3, vec![0u8; 288]);
        s.snapshot_tune().unwrap();

        // The snapshot's cell 7 becomes the one in-range, differing value
        // to pick. `Tune::set_cells` (already fixed, M4) only validates the
        // touched cell, so this setup step is unaffected by the rest of the
        // array being out of range.
        s.snapshot
            .as_mut()
            .unwrap()
            .set_cells("afrTable", &[(7, 20.0)])
            .unwrap();

        s.merge_picks(&[MergePick::Cells {
            name: "afrTable".to_string(),
            indices: vec![7],
        }])
        .expect("merge_picks");

        let Value::Array(after) = s.tune.as_ref().unwrap().get("afrTable").unwrap() else {
            panic!("expected an array");
        };
        assert_eq!(after[7], 20.0, "the picked in-range cell lands");
        assert_eq!(
            after[5], 0.0,
            "an untouched out-of-range cell is unaffected"
        );
        assert_eq!(
            ecu_page(&s, 3)[7],
            200, // raw = 20.0 / scale(0.1)
            "the picked cell's delta reached the ECU too"
        );
    }

    #[test]
    fn diff_and_merge_without_a_snapshot_error() {
        let mut s = session();
        s.load_tune().unwrap();
        assert!(s.diff_tune().is_err());
        assert!(s.merge_tune(&["reqFuel".to_string()]).is_err());
    }

    #[test]
    fn snapshot_without_a_tune_errors() {
        let mut s = session();
        assert!(s.snapshot_tune().is_err());
    }
}
