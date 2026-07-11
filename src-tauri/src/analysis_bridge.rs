// SPDX-License-Identifier: GPL-3.0-or-later
//! M4 Task 11 — the VE-analysis bridge: resolves everything
//! `opentune_analysis::ve_analyze` needs (the `[VeAnalyze]` binding, the VE
//! and AFR/lambda target grids, the compiled filter list) from the parsed
//! `Definition` + the live `Tune`, runs the engine, and projects its report
//! into the IPC [`VeAnalysisReportDto`]. Pure wiring — no I/O, unit-testable
//! without an owner/connection (the owner arm calls this inline; Task 10's
//! engine is a few ms over ≤27k rows).
//!
//! **Target-grid provenance (locked, Task 9/10 review seam):** the AFR/lambda
//! target grid is built from the `[VeAnalyze]` map's target table
//! (`afrTable1Tbl` in the bundled INI) *as stored in the tune* — never a
//! fallback to an `afrTarget` output-channel constant. An all-zero target
//! table (never written) makes every sample fail `targetMissing`, which is
//! correct fail-open behavior, not a bug this bridge should paper over.

use opentune_analysis::{
    AnalyzeBinding, FilterOp as AnalysisFilterOp, FilterSpec, TableGrid, VeAnalyzeParams,
};
use opentune_ini::{AnalyzeFilterDef, Definition, FilterOp as IniFilterOp, TableDef};
use opentune_model::{Tune, Value};

use crate::dto::VeAnalysisReportDto;

/// Resolve everything the engine needs from the parsed definition + live tune
/// and run it. `table` is a `[TableEditor]` id with a `[VeAnalyze]` map.
pub fn run_ve_analyze(
    def: &Definition,
    tune: &Tune,
    samples: &opentune_analysis::SampleSet,
    table: &str,
) -> Result<VeAnalysisReportDto, String> {
    let map = def
        .ve_analyze
        .as_ref()
        .and_then(|v| v.maps.iter().find(|m| m.table == table))
        .ok_or_else(|| format!("no [VeAnalyze] map for table `{table}`"))?;
    // `map` only resolves through a `Some(ve_analyze)`, so this is safe.
    let ve_analyze = def.ve_analyze.as_ref().expect("map implies ve_analyze");

    let ve_table = def
        .table(&map.table)
        .ok_or_else(|| format!("no [TableEditor] table `{}`", map.table))?;
    let target_table = def
        .table(&map.target_table)
        .ok_or_else(|| format!("no [TableEditor] table `{}`", map.target_table))?;

    let ve_grid = grid_from_tune(tune, ve_table)?;
    let target_grid = grid_from_tune(tune, target_table)?;

    if ve_table.x_channel.is_empty() {
        return Err(format!("table `{}` declares no x-axis channel", map.table));
    }
    if ve_table.y_channel.is_empty() {
        return Err(format!("table `{}` declares no y-axis channel", map.table));
    }

    let filters = ve_analyze
        .filters
        .iter()
        .filter_map(compile_filter)
        .collect();

    let binding = AnalyzeBinding {
        x_channel: ve_table.x_channel.clone(),
        y_channel: ve_table.y_channel.clone(),
        afr_channel: map.lambda_channel.clone(),
        ego_channel: map.ego_channel.clone(),
        filters,
    };

    let params = VeAnalyzeParams::default();
    let report = opentune_analysis::ve_analyze(samples, &ve_grid, &target_grid, &binding, &params)
        .map_err(|e| e.to_string())?;

    Ok(VeAnalysisReportDto {
        table: table.to_string(),
        x_len: report.x_len,
        y_len: report.y_len,
        cells: report.cells.into_iter().map(Into::into).collect(),
        filtered: report.filtered.into_iter().map(Into::into).collect(),
        total_samples: report.total_samples,
        used_samples: report.used_samples,
    })
}

/// Decode a `[TableEditor]` table's `x_bins`/`y_bins`/`z` constants — as
/// currently stored in `tune` — into an `opentune_analysis::TableGrid`.
fn grid_from_tune(tune: &Tune, table: &TableDef) -> Result<TableGrid, String> {
    Ok(TableGrid {
        x_bins: array_value(tune, &table.x_bins)?,
        y_bins: array_value(tune, &table.y_bins)?,
        z: array_value(tune, &table.z)?,
    })
}

/// Read a named constant's current value, requiring `Value::Array`.
fn array_value(tune: &Tune, name: &str) -> Result<Vec<f64>, String> {
    match tune.get(name) {
        Ok(Value::Array(xs)) => Ok(xs),
        Ok(_) => Err(format!("`{name}` is not an array constant")),
        Err(e) => Err(format!("failed to read `{name}`: {e:?}")),
    }
}

/// Compile one `[VeAnalyze]` `filter = ...` row into the engine's
/// `FilterSpec`. `std_xAxisMin/Max`/`std_yAxisMin/Max` map to the axis
/// variants, `std_DeadLambda` to `DeadLambda`; every other `std_*` — notably
/// `std_Custom`, the "standard custom expression filter" marker — has no
/// engine equivalent and is silently skipped (recorded-deferred, per
/// `opentune_ini::ve_analyze`'s own doc comment). `Custom` filters map
/// field-for-field.
fn compile_filter(def: &AnalyzeFilterDef) -> Option<FilterSpec> {
    match def {
        AnalyzeFilterDef::Std(name) => match name.as_str() {
            "std_xAxisMin" => Some(FilterSpec::XAxisMin),
            "std_xAxisMax" => Some(FilterSpec::XAxisMax),
            "std_yAxisMin" => Some(FilterSpec::YAxisMin),
            "std_yAxisMax" => Some(FilterSpec::YAxisMax),
            "std_DeadLambda" => Some(FilterSpec::DeadLambda),
            _ => None,
        },
        AnalyzeFilterDef::Custom {
            id,
            label,
            channel,
            op,
            value,
            ..
        } => Some(FilterSpec::Custom {
            id: id.clone(),
            label: label.clone(),
            channel: channel.clone(),
            op: compile_op(op),
            value: *value,
        }),
    }
}

