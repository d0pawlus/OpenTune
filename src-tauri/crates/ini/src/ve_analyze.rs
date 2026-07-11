// SPDX-License-Identifier: GPL-3.0-or-later
//! `[VeAnalyze]` types — the parsed binding between a VE (or ignition) table
//! and the AFR/lambda target table used to auto-tune it, plus the sample
//! filters applied before analysis (Task 2 parses; this module only freezes
//! the shape).
//!
//! **Written fresh** (ADR-0006 recorded exception): hyper-tuner/ini's section
//! switch ends at `[Datalog]` (`src/ini.ts` ~lines 146-190) — it never parses
//! `[VeAnalyze]`, so there is nothing to port. The grammar truth-source is the
//! real `speeduino.ini` (noisymime/speeduino @ 0832dc1d, GPL-3 — consulted as
//! a reference corpus only, no code involved), quoted verbatim (l.5984-6010,
//! `#else` branch — the parser has no build-profile symbols, see
//! [`crate::definition::parse_definition`]):
//!
//! ```text
//! [VeAnalyze]
//! #if LAMBDA
//!      veAnalyzeMap = veTable1Tbl, lambdaTable1Tbl, lambda, egoCorrection
//!      lambdaTargetTables = lambdaTable1Tbl, afrTSCustom
//! #else
//!      veAnalyzeMap = veTable1Tbl, afrTable1Tbl, afr, egoCorrection
//!      lambdaTargetTables = afrTable1Tbl, afrTSCustom
//! #endif
//!          filter = std_xAxisMin ; Auto build
//!          filter = std_xAxisMax ; Auto build
//!          filter = std_DeadLambda ; Auto build
//! #if CELSIUS
//!          filter = minCltFilter, "Minimum CLT", coolant,       <       , 71,       , true
//! #else
//!          filter = minCltFilter, "Minimum CLT", coolant,       <       , 160,      , true
//! #endif
//!          filter = accelFilter, "Accel Flag" , engine,         &       , 16,       , false
//!          filter = overrunFilter, "Overrun"    , pulseWidth,  =       , 0,        , false
//!          filter = std_Custom ; Standard Custom Expression Filter.
//! ```
//!
//! `lambdaTargetTables` and `[WueAnalyze]` are known-but-deferred (m4-decisions:
//! silently skipped, not represented here).

/// The parsed `[VeAnalyze]` binding: which table(s) a VE-analysis run
/// corrects, and the sample filters applied before analysis.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct VeAnalyzeDef {
    pub maps: Vec<VeAnalyzeMapDef>,
    pub filters: Vec<AnalyzeFilterDef>,
}

/// `veAnalyzeMap = veTable1Tbl, afrTable1Tbl, afr, egoCorrection`
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct VeAnalyzeMapDef {
    /// `[TableEditor]` id of the table being corrected.
    pub table: String,
    /// `[TableEditor]` id of the AFR/lambda target table.
    pub target_table: String,
    /// measured-AFR/lambda output channel.
    pub lambda_channel: String,
    /// EGO-correction output channel.
    pub ego_channel: String,
}

/// One `filter = ...` row: either a built-in standard filter (referenced by
/// name only) or a custom threshold filter.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub enum AnalyzeFilterDef {
    /// `filter = std_xAxisMin` etc. — carries the raw std name.
    Std(String),
    /// `filter = minCltFilter, "Minimum CLT", coolant, <, 160, , true`
    Custom {
        id: String,
        label: String,
        channel: String,
        op: FilterOp,
        value: f64,
        /// Trailing INI flag, captured verbatim (TS semantics unconfirmed —
        /// OpenTune applies all parsed filters; params can disable by id).
        default_on: bool,
    },
}

/// A custom filter's comparison operator.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub enum FilterOp {
    Lt,
    Gt,
    Eq,
    And,
}
