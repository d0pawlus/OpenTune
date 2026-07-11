// SPDX-License-Identifier: GPL-3.0-or-later
//! Task 4 tests — `Tune` scaled accessors (sub-steps 4.1/4.2) and the
//! error surface (4.5). Dirty/flash and undo/redo tests live in
//! `tune_state.rs`.

mod common;

use common::{load1, scalar, tune, PAGE_SIZE};
use opentune_ini::{ConstantKind, Endianness, Number, ScalarType, Shape};
use opentune_model::{ModelError, Tune, Value};

// ---------- 4.1 / 4.2 — scaled get/set roundtrip ----------

#[test]
fn get_set_roundtrip() {
    let mut t = tune(
        Endianness::Little,
        vec![scalar("rpmK", ScalarType::U08, 0, 0.1, 0.0, 25.5)],
    );
    load1(&mut t, &[123]);
    assert_eq!(t.get("rpmK").unwrap(), Value::Scalar(123.0 * 0.1));

    t.set("rpmK", Value::Scalar(20.0)).unwrap();
    assert_eq!(t.page_bytes(1)[0], 200);
    assert_eq!(t.get("rpmK").unwrap(), Value::Scalar(200.0 * 0.1));
}

fn scalar_cases_little_endian() -> Vec<(ScalarType, f64, Vec<u8>)> {
    vec![
        (ScalarType::U08, 200.0, vec![200]),
        (ScalarType::S08, -5.0, vec![0xFB]),
        (ScalarType::U16, 4660.0, vec![0x34, 0x12]),
        (ScalarType::S16, -300.0, vec![0xD4, 0xFE]),
        (ScalarType::U32, 305_419_896.0, vec![0x78, 0x56, 0x34, 0x12]),
        (ScalarType::S32, -2.0, vec![0xFE, 0xFF, 0xFF, 0xFF]),
        (ScalarType::F32, 3.5, 3.5f32.to_le_bytes().to_vec()),
    ]
}

#[test]
fn every_scalar_type_roundtrips_exact_bytes_little_endian() {
    for (ty, physical, expected) in scalar_cases_little_endian() {
        let mut t = tune(
            Endianness::Little,
            vec![scalar("c", ty, 4, 1.0, -4.0e9, 4.0e9)],
        );
        t.set("c", Value::Scalar(physical)).unwrap();
        assert_eq!(
            &t.page_bytes(1)[4..4 + expected.len()],
            &expected[..],
            "{ty:?}"
        );
        assert_eq!(t.get("c").unwrap(), Value::Scalar(physical), "{ty:?}");
    }
}

#[test]
fn every_scalar_type_roundtrips_exact_bytes_big_endian() {
    for (ty, physical, mut expected) in scalar_cases_little_endian() {
        expected.reverse();
        let mut t = tune(
            Endianness::Big,
            vec![scalar("c", ty, 4, 1.0, -4.0e9, 4.0e9)],
        );
        t.set("c", Value::Scalar(physical)).unwrap();
        assert_eq!(
            &t.page_bytes(1)[4..4 + expected.len()],
            &expected[..],
            "{ty:?}"
        );
        assert_eq!(t.get("c").unwrap(), Value::Scalar(physical), "{ty:?}");
    }
}

#[test]
fn write_rounds_half_away_from_zero() {
    let mut t = tune(
        Endianness::Little,
        vec![scalar("c", ScalarType::S08, 0, 0.5, -10.0, 10.0)],
    );
    t.set("c", Value::Scalar(2.25)).unwrap(); // raw 4.5 -> 5
    assert_eq!(t.page_bytes(1)[0], 5);
    assert_eq!(t.get("c").unwrap(), Value::Scalar(2.5));
    t.set("c", Value::Scalar(-2.25)).unwrap(); // raw -4.5 -> -5
    assert_eq!(t.page_bytes(1)[0], 0xFB);
}

