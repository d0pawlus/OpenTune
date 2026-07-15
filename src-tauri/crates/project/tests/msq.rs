// SPDX-License-Identifier: GPL-3.0-or-later
mod common;

use common::{array_on, bits_on, scalar, text_on, tune, SIGNATURE};
use opentune_ini::ScalarType;
use opentune_model::Value;
use opentune_project::msq::{load_msq_into, tune_to_msq, MsqError};

#[test]
fn scalar_array_bits_text_round_trip() {
    let mut t = tune(vec![
        scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0),
        array_on("veTable", 4, 2, 2),
        bits_on("algorithm", 8, &["Speed Density", "Alpha-N", "MAP"]),
        text_on("tuneName", 16, 8),
    ]);
    t.set("crankingRPM", Value::Scalar(40.0)).unwrap();
    t.set("veTable", Value::Array(vec![10.0, 20.0, 30.0, 40.0]))
        .unwrap();
    t.set("algorithm", Value::Enum(1)).unwrap(); // "Alpha-N"
    t.set("tuneName", Value::Text("MyTune".to_string()))
        .unwrap();

    let xml = tune_to_msq(&t);
    assert!(xml.contains(&format!("signature=\"{SIGNATURE}\"")));

    // Re-load into a fresh zeroed tune with the same definition.
    let mut fresh = tune(vec![
        scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0),
        array_on("veTable", 4, 2, 2),
        bits_on("algorithm", 8, &["Speed Density", "Alpha-N", "MAP"]),
        text_on("tuneName", 16, 8),
    ]);
    let report = load_msq_into(&mut fresh, &xml).unwrap();
    assert_eq!(report.applied, 4);
    assert!(report.skipped.is_empty());
    assert!(report.failed.is_empty());
    assert_eq!(fresh.get("crankingRPM").unwrap(), Value::Scalar(40.0));
    assert_eq!(
        fresh.get("veTable").unwrap(),
        Value::Array(vec![10.0, 20.0, 30.0, 40.0])
    );
    assert_eq!(fresh.get("algorithm").unwrap(), Value::Enum(1));
    assert_eq!(
        fresh.get("tuneName").unwrap(),
        Value::Text("MyTune".to_string())
    );
}

#[test]
fn unknown_constant_is_skipped_not_fatal() {
    // A .msq may name a constant the loaded Definition doesn't declare
    // (different firmware build). It goes to `skipped`, never aborts the
    // load, and the constants the definition DOES declare still apply.
    let mut t = tune(vec![scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0)]);
    t.set("crankingRPM", Value::Scalar(40.0)).unwrap();
    // Inject a constant the fresh tune's definition has no knowledge of.
    let xml = tune_to_msq(&t).replace(
        "  </page>",
        "    <constant name=\"doesNotExistInDef\">1</constant>\n  </page>",
    );

    let mut fresh = tune(vec![scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0)]);
    let report = load_msq_into(&mut fresh, &xml).unwrap();
    assert_eq!(report.applied, 1); // crankingRPM still applied
    assert!(report.failed.is_empty());
    assert_eq!(report.skipped, vec!["doesNotExistInDef".to_string()]);
    assert_eq!(fresh.get("crankingRPM").unwrap(), Value::Scalar(40.0));
}

#[test]
fn out_of_range_scalar_clamps_like_tunerstudio() {
    // TunerStudio clamps out-of-range values into [low, high] on load
    // instead of rejecting them (rusEFI files routinely store 0 for
    // constants whose INI minimum is 1). The clamp is reported, and the
    // clamped value applies.
    let mut t = tune(vec![
        scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0),
        scalar("boostLimit", ScalarType::U08, 1, 1.0, 255.0),
    ]);
    t.set("crankingRPM", Value::Scalar(40.0)).unwrap();
    t.set("boostLimit", Value::Scalar(200.0)).unwrap();
    // 999 parses as a scalar but exceeds high=255 → clamps to 255.
    let xml = tune_to_msq(&t).replace(">200<", ">999<");

    let mut fresh = tune(vec![
        scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0),
        scalar("boostLimit", ScalarType::U08, 1, 1.0, 255.0),
    ]);
    let report = load_msq_into(&mut fresh, &xml).unwrap();
    assert_eq!(report.applied, 2);
    assert!(report.failed.is_empty(), "failed: {:?}", report.failed);
    assert_eq!(report.clamped, vec!["boostLimit".to_string()]);
    assert_eq!(fresh.get("boostLimit").unwrap(), Value::Scalar(255.0));
    assert_eq!(fresh.get("crankingRPM").unwrap(), Value::Scalar(40.0));
}

#[test]
fn out_of_range_array_elements_clamp_like_tunerstudio() {
    let mut t = tune(vec![array_on("veTable", 4, 2, 2)]);
    t.set("veTable", Value::Array(vec![10.0, 20.0, 30.0, 40.0]))
        .unwrap();
    // 999 exceeds the array's high=255 → that element clamps, rest apply.
    let xml = tune_to_msq(&t).replace(">10 20 30 40<", ">10 999 30 40<");

    let mut fresh = tune(vec![array_on("veTable", 4, 2, 2)]);
    let report = load_msq_into(&mut fresh, &xml).unwrap();
    assert_eq!(report.applied, 1);
    assert!(report.failed.is_empty(), "failed: {:?}", report.failed);
    assert_eq!(report.clamped, vec!["veTable".to_string()]);
    assert_eq!(
        fresh.get("veTable").unwrap(),
        Value::Array(vec![10.0, 255.0, 30.0, 40.0])
    );
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
fn quoted_bit_label_from_tunerstudio_applies() {
    // TunerStudio wraps enum/bit labels in double quotes on save
    // (`<constant name="algorithm">"Alpha-N"</constant>`); rusEFI project
    // files are full of these. The quoted text must match the unquoted
    // option list instead of failing as an unknown option.
    let mut t = tune(vec![bits_on("algorithm", 8, &["Speed Density", "Alpha-N"])]);
    t.set("algorithm", Value::Enum(1)).unwrap();
    let xml = tune_to_msq(&t).replace(">Alpha-N<", ">\"Alpha-N\"<");

    let mut fresh = tune(vec![bits_on("algorithm", 8, &["Speed Density", "Alpha-N"])]);
    let report = load_msq_into(&mut fresh, &xml).unwrap();
    assert!(report.failed.is_empty(), "failed: {:?}", report.failed);
    assert_eq!(report.applied, 1);
    assert_eq!(fresh.get("algorithm").unwrap(), Value::Enum(1));
}

#[test]
fn quoted_text_value_from_tunerstudio_unquotes() {
    // TunerStudio quotes string constants too — the stored value must not
    // keep the literal quotes.
    let mut t = tune(vec![text_on("tuneName", 16, 8)]);
    t.set("tuneName", Value::Text("MyTune".to_string()))
        .unwrap();
    let xml = tune_to_msq(&t).replace(">MyTune<", ">\"MyTune\"<");

    let mut fresh = tune(vec![text_on("tuneName", 16, 8)]);
    let report = load_msq_into(&mut fresh, &xml).unwrap();
    assert!(report.failed.is_empty(), "failed: {:?}", report.failed);
    assert_eq!(
        fresh.get("tuneName").unwrap(),
        Value::Text("MyTune".to_string())
    );
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
