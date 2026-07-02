// SPDX-License-Identifier: GPL-3.0-or-later
//! Task 8 tests — `diff`/`merge` over the M2 `Tune` model.
//!
//! `diff`/`merge` are meaningful when both tunes are built from the same
//! shape of `Definition` (documented precondition on `crate::diff`); most
//! tests below build two independent `Tune`s from equal-but-distinct
//! `Definition`s (via `common::tune`, called twice) to prove that structural
//! equality is enough — literal `Arc` sharing is not required.

mod common;

use common::{scalar, tune};
use opentune_ini::{ConstantKind, Endianness, Number, ScalarType, Shape};
use opentune_model::{diff, merge, CellDiff, Value};

/// A 2x2 `U08` array constant ("map") at offset 8, scale 1.0.
fn table_const() -> opentune_ini::ConstantDef {
    let mut c = scalar("map", ScalarType::U08, 8, 1.0, 0.0, 255.0);
    c.kind = ConstantKind::Array {
        elem: ScalarType::U08,
        shape: Shape { rows: 2, cols: 2 },
    };
    c
}

// ---------- 8.1 / 8.2 — diff ----------

#[test]
fn diffs_scalar_and_table() {
    let rpm = scalar("rpmK", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    let map = table_const();
    let steady = scalar("steady", ScalarType::U08, 20, 1.0, 0.0, 255.0);

    let mut a = tune(
        Endianness::Little,
        vec![rpm.clone(), map.clone(), steady.clone()],
    );
    a.set("rpmK", Value::Scalar(10.0)).unwrap();
    a.set("map", Value::Array(vec![1.0, 2.0, 3.0, 4.0]))
        .unwrap();

    let mut b = tune(Endianness::Little, vec![rpm, map, steady]);
    b.set("rpmK", Value::Scalar(20.0)).unwrap(); // scalar differs
    b.set("map", Value::Array(vec![1.0, 9.0, 3.0, 8.0]))
        .unwrap(); // cells 1 and 3 differ; 0 and 2 match
                   // `steady` is left at its zeroed default on both sides — unchanged.

    let diffs = diff(&a, &b);
    assert_eq!(
        diffs.len(),
        2,
        "only the two differing constants, nothing for `steady`: {diffs:?}"
    );

    let rpm_diff = diffs.iter().find(|d| d.name == "rpmK").unwrap();
    assert_eq!(rpm_diff.a, Value::Scalar(10.0));
    assert_eq!(rpm_diff.b, Value::Scalar(20.0));
    assert!(
        rpm_diff.cells.is_empty(),
        "a scalar diff carries no per-cell breakdown"
    );

    let map_diff = diffs.iter().find(|d| d.name == "map").unwrap();
    assert_eq!(map_diff.a, Value::Array(vec![1.0, 2.0, 3.0, 4.0]));
    assert_eq!(map_diff.b, Value::Array(vec![1.0, 9.0, 3.0, 8.0]));
    assert_eq!(
        map_diff.cells,
        vec![
            CellDiff {
                index: 1,
                a: 2.0,
                b: 9.0
            },
            CellDiff {
                index: 3,
                a: 4.0,
                b: 8.0
            },
        ],
        "row-major cell indices for only the changed elements"
    );
}

#[test]
fn identical_tunes_diff_to_nothing() {
    let rpm = scalar("rpmK", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    let mut a = tune(Endianness::Little, vec![rpm.clone()]);
    a.set("rpmK", Value::Scalar(42.0)).unwrap();
    let mut b = tune(Endianness::Little, vec![rpm]);
    b.set("rpmK", Value::Scalar(42.0)).unwrap();

    assert!(diff(&a, &b).is_empty());
}

#[test]
fn unresolvable_expr_constants_are_skipped_on_both_sides() {
    // Both `a` and `b` are built from the same constant list, so the broken
    // `Expr` scale fails identically on both sides -- there is no
    // meaningful before/after to report, so `diff` skips it rather than
    // surfacing a definition-level diagnostic through the diff view.
    let mut broken = scalar("broken", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    broken.scale = Number::Expr("bogusVar * 2".to_string());
    let a = tune(Endianness::Little, vec![broken.clone()]);
    let b = tune(Endianness::Little, vec![broken]);

    assert!(diff(&a, &b).is_empty());
}

#[test]
fn a_side_only_error_is_skipped_without_panicking() {
    // Defensive coverage: deliberately violates the same-`Definition`
    // precondition (broken scale on `a`'s constant, literal scale on `b`'s)
    // to prove `diff` degrades safely -- skip, never panic, never fabricate
    // a `Value` for the side that failed to resolve.
    let mut broken = scalar("c", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    broken.scale = Number::Expr("bogusVar".to_string());
    let fine = scalar("c", ScalarType::U08, 0, 1.0, 0.0, 255.0);

    let a = tune(Endianness::Little, vec![broken]);
    let b = tune(Endianness::Little, vec![fine]);

    assert!(diff(&a, &b).is_empty());
}

// ---------- 8.3 — merge ----------

#[test]
fn merge_selected_applies_only_the_picked_constant() {
    let rpm = scalar("rpmK", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    let clt = scalar("clt", ScalarType::U08, 1, 1.0, 0.0, 255.0);

    let mut base = tune(Endianness::Little, vec![rpm.clone(), clt.clone()]);
    base.set("rpmK", Value::Scalar(10.0)).unwrap();
    base.set("clt", Value::Scalar(50.0)).unwrap();

    let mut incoming = tune(Endianness::Little, vec![rpm, clt]);
    incoming.set("rpmK", Value::Scalar(20.0)).unwrap();
    incoming.set("clt", Value::Scalar(90.0)).unwrap();

    merge(&mut base, &incoming, &["rpmK".to_string()]);

    assert_eq!(
        base.get("rpmK").unwrap(),
        Value::Scalar(20.0),
        "the picked constant is merged"
    );
    assert_eq!(
        base.get("clt").unwrap(),
        Value::Scalar(50.0),
        "the un-picked (but differing) constant is left alone"
    );
}

#[test]
fn merge_records_an_undoable_edit_on_base() {
    let rpm = scalar("rpmK", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    let mut base = tune(Endianness::Little, vec![rpm.clone()]);
    base.set("rpmK", Value::Scalar(10.0)).unwrap();

    let mut incoming = tune(Endianness::Little, vec![rpm]);
    incoming.set("rpmK", Value::Scalar(20.0)).unwrap();

    merge(&mut base, &incoming, &["rpmK".to_string()]);
    assert_eq!(base.get("rpmK").unwrap(), Value::Scalar(20.0));

    assert!(base.undo(), "merge's `Tune::set` recorded a normal edit");
    assert_eq!(
        base.get("rpmK").unwrap(),
        Value::Scalar(10.0),
        "undo reverts a merged pick exactly like a manual edit"
    );
}

#[test]
fn merge_skips_an_unknown_pick_without_touching_base() {
    // `base` starts clean (no `.set()` calls) so `base.undo()` can only
    // succeed if `merge` itself recorded an edit.
    let rpm = scalar("rpmK", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    let mut base = tune(Endianness::Little, vec![rpm.clone()]);
    let incoming = tune(Endianness::Little, vec![rpm]);

    merge(&mut base, &incoming, &["ghost".to_string()]);

    assert_eq!(
        base.get("rpmK").unwrap(),
        Value::Scalar(0.0),
        "an unresolvable pick name is a no-op, not a panic"
    );
    assert!(!base.undo(), "the no-op pick recorded no edit");
}