#[test]
fn translate_applies_on_read_and_inverts_on_write() {
    let mut clt = scalar("clt", ScalarType::U08, 0, 1.0, -40.0, 215.0);
    clt.translate = Number::Lit(-40.0);
    let mut t = tune(Endianness::Little, vec![clt]);
    load1(&mut t, &[100]);
    assert_eq!(t.get("clt").unwrap(), Value::Scalar(60.0));
    t.set("clt", Value::Scalar(0.0)).unwrap(); // raw = (0 - -40) / 1 = 40
    assert_eq!(t.page_bytes(1)[0], 40);
}

#[test]
fn array_reads_and_writes_element_wise() {
    let mut c = scalar("map", ScalarType::U08, 8, 0.5, 0.0, 100.0);
    c.kind = ConstantKind::Array {
        elem: ScalarType::U08,
        shape: Shape { rows: 2, cols: 2 },
    };
    let mut t = tune(Endianness::Little, vec![c]);
    let mut page = vec![0u8; PAGE_SIZE];
    page[8..12].copy_from_slice(&[2, 4, 6, 8]);
    t.load_page(1, page);
    assert_eq!(
        t.get("map").unwrap(),
        Value::Array(vec![1.0, 2.0, 3.0, 4.0])
    );

    t.set("map", Value::Array(vec![4.0, 3.0, 2.0, 1.0]))
        .unwrap();
    assert_eq!(&t.page_bytes(1)[8..12], &[8, 6, 4, 2]);
}

#[test]
fn bits_extract_and_mask_preserving_neighbors() {
    let mut c = scalar("algo", ScalarType::U08, 0, 1.0, 0.0, 0.0);
    c.kind = ConstantKind::Bits {
        storage: ScalarType::U08,
        bit_lo: 4,
        bit_hi: 7,
        options: vec![],
    };
    let mut t = tune(Endianness::Little, vec![c]);
    load1(&mut t, &[0xA3]);
    assert_eq!(t.get("algo").unwrap(), Value::Enum(0xA));

    t.set("algo", Value::Enum(3)).unwrap();
    assert_eq!(t.page_bytes(1)[0], 0x33); // low nibble preserved
    assert_eq!(t.get("algo").unwrap(), Value::Enum(3));
}

#[test]
fn bits_in_u16_storage_respect_endianness() {
    let mut c = scalar("flags", ScalarType::U16, 0, 1.0, 0.0, 0.0);
    c.kind = ConstantKind::Bits {
        storage: ScalarType::U16,
        bit_lo: 8,
        bit_hi: 11,
        options: vec![],
    };
    let mut t = tune(Endianness::Little, vec![c]);
    load1(&mut t, &[0x34, 0x12]); // pattern 0x1234
    assert_eq!(t.get("flags").unwrap(), Value::Enum(2));

    t.set("flags", Value::Enum(0xF)).unwrap();
    assert_eq!(&t.page_bytes(1)[0..2], &[0x34, 0x1F]); // pattern 0x1F34
}

#[test]
fn text_roundtrips_with_zero_padding() {
    let mut c = scalar("note", ScalarType::U08, 0, 1.0, 0.0, 0.0);
    c.kind = ConstantKind::Text { len: 8 };
    let mut t = tune(Endianness::Little, vec![c]);
    t.set("note", Value::Text("ABC".to_string())).unwrap();
    assert_eq!(&t.page_bytes(1)[0..8], b"ABC\0\0\0\0\0");
    assert_eq!(t.get("note").unwrap(), Value::Text("ABC".to_string()));
}

