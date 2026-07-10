// SPDX-License-Identifier: GPL-3.0-or-later
//! Determinism + semantics tests for the ve_analyze engine (M4 Task 10).
use opentune_analysis::*;

fn flat_grid(v: f64) -> TableGrid {
    TableGrid {
        x_bins: vec![1000.0, 2000.0],
        y_bins: vec![20.0, 40.0],
        z: vec![v; 4],
    }
}
fn binding() -> AnalyzeBinding {
    AnalyzeBinding {
        x_channel: "rpm".into(),
        y_channel: "map".into(),
        afr_channel: "afr".into(),
        ego_channel: "egoCorrection".into(),
        filters: vec![
            FilterSpec::XAxisMin,
            FilterSpec::XAxisMax,
            FilterSpec::YAxisMin,
            FilterSpec::YAxisMax,
            FilterSpec::DeadLambda,
        ],
    }
}
fn params(lag: u32) -> VeAnalyzeParams {
    VeAnalyzeParams {
        lag_records: lag,
        ..VeAnalyzeParams::default()
    }
}
fn samples(rows: Vec<Vec<f64>>) -> SampleSet {
    SampleSet {
        columns: vec![
            "rpm".into(),
            "map".into(),
            "afr".into(),
            "egoCorrection".into(),
        ],
        t_ms: (0..rows.len()).map(|i| i as f64 * 40.0).collect(),
        rows,
    }
}

#[test]
fn lean_sample_raises_ve_with_pinned_numbers() {
    // One sample exactly on cell 0: factor = 29.4/14.7 = 2.0 exactly.
    let s = samples(vec![vec![1000.0, 20.0, 29.4, 100.0]]);
    let r = ve_analyze(
        &s,
        &flat_grid(50.0),
        &flat_grid(14.7),
        &binding(),
        &params(0),
    )
    .unwrap();
    let c = &r.cells[0];
    assert!((c.hit_weight - 1.0).abs() < 1e-12);
    assert_eq!(c.sample_count, 1);
    // w_conf = 1/20, v_conf = 1 (var 0) → confidence 0.05;
    // blended = 50 + (100-50)·0.05·0.8 = 52.0 (inside the ±15% clamp).
    assert!((c.confidence - 0.05).abs() < 1e-9);
    assert!((c.proposed - 52.0).abs() < 1e-9);
    assert!((c.delta_pct - 4.0).abs() < 1e-9);
    assert_eq!(r.used_samples, 1);
    // Untouched cells: below min_weight ⇒ unchanged, confidence 0.
    assert_eq!(r.cells[3].proposed, 50.0);
    assert_eq!(r.cells[3].confidence, 0.0);
}

#[test]
fn max_delta_clamp_engages() {
    // 10 identical strong-lean samples: confidence 0.5 → blended 70 → clamped to 57.5.
    let s = samples(vec![vec![1000.0, 20.0, 29.4, 100.0]; 10]);
    let r = ve_analyze(
        &s,
        &flat_grid(50.0),
        &flat_grid(14.7),
        &binding(),
        &params(0),
    )
    .unwrap();
    assert!((r.cells[0].proposed - 57.5).abs() < 1e-9); // 50 ± 15%
}

#[test]
fn mid_cell_sample_splits_weight_and_stays_below_min_weight() {
    let s = samples(vec![vec![1500.0, 30.0, 29.4, 100.0]]); // 4 × w=0.25 < min_weight 1.0
    let r = ve_analyze(
        &s,
        &flat_grid(50.0),
        &flat_grid(14.7),
        &binding(),
        &params(0),
    )
    .unwrap();
    assert!(r
        .cells
        .iter()
        .all(|c| c.proposed == 50.0 && c.confidence == 0.0));
    assert!((r.cells[0].hit_weight - 0.25).abs() < 1e-12);
    assert_eq!(
        r.cells[0].sample_count, 1,
        "max-weight tie breaks to lowest index"
    );
}

