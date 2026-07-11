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
use crate::session::{dirty_event, page_deltas, write_deltas, NO_TUNE};
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
    /// the in-memory tune changes. A cell pick becomes one array `Tune::set`,
    /// so one UI gesture remains one undo step and one contiguous wire delta.
    pub fn merge_picks(&mut self, picks: &[MergePick]) -> Result<TuneDirtyEvent, String> {
        let Session {
            conn,
            def,
            tune,
            snapshot,
        } = self;
        let tune = tune.as_mut().ok_or_else(|| NO_TUNE.to_string())?;
        let snapshot = snapshot.as_ref().ok_or_else(|| NO_SNAPSHOT.to_string())?;

        for pick in picks {
            let name = pick.name();
            let value = match pick {
                MergePick::All(_) => snapshot.get(name),
                MergePick::Cells { indices, .. } => {
                    let (Ok(Value::Array(mut current)), Ok(Value::Array(incoming))) =
                        (tune.get(name), snapshot.get(name))
                    else {
                        continue;
                    };
                    let mut changed = false;
                    for &index in indices {
                        let (Some(dst), Some(&src)) = (current.get_mut(index), incoming.get(index))
                        else {
                            continue;
                        };
                        if *dst != src {
                            *dst = src;
                            changed = true;
                        }
                    }
                    if !changed {
                        continue;
                    }
                    Ok(Value::Array(current))
                }
            };
            let Ok(value) = value else {
                continue; // unresolvable on the snapshot -- nothing to merge
            };
            let mut probe = tune.clone();
            if probe.set(name, value.clone()).is_err() {
                continue; // rejected pick -- skip rather than abort the batch
            }
            let deltas = page_deltas(tune, &probe, &def.pages);
            write_deltas(conn, &def.comms, &deltas)?;
            *tune = probe;
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
            conn,
            def,
            tune: None,
            snapshot: None,
        }
    }

    /// Read a page straight off the ECU (bypassing the tune) — the "reached
    /// the wire" oracle, same pattern as `session.rs`'s own tests.
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

        // Merge the pick back to the snapshot's (pre-edit) value.
        let ev = s
            .merge_tune(&["reqFuel".to_string()])
            .expect("merge reqFuel");
        assert!(ev.dirty);
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