#[test]
fn expr_scale_resolves_via_other_constant() {
    let load_div = scalar("loadDiv", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    let mut fuel = scalar("fuelLoad", ScalarType::U08, 1, 1.0, 0.0, 255.0);
    fuel.scale = Number::Expr("loadDiv / 10".to_string());
    let mut t = tune(Endianness::Little, vec![load_div, fuel]);
    load1(&mut t, &[5, 10]); // loadDiv = 5 -> fuelLoad scale = 0.5
    assert_eq!(t.get("fuelLoad").unwrap(), Value::Scalar(5.0));

    t.set("fuelLoad", Value::Scalar(20.0)).unwrap(); // raw = 20 / 0.5 = 40
    assert_eq!(t.page_bytes(1)[1], 40);
}

#[test]
fn expr_scale_uses_referenced_constants_physical_value_not_raw_byte() {
    // Speeduino's `scale = { 0.1 / stoich }` pattern: `stoich` is itself
    // scaled (U08, scale=0.1), so its *physical* value (~14.7, the
    // stoichiometric ratio) must feed the expression -- not its raw storage
    // byte (147). With non-unity scale on the referenced constant, physical
    // and raw give a 10x-different (and here range-discriminating) result,
    // pinning the physical-value contract documented on
    // `Tune::lookup_expr_var`.
    let stoich = scalar("stoich", ScalarType::U08, 0, 0.1, 0.0, 25.5);
    let mut afr = scalar("afr", ScalarType::U08, 1, 1.0, 0.0, 100.0);
    afr.scale = Number::Expr("0.1 / stoich".to_string());
    let mut t = tune(Endianness::Little, vec![stoich, afr]);

    // ---- read path ----
    load1(&mut t, &[147, 100]); // stoich raw 147 -> physical 14.7; afr raw 100
    let Value::Scalar(actual) = t.get("afr").unwrap() else {
        panic!("expected a scalar value");
    };

    // Physical-based expectation: afr's scale resolves to `0.1 / 14.7`
    // (stoich's physical value), so afr's physical value is
    // `100 * (0.1 / 14.7)`. Floats: compare with an epsilon rather than `==`.
    let physical_based = 100.0 * (0.1 / 14.7);
    assert!(
        (actual - physical_based).abs() < 1e-9,
        "expected physical-based value {physical_based}, got {actual}"
    );

    // If `stoich`'s *raw* byte (147) were used instead of its physical
    // value, the scale would be `0.1 / 147` -- a full order of magnitude
    // smaller -- giving a clearly different (wrong) result.
    let raw_based = 100.0 * (0.1 / 147.0);
    assert!(
        (actual - raw_based).abs() > 0.1,
        "value {actual} should discriminate from the raw-based (buggy) {raw_based}"
    );

    // ---- write path ----
    // With the physical-based scale (0.1 / 14.7 ~= 0.0068027), a physical
    // value of 0.5 inverse-scales to raw 73.5 -> rounds to 74 (fits U08).
    // The raw-based (buggy) scale (0.1 / 147 ~= 0.00068027) would instead
    // require raw 735, which does not fit a U08 byte at all.
    t.set("afr", Value::Scalar(0.5)).unwrap();
    assert_eq!(t.page_bytes(1)[1], 74);
}

#[test]
fn unresolvable_expr_is_error_not_panic() {
    let mut c = scalar("c", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    c.scale = Number::Expr("bogusVar * 2".to_string());
    let t = tune(Endianness::Little, vec![c]);
    assert!(matches!(t.get("c"), Err(ModelError::UnresolvedExpr(_))));
}

#[test]
fn expr_lookup_does_not_recurse_into_expr_scaled_constants() {
    let mut a = scalar("a", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    a.scale = Number::Expr("b".to_string());
    let mut b = scalar("b", ScalarType::U08, 1, 1.0, 0.0, 255.0);
    b.scale = Number::Expr("1 + 1".to_string());
    let t = tune(Endianness::Little, vec![a, b]);
    // `b` is Expr-scaled, so the (deliberately Lit-only) lookup refuses to
    // resolve it — bounded, no recursion, surfaced as a diagnostic error.
    assert!(matches!(t.get("a"), Err(ModelError::UnresolvedExpr(_))));
}

#[test]
fn lit_scaled_get_ignores_broken_exprs_elsewhere() {
    let mut broken = scalar("broken", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    broken.scale = Number::Expr("{{{ not an expr".to_string());
    let good = scalar("good", ScalarType::U08, 1, 1.0, 0.0, 255.0);
    let mut t = tune(Endianness::Little, vec![broken, good]);
    load1(&mut t, &[0, 42]);
    assert_eq!(t.get("good").unwrap(), Value::Scalar(42.0));
}

// ---------- 4.5 — error surface ----------

#[test]
fn unknown_constant_errors_on_get_and_set() {
    let mut t = tune(Endianness::Little, vec![]);
    assert_eq!(
        t.get("nope"),
        Err(ModelError::UnknownConstant("nope".to_string()))
    );
    assert_eq!(
        t.set("nope", Value::Scalar(1.0)),
        Err(ModelError::UnknownConstant("nope".to_string()))
    );
}

#[test]
fn wrong_value_variant_is_type_mismatch() {
    let mut text = scalar("note", ScalarType::U08, 8, 1.0, 0.0, 0.0);
    text.kind = ConstantKind::Text { len: 4 };
    let c = scalar("c", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    let mut t = tune(Endianness::Little, vec![c, text]);
    let r = t.set("c", Value::Text("x".to_string()));
    assert!(matches!(r, Err(ModelError::TypeMismatch(_))));
    let r = t.set("note", Value::Scalar(1.0));
    assert!(matches!(r, Err(ModelError::TypeMismatch(_))));
}

#[test]
fn boundary_values_are_valid_and_outside_is_out_of_range() {
    let mut t = tune(
        Endianness::Little,
        vec![scalar("c", ScalarType::U08, 0, 1.0, 10.0, 200.0)],
    );
    t.set("c", Value::Scalar(10.0)).unwrap(); // exactly low
    t.set("c", Value::Scalar(200.0)).unwrap(); // exactly high
    assert_eq!(
        t.set("c", Value::Scalar(200.1)),
        Err(ModelError::OutOfRange {
            name: "c".to_string(),
            value: 200.1
        })
    );
    assert_eq!(
        t.set("c", Value::Scalar(9.9)),
        Err(ModelError::OutOfRange {
            name: "c".to_string(),
            value: 9.9
        })
    );
    assert_eq!(t.page_bytes(1)[0], 200); // rejected writes leave bytes alone
}

#[test]
fn array_length_mismatch_is_type_mismatch() {
    let mut c = scalar("map", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    c.kind = ConstantKind::Array {
        elem: ScalarType::U08,
        shape: Shape { rows: 2, cols: 2 },
    };
    let mut t = tune(Endianness::Little, vec![c]);
    let r = t.set("map", Value::Array(vec![1.0, 2.0, 3.0]));
    assert!(matches!(r, Err(ModelError::TypeMismatch(_))));
}

#[test]
fn array_element_out_of_range_rejects_the_whole_write() {
    let mut c = scalar("map", ScalarType::U08, 0, 1.0, 0.0, 100.0);
    c.kind = ConstantKind::Array {
        elem: ScalarType::U08,
        shape: Shape { rows: 2, cols: 2 },
    };
    let mut t = tune(Endianness::Little, vec![c]);
    let r = t.set("map", Value::Array(vec![1.0, 999.0, 1.0, 1.0]));
    assert_eq!(
        r,
        Err(ModelError::OutOfRange {
            name: "map".to_string(),
            value: 999.0
        })
    );
    assert_eq!(&t.page_bytes(1)[0..4], &[0, 0, 0, 0]);
}

#[test]
fn enum_index_beyond_options_or_capacity_is_out_of_range() {
    let mut with_options = scalar("mode", ScalarType::U08, 0, 1.0, 0.0, 0.0);
    with_options.kind = ConstantKind::Bits {
        storage: ScalarType::U08,
        bit_lo: 0,
        bit_hi: 1,
        options: vec!["Off".to_string(), "On".to_string()],
    };
    let mut capacity_only = scalar("raw", ScalarType::U08, 1, 1.0, 0.0, 0.0);
    capacity_only.kind = ConstantKind::Bits {
        storage: ScalarType::U08,
        bit_lo: 0,
        bit_hi: 1,
        options: vec![],
    };
    let mut t = tune(Endianness::Little, vec![with_options, capacity_only]);
    t.set("mode", Value::Enum(1)).unwrap();
    assert_eq!(
        t.set("mode", Value::Enum(2)), // capacity holds 0..=3, but only 2 options
        Err(ModelError::OutOfRange {
            name: "mode".to_string(),
            value: 2.0
        })
    );
    t.set("raw", Value::Enum(3)).unwrap();
    assert_eq!(
        t.set("raw", Value::Enum(4)), // beyond the 2-bit field capacity
        Err(ModelError::OutOfRange {
            name: "raw".to_string(),
            value: 4.0
        })
    );
}

#[test]
fn rejected_bits_write_preserves_all_storage_bytes() {
    let mut mode = scalar("mode", ScalarType::U16, 0, 1.0, 0.0, 0.0);
    mode.kind = ConstantKind::Bits {
        storage: ScalarType::U16,
        bit_lo: 4,
        bit_hi: 5,
        options: vec!["Zero".to_string(), "One".to_string()],
    };
    let mut t = tune(Endianness::Little, vec![mode]);
    load1(&mut t, &[0xA5, 0x5A]);
    let before = t.page_bytes(1).to_vec();

    assert_eq!(
        t.set("mode", Value::Enum(2)),
        Err(ModelError::OutOfRange {
            name: "mode".to_string(),
            value: 2.0
        })
    );
    assert_eq!(
        t.page_bytes(1),
        before,
        "rejected enum writes must not alter neighboring or field bits"
    );
}

#[test]
fn text_longer_than_declared_len_is_type_mismatch() {
    let mut c = scalar("note", ScalarType::U08, 0, 1.0, 0.0, 0.0);
    c.kind = ConstantKind::Text { len: 4 };
    let mut t = tune(Endianness::Little, vec![c]);
    let r = t.set("note", Value::Text("12345".to_string()));
    assert!(matches!(r, Err(ModelError::TypeMismatch(_))));
}

#[test]
fn unresolvable_bound_expr_fails_set_with_unresolved_expr() {
    let mut c = scalar("c", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    c.high = Number::Expr("bogusLimit".to_string());
    let mut t = tune(Endianness::Little, vec![c]);
    let r = t.set("c", Value::Scalar(1.0));
    assert!(matches!(r, Err(ModelError::UnresolvedExpr(_))));
}

// ---------- M4 Task 3 — set_cells (per-gesture cell writes) ----------

/// The Task 3 fixture: a 2-D array constant plus a scalar, built the same
/// way as the array-codec tests above (`scalar` + kind override). The
/// shared fixture's pages are `PAGE_SIZE` (64) bytes, so `veTable` is a
/// 4x8 U08 grid (32 cells; index 17 valid, 9999 out of bounds) rather
/// than a full 16x16 — every `set_cells` behaviour under test
/// (multi-cell gesture, bounds, range, one undo step) is
/// shape-independent.
fn test_tune() -> Tune {
    let mut ve = scalar("veTable", ScalarType::U08, 0, 1.0, 0.0, 100.0);
    ve.kind = ConstantKind::Array {
        elem: ScalarType::U08,
        shape: Shape { rows: 4, cols: 8 },
    };
    let req_fuel = scalar("reqFuel", ScalarType::U16, 40, 0.1, 0.0, 6553.5);
    tune(Endianness::Little, vec![ve, req_fuel])
}

#[test]
fn set_cells_edits_cells_and_undoes_as_one_step() {
    let mut tune = test_tune(); // the module's existing array-constant fixture helper
    let Value::Array(before) = tune.get("veTable").unwrap() else {
        panic!()
    };
    tune.set_cells("veTable", &[(0, 55.0), (17, 60.0)]).unwrap();
    let Value::Array(after) = tune.get("veTable").unwrap() else {
        panic!()
    };
    assert_eq!(after[0], 55.0);
    assert_eq!(after[17], 60.0);
    assert_eq!(after[1], before[1], "untouched cells intact");
    assert!(tune.is_dirty());
    assert!(tune.undo(), "one gesture = one undo step");
    assert_eq!(tune.get("veTable").unwrap(), Value::Array(before));
    assert!(!tune.undo() || tune.get("veTable").unwrap() != Value::Array(vec![]));
}

#[test]
fn set_cells_rejects_out_of_bounds_and_non_array_untouched() {
    let mut tune = test_tune();
    let before = tune.get("veTable").unwrap();
    assert!(tune.set_cells("veTable", &[(9999, 1.0)]).is_err());
    assert_eq!(
        tune.get("veTable").unwrap(),
        before,
        "failed call touches nothing"
    );
    assert!(
        tune.set_cells("reqFuel", &[(0, 1.0)]).is_err(),
        "scalar is not an array"
    );
    assert!(
        tune.set_cells("veTable", &[]).is_ok(),
        "empty gesture is a no-op"
    );
}

#[test]
fn set_cells_untouched_out_of_range_cell_does_not_block_other_edits() {
    // M4 final-review fix wave item 3: an UNTOUCHED stored cell outside the
    // declared [low, high] range (stale tune vs. a newer INI's bounds is a
    // legitimate real-world state — `load_page` never validates) must not
    // fail a gesture that edits a DIFFERENT, in-range cell.
    let mut tune = test_tune();
    let mut page = vec![0u8; PAGE_SIZE];
    page[5] = 200; // veTable[5] decodes to 200.0 — outside the [0,100] range
    tune.load_page(1, page);

    tune.set_cells("veTable", &[(0, 55.0)])
        .expect("an untouched out-of-range cell must not block edits to other cells");

    let Value::Array(after) = tune.get("veTable").unwrap() else {
        panic!()
    };
    assert_eq!(after[0], 55.0, "touched cell applied");
    assert_eq!(
        tune.page_bytes(1)[5],
        200,
        "untouched out-of-range cell's bytes stay byte-identical"
    );
    assert!(tune.undo(), "one gesture = one undo step");
    let Value::Array(undone) = tune.get("veTable").unwrap() else {
        panic!()
    };
    assert_eq!(undone[0], 0.0, "undo restores the touched cell");
    assert_eq!(
        tune.page_bytes(1)[5],
        200,
        "untouched out-of-range cell survives undo too"
    );

    // Touching the OOR cell itself with another OOR value still errors.
    let mut tune2 = test_tune();
    let mut page2 = vec![0u8; PAGE_SIZE];
    page2[5] = 200;
    tune2.load_page(1, page2);
    assert_eq!(
        tune2.set_cells("veTable", &[(5, 150.0)]),
        Err(ModelError::OutOfRange {
            name: "veTable".to_string(),
            value: 150.0
        }),
        "touching the OOR cell with another OOR value still errors"
    );
}

#[test]
fn set_cells_value_above_high_is_out_of_range_and_touches_nothing() {
    // Range violations surface as `ModelError::OutOfRange` from
    // `encode_scalar` — `set_cells` shares `set`'s per-element checks.
    let mut tune = test_tune();
    let before = tune.get("veTable").unwrap();
    assert_eq!(
        tune.set_cells("veTable", &[(0, 55.0), (1, 150.0)]),
        Err(ModelError::OutOfRange {
            name: "veTable".to_string(),
            value: 150.0
        })
    );
    assert_eq!(
        tune.get("veTable").unwrap(),
        before,
        "rejected gesture touches nothing"
    );
    assert!(!tune.is_dirty());
}