fn compile_op(op: &IniFilterOp) -> AnalysisFilterOp {
    match op {
        IniFilterOp::Lt => AnalysisFilterOp::Lt,
        IniFilterOp::Gt => AnalysisFilterOp::Gt,
        IniFilterOp::Eq => AnalysisFilterOp::Eq,
        IniFilterOp::And => AnalysisFilterOp::And,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentune_ini::parse_definition;
    use opentune_model::Tune;
    use std::sync::Arc;

    const BUNDLED_INI: &str = include_str!("../resources/speeduino.sample.ini");

    fn seed_array(tune: &mut Tune, name: &str, values: Vec<f64>) {
        tune.set(name, Value::Array(values))
            .unwrap_or_else(|e| panic!("seeding `{name}` must succeed: {e:?}"));
    }

    /// 30 identical rows: rpm=3000 (an exact `rpmBins` bin), fuelLoad=50 (an
    /// exact `fuelLoadBins` bin), afr=16.17 (10% lean over the flat-14.7
    /// target), ego=100 (neutral), coolant=90 (>= the 60 `minCltFilter`
    /// threshold, so it never trips). Column order matches the real bundled
    /// INI's `[VeAnalyze]` binding: `x_channel`/`y_channel` come from
    /// `veTable1Tbl`'s `xBins`/`yBins` tokens (`rpm`/`fuelLoad`, NOT the raw
    /// `map` channel `fuelLoad` is derived from) and `afr`/`egoCorrection`
    /// from the `veAnalyzeMap` row.
    fn hand_built_samples() -> opentune_analysis::SampleSet {
        let columns = vec![
            "rpm".to_string(),
            "fuelLoad".to_string(),
            "afr".to_string(),
            "egoCorrection".to_string(),
            "coolant".to_string(),
        ];
        let row = vec![3000.0, 50.0, 16.17, 100.0, 90.0];
        let rows = vec![row; 30];
        opentune_analysis::SampleSet {
            t_ms: (0..rows.len()).map(|i| i as f64 * 40.0).collect(),
            columns,
            rows,
        }
    }

    fn seeded_tune(def: &Definition) -> Tune {
        let mut tune = Tune::new(Arc::new(def.clone()));
        let rpm_bins: Vec<f64> = (0..16).map(|i| 500.0 + i as f64 * 500.0).collect();
        let fuel_load_bins: Vec<f64> = (0..16).map(|i| 20.0 + i as f64 * 5.0).collect();
        seed_array(&mut tune, "rpmBins", rpm_bins.clone());
        seed_array(&mut tune, "fuelLoadBins", fuel_load_bins.clone());
        seed_array(&mut tune, "veTable", vec![50.0; 256]);
        seed_array(&mut tune, "rpmBinsAFR", rpm_bins);
        seed_array(&mut tune, "loadBinsAFR", fuel_load_bins);
        seed_array(&mut tune, "afrTable", vec![14.7; 256]);
        tune
    }

    #[test]
    fn bridges_the_bundled_ve_analyze_map_into_a_report() {
        let def = parse_definition(BUNDLED_INI).expect("bundled INI must parse");
        assert!(
            def.diagnostics.is_empty(),
            "bundled INI must parse diagnostic-free: {:?}",
            def.diagnostics
        );
        let tune = seeded_tune(&def);
        let samples = hand_built_samples();

        let report = run_ve_analyze(&def, &tune, &samples, "veTable1Tbl")
            .expect("veTable1Tbl has a [VeAnalyze] map");

        assert_eq!(report.table, "veTable1Tbl");
        assert_eq!(report.x_len, 16);
        assert_eq!(report.y_len, 16);

        // rpm=3000 -> rpmBins index 5 (500 + 5*500); fuelLoad=50 -> index 6
        // (20 + 6*5) -> flat index 6 * 16 + 5 = 101.
        let hit = &report.cells[101];
        assert!(
            hit.proposed > hit.current,
            "10% lean measured AFR must raise the hit cell's VE, got {} -> {}",
            hit.current,
            hit.proposed
        );

        let min_clt = report
            .filtered
            .iter()
            .find(|f| f.id == "minCltFilter")
            .expect("minCltFilter must always appear in the report");
        assert_eq!(min_clt.count, 0, "coolant=90 must never trip minCltFilter");
    }

    #[test]
    fn unknown_table_id_errors_with_a_clear_message() {
        let def = parse_definition(BUNDLED_INI).expect("bundled INI must parse");
        let tune = Tune::new(Arc::new(def.clone()));
        let samples = hand_built_samples();

        let err = run_ve_analyze(&def, &tune, &samples, "notATable").unwrap_err();
        assert!(err.contains("no [VeAnalyze] map"), "got: {err}");
    }
}
