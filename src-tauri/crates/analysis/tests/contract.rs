// SPDX-License-Identifier: GPL-3.0-or-later
//! Signature-pinning contract tests for the M4-frozen analysis seams.
use opentune_analysis::*;

#[test]
#[allow(clippy::type_complexity)]
fn seams_compile_and_default_params_are_pinned() {
    let p = VeAnalyzeParams::default();
    assert_eq!(
        (p.min_weight, p.confidence_sat_weight, p.variance_penalty),
        (1.0, 20.0, 4.0)
    );
    assert_eq!((p.cell_change_resistance, p.max_delta_pct), (0.2, 15.0));
    assert_eq!((p.lag_records, p.ego_center), (6, 100.0));
    // Pin the ve_analyze signature without invoking the stub body.
    let _: fn(
        &SampleSet,
        &TableGrid,
        &TableGrid,
        &AnalyzeBinding,
        &VeAnalyzeParams,
    ) -> Result<VeAnalysisReport, AnalyzeError> = ve_analyze;
    let s = SampleSet {
        columns: vec!["rpm".into()],
        t_ms: vec![0.0],
        rows: vec![vec![1000.0]],
    };
    assert_eq!(s.column("rpm"), Some(0));
    assert_eq!(s.len(), 1);
}
