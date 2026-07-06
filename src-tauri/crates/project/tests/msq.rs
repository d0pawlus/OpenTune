// SPDX-License-Identifier: GPL-3.0-or-later
mod common;

use common::{array_on, bits_on, scalar, tune, SIGNATURE};
use opentune_ini::ScalarType;
use opentune_model::Value;
use opentune_project::msq::{load_msq_into, tune_to_msq, MsqError};

#[test]
fn scalar_array_bits_text_round_trip() {
    let mut t = tune(vec![
        scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0),
        array_on("veTable", 4, 2, 2),
        bits_on("algorithm", 8, &["Speed Density", "Alpha-N", "MAP"]),
    ]);
    t.set("crankingRPM", Value::Scalar(40.0)).unwrap();
    t.set("veTable", Value::Array(vec![10.0, 20.0, 30.0, 40.0]))
        .unwrap();
    t.set("algorithm", Value::Enum(1)).unwrap(); // "Alpha-N"

    let xml = tune_to_msq(&t);
    assert!(xml.contains(&format!("signature=\"{SIGNATURE}\"")));

    // Re-load into a fresh zeroed tune with the same definition.
    let mut fresh = tune(vec![
        scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0),
        array_on("veTable", 4, 2, 2),
        bits_on("algorithm", 8, &["Speed Density", "Alpha-N", "MAP"]),
    ]);
    let report = load_msq_into(&mut fresh, &xml).unwrap();
    assert_eq!(report.applied, 3);
    assert!(report.skipped.is_empty());
    assert!(report.failed.is_empty());
    assert_eq!(fresh.get("crankingRPM").unwrap(), Value::Scalar(40.0));
    assert_eq!(
        fresh.get("veTable").unwrap(),
        Value::Array(vec![10.0, 20.0, 30.0, 40.0])
    );
    assert_eq!(fresh.get("algorithm").unwrap(), Value::Enum(1));
}

#[test]
fn bad_bit_label_is_collected_not_fatal() {
    // A real .msq may carry a bit-field label the INI's parsed options don't
    // match exactly. That one constant must fail into the report, not abort
    // the whole load — the good constants still apply.
    let good = tune(vec![
        scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0),
        bits_on("algorithm", 8, &["Speed Density", "Alpha-N"]),
    ]);
    // Serialize a valid tune, then corrupt only the bit-field's label text.
    let xml = {
        let mut t = good;
        t.set("crankingRPM", Value::Scalar(40.0)).unwrap();
        t.set("algorithm", Value::Enum(1)).unwrap();
        tune_to_msq(&t).replace(">Alpha-N<", ">Nope-Not-An-Option<")
    };
    let mut fresh = tune(vec![
        scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0),
        bits_on("algorithm", 8, &["Speed Density", "Alpha-N"]),
    ]);
    let report = load_msq_into(&mut fresh, &xml).unwrap();
    assert_eq!(report.applied, 1); // crankingRPM applied
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].0, "algorithm");
    assert_eq!(fresh.get("crankingRPM").unwrap(), Value::Scalar(40.0));
}

#[test]
fn bit_field_serializes_as_label_not_index() {
    let mut t = tune(vec![bits_on("algorithm", 0, &["Speed Density", "Alpha-N"])]);
    t.set("algorithm", Value::Enum(1)).unwrap();
    let xml = tune_to_msq(&t);
    assert!(
        xml.contains(">Alpha-N<"),
        "bit field must serialize the option label, got: {xml}"
    );
    assert!(!xml.contains(">1<"), "must not serialize the raw index");
}

#[test]
fn signature_mismatch_is_rejected() {
    let t = tune(vec![scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0)]);
    let bad = tune_to_msq(&t).replace(SIGNATURE, "rusEFI 2024");
    let mut fresh = tune(vec![scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0)]);
    let err = load_msq_into(&mut fresh, &bad).unwrap_err();
    assert!(matches!(err, MsqError::SignatureMismatch { .. }));
}
