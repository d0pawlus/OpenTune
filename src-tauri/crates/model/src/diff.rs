// SPDX-License-Identifier: GPL-3.0-or-later
//! Tune diff / merge (Task 8) — compares two [`Tune`]s built from the same
//! [`Definition`](opentune_ini::Definition) and selectively applies picked
//! constants from one into the other.
//!
//! **Precondition:** `diff`/`merge` are only meaningful when `a`/`base` and
//! `b`/`incoming` share the same definition (same constants, same shapes) —
//! typically two [`Tune`]s built via `Tune::new(Arc::clone(&def))` against
//! one `Arc<Definition>`, e.g. a live tune and an in-memory snapshot of it
//! taken earlier (file-based `.msq` diff is M6). `diff` walks `a`'s
//! constants and looks each one up by name in `b`; a `Definition` mismatch
//! degrades gracefully (see below) rather than panicking, but produces a
//! diff that is only meaningful under the shared-definition precondition.

use crate::tune::Tune;
use crate::value::Value;

/// One constant whose value differs between two tunes.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDiff {
    /// The constant's name.
    pub name: String,
    /// The value in the first (`a`, "current") tune.
    pub a: Value,
    /// The value in the second (`b`, "other") tune.
    pub b: Value,
    /// For `Array`/table constants, the per-element differences — only the
    /// indices whose value actually changed, not a full per-cell dump.
    /// Empty for scalar/enum/text constants (which are diffed as a whole).
    pub cells: Vec<CellDiff>,
}

/// A single differing element within an `Array`/table constant.
#[derive(Debug, Clone, PartialEq)]
pub struct CellDiff {
    /// The element's row-major index into the constant's flattened array
    /// (`Value::Array` is already row-major, so this indexes it directly).
    pub index: usize,
    /// The element's value in `a`.
    pub a: f64,
    /// The element's value in `b`.
    pub b: f64,
}

/// One selective merge request.
///
/// `All` replaces the complete value (scalar, enum, text, or array).
/// `Cells` applies only the named array indices from the incoming tune,
/// preserving every unselected cell in the base tune.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergePick {
    All(String),
    Cells { name: String, indices: Vec<usize> },
}

impl MergePick {
    /// The constant targeted by this pick.
    pub fn name(&self) -> &str {
        match self {
            Self::All(name) | Self::Cells { name, .. } => name,
        }
    }
}

/// Compare two tunes and return one [`FieldDiff`] per constant whose value
/// differs, in `Definition::constants` order.
///
/// A constant is **skipped** (produces no `FieldDiff`) when:
/// - both sides resolve to the same [`Value`] (nothing changed), or
/// - `Tune::get` errors on **either** side.
///
/// The get-errors-on-either-side case covers two situations: an
/// `Expr`-scaled constant that can't resolve (e.g. references an unknown
/// PC variable) fails identically on both `a` and `b` under the documented
/// same-`Definition` precondition — there's no meaningful "before/after" to
/// report for a value neither side can compute, so it's skipped rather than
/// surfaced as a diagnostic (a diff view is not the place to surface a
/// definition-level parse problem). A one-sided error can't arise when the
/// precondition holds (both tunes see the same constant, same expression);
/// if it *does* arise (a caller violates the precondition), skipping is the
/// safe degrade — never panics, never fabricates a `Value` for the side
/// that failed.
pub fn diff(a: &Tune, b: &Tune) -> Vec<FieldDiff> {
    a.def
        .constants
        .iter()
        .filter_map(|c| {
            let av = a.get(&c.name).ok()?;
            let bv = b.get(&c.name).ok()?;
            (av != bv).then(|| FieldDiff {
                name: c.name.clone(),
                cells: cell_diffs(&av, &bv),
                a: av,
                b: bv,
            })
        })
        .collect()
}