#[test]
fn filters_reject_in_declared_order_and_are_all_reported() {
    let mut b = binding();
    b.filters.push(FilterSpec::Custom {
        id: "minCltFilter".into(),
        label: "Minimum CLT".into(),
        channel: "coolant".into(),
        op: FilterOp::Lt,
        value: 60.0,
    });
    let mut s = samples(vec![
        vec![500.0, 30.0, 14.7, 100.0],  // rpm below x-axis → std_xAxisMin
        vec![1500.0, 30.0, 0.0, 100.0],  // dead lambda
        vec![1000.0, 20.0, 14.7, 100.0], // survives
    ]);
    s.columns.push("coolant".into());
    for row in &mut s.rows {
        row.push(90.0); // warm coolant — the CLT filter never fires
    }
    let r = ve_analyze(&s, &flat_grid(50.0), &flat_grid(14.7), &b, &params(0)).unwrap();
    assert_eq!(r.used_samples, 1);
    let count = |id: &str| r.filtered.iter().find(|f| f.id == id).unwrap().count;
    assert_eq!(count("std_xAxisMin"), 1);
    assert_eq!(count("std_DeadLambda"), 1);
    assert_eq!(count("minCltFilter"), 0, "reported even at zero");
    assert_eq!(count("nonFinite"), 0);
}

#[test]
fn lag_pairs_afr_with_the_earlier_operating_point() {
    // Row 0 sits on cell 0; row 1 has moved to cell 3 but carries the lean afr.
    // With lag=1 the lean reading must credit CELL 0, not cell 3.
    let s = samples(vec![
        vec![1000.0, 20.0, 14.7, 100.0],
        vec![2000.0, 40.0, 29.4, 100.0],
    ]);
    let r = ve_analyze(
        &s,
        &flat_grid(50.0),
        &flat_grid(14.7),
        &binding(),
        &params(1),
    )
    .unwrap();
    assert!(
        r.cells[0].proposed > 50.0,
        "correction lands on the lagged point"
    );
    assert_eq!(r.cells[3].proposed, 50.0);
    assert_eq!(r.total_samples, 2);
    assert_eq!(r.used_samples, 1, "lag consumes one pairing");
}

#[test]
fn ego_neutralization_folds_the_trim_in_and_center_zero_disables() {
    // The ECU already trimmed +10% (ego 110): even at afr == target the table must rise.
    let s = samples(vec![vec![1000.0, 20.0, 14.7, 110.0]]);
    let r = ve_analyze(
        &s,
        &flat_grid(50.0),
        &flat_grid(14.7),
        &binding(),
        &params(0),
    )
    .unwrap();
    assert!(r.cells[0].proposed > 50.0);
    let mut p = params(0);
    p.ego_center = 0.0; // disabled → afr == target → factor 1 → no change
    let r2 = ve_analyze(&s, &flat_grid(50.0), &flat_grid(14.7), &binding(), &p).unwrap();
    assert_eq!(r2.cells[0].proposed, 50.0);
}

#[test]
fn same_input_is_bitwise_identical() {
    // 200 pseudo-varied samples built DETERMINISTICALLY (no RNG in tests either).
    let rows: Vec<Vec<f64>> = (0..200)
        .map(|i| {
            let f = i as f64;
            vec![
                1000.0 + (f * 37.0) % 1000.0,
                20.0 + (f * 13.0) % 20.0,
                13.0 + (f * 7.0) % 4.0,
                98.0 + (f % 5.0),
            ]
        })
        .collect();
    let s = samples(rows);
    let a = ve_analyze(
        &s,
        &flat_grid(50.0),
        &flat_grid(14.7),
        &binding(),
        &params(3),
    )
    .unwrap();
    let b = ve_analyze(
        &s,
        &flat_grid(50.0),
        &flat_grid(14.7),
        &binding(),
        &params(3),
    )
    .unwrap();
    for (ca, cb) in a.cells.iter().zip(&b.cells) {
        assert_eq!(ca.proposed.to_bits(), cb.proposed.to_bits());
        assert_eq!(ca.confidence.to_bits(), cb.confidence.to_bits());
        assert_eq!(ca.hit_weight.to_bits(), cb.hit_weight.to_bits());
    }
    assert_eq!(a.filtered, b.filtered);
}

#[test]
fn missing_binding_channel_is_a_hard_error() {
    let s = SampleSet {
        columns: vec!["rpm".into()],
        t_ms: vec![0.0],
        rows: vec![vec![1.0]],
    };
    assert!(matches!(
        ve_analyze(&s, &flat_grid(50.0), &flat_grid(14.7), &binding(), &params(0)),
        Err(AnalyzeError::MissingChannel(n)) if n == "map"
    ));
}
