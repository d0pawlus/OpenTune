// SPDX-License-Identifier: GPL-3.0-or-later
//! Task 4 tests — `Tune` scaled accessors (sub-steps 4.1/4.2) and the
//! error surface (4.5). Dirty/flash and undo/redo tests live in
//! `tune_state.rs`.

mod common;

use common::{load1, scalar, tune, PAGE_SIZE};
use opentune_ini::{ConstantKind, Endianness, Number, ScalarType, Shape};
use opentune_model::{ModelError, Value};

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