/// Per-index differences between two array values; empty for any other
/// [`Value`] kind. Positions beyond the shorter side's length are ignored —
/// under the shared-`Definition` precondition the two arrays are always the
/// same length.
fn cell_diffs(a: &Value, b: &Value) -> Vec<CellDiff> {
    let (Value::Array(a), Value::Array(b)) = (a, b) else {
        return Vec::new();
    };
    a.iter()
        .zip(b.iter())
        .enumerate()
        .filter_map(|(index, (&a, &b))| (a != b).then_some(CellDiff { index, a, b }))
        .collect()
}

/// Apply the named `picks` from `incoming` onto `base`, via [`Tune::set`] —
/// each successful pick records a normal undo [`Edit`](crate::edit::Edit) on
/// `base`, so merged changes are undoable exactly like a manual edit.
///
/// Un-picked constants (including ones that differ between `base` and
/// `incoming`) are left untouched. A pick is silently skipped when
/// `incoming.get(name)` errors (unknown/unresolvable) or when
/// `base.set(name, value)` rejects it (e.g. out-of-range, type mismatch) —
/// the frozen signature returns nothing, so there is no per-pick error
/// channel; callers that need to know what actually landed can `diff` again
/// after merging. This mirrors `Tune::set`'s own "no partial write" — a
/// rejected pick changes no bytes.
pub fn merge(base: &mut Tune, incoming: &Tune, picks: &[String]) {
    let picks: Vec<_> = picks.iter().cloned().map(MergePick::All).collect();
    merge_picks(base, incoming, &picks);
}

/// Apply field- or cell-level picks from `incoming` to `base`.
///
/// Invalid names and non-array `Cells` picks degrade per pick: an `All` pick
/// on an unresolvable/rejected name is a no-op; a `Cells` pick on a
/// non-array or unknown name is a no-op too. Out-of-bounds cell indices
/// within a valid `Cells` pick are skipped individually — the pick's other,
/// in-bounds indices still land.
///
/// `Cells` picks are applied through [`Tune::set_cells`], which validates
/// and encodes only the touched indices. This matters because `base` may
/// legitimately hold cells outside the constant's current `[low, high]`
/// range (a stale tune vs. a newer/tighter INI — `Tune::load_page` never
/// range-checks): such an untouched cell must never block a pick that edits
/// a *different*, in-range cell. Whole-array revalidation (the old
/// `base.set(name, whole_array)` path) would range-check every element,
/// including untouched ones, and silently drop the entire pick.
pub fn merge_picks(base: &mut Tune, incoming: &Tune, picks: &[MergePick]) {
    for pick in picks {
        let name = pick.name();
        match pick {
            MergePick::All(_) => {
                let Ok(value) = incoming.get(name) else {
                    continue;
                };
                let _ = base.set(name, value);
            }
            MergePick::Cells { indices, .. } => {
                let Some(cells) = changed_cells(base, incoming, name, indices) else {
                    continue;
                };
                if cells.is_empty() {
                    continue;
                }
                let _ = base.set_cells(name, &cells);
            }
        }
    }
}

/// The `(index, value)` pairs among `indices` where `incoming`'s array cell
/// differs from `base`'s — ready to hand to [`Tune::set_cells`]. `None` when
/// `name` doesn't resolve to an array on both tunes (unknown name, type
/// mismatch, or the shared-`Definition` precondition doesn't hold). Indices
/// out of bounds on either side, or that don't fit in a `u32`, are skipped
/// rather than failing the whole pick.
fn changed_cells(
    base: &Tune,
    incoming: &Tune,
    name: &str,
    indices: &[usize],
) -> Option<Vec<(u32, f64)>> {
    let (Value::Array(current), Value::Array(incoming)) =
        (base.get(name).ok()?, incoming.get(name).ok()?)
    else {
        return None;
    };
    Some(
        indices
            .iter()
            .filter_map(|&index| {
                let &src = incoming.get(index)?;
                let &dst = current.get(index)?;
                if dst == src {
                    return None;
                }
                u32::try_from(index).ok().map(|i| (i, src))
            })
            .collect(),
    )
}
