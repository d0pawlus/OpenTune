// SPDX-License-Identifier: GPL-3.0-or-later
//! Expression-variable resolution beyond page-backed constants — the MS3
//! context chain: `[PcVariables]` values (via `[ConstantsExtensions]`
//! `defaultValue`) and computed `[OutputChannels]` entries.
//!
//! Real MS3 INIs compute bounds and even scale factors from PC-side
//! settings: `fc_rpm` has `high = { rpmhigh }` (a pc_variable), and
//! `launchvss_minvss` has `scale = { msToPrefUnitsScale }` where
//! `msToPrefUnitsScale = { prefSpeedUnits == 0 ? 0.22369 : 0.36 }` is a
//! computed output channel referencing a pc_variable. Before this chain
//! existed, 234 constants failed to apply on the MS3 example project.

mod common;

use std::sync::Arc;

use common::{definition, load1, scalar, PAGE_SIZE};
use opentune_ini::{ConstantDef, ConstantKind, Endianness, Number, OutputChannelDef, ScalarType};
use opentune_model::{ModelError, Tune, Value};

/// A pc_variable (no page/offset semantics — never stored in ECU memory).
fn pc_scalar(name: &str) -> ConstantDef {
    scalar(name, ScalarType::U16, 0, 1.0, 0.0, 30000.0)
}

/// A pc_variable bit field with the given option labels.
fn pc_bits(name: &str, options: &[&str]) -> ConstantDef {
    ConstantDef {
        name: name.to_string(),
        page: 0,
        offset: 0,
        kind: ConstantKind::Bits {
            storage: ScalarType::U08,
            bit_lo: 0,
            bit_hi: 0,
            options: options.iter().map(|s| s.to_string()).collect(),
        },
        scale: Number::Lit(1.0),
        translate: Number::Lit(0.0),
        units: String::new(),
        low: Number::Lit(0.0),
        high: Number::Lit(0.0),
        digits: 0,
    }
}

fn computed(name: &str, expr: &str) -> OutputChannelDef {
    OutputChannelDef::Computed {
        name: name.to_string(),
        expr: expr.to_string(),
        units: String::new(),
    }
}

#[test]
fn bounds_resolve_pc_variable_via_constants_extensions_default() {
    // MS3: `fc_rpm = scalar, U16, 74, "rpm", 1, 0, 0, {rpmhigh}, 0` with
    // `[ConstantsExtensions] defaultValue = rpmhigh, 9000`.
    let mut fc_rpm = scalar("fc_rpm", ScalarType::U16, 0, 1.0, 0.0, 0.0);
    fc_rpm.high = Number::Expr("rpmhigh".to_string());
    let mut def = definition(Endianness::Little, vec![fc_rpm]);
    def.pc_variables = vec![pc_scalar("rpmhigh")];
    def.pc_defaults = vec![("rpmhigh".to_string(), "9000".to_string())];
    let tune = Tune::new(Arc::new(def));

    assert_eq!(tune.bounds("fc_rpm").expect("must resolve"), (0.0, 9000.0));
}

#[test]
fn bounds_resolve_computed_channel_referencing_pc_bits_label_default() {
    // MS3: `EAEAWWCLTbins` has `high = {clthighlim}` where
    // `[OutputChannels] clthighlim = { clt_exp ? 230 : 120 }` and `clt_exp`
    // is a pc_variable bit field. The default is the option LABEL.
    let mut bins = scalar("cltBins", ScalarType::S16, 0, 1.0, -40.0, 0.0);
    bins.high = Number::Expr("clthighlim".to_string());
    let mut def = definition(Endianness::Little, vec![bins]);
    def.pc_variables = vec![pc_bits("clt_exp", &["Normal", "Expanded"])];
    def.pc_defaults = vec![("clt_exp".to_string(), "Expanded".to_string())];
    def.output_channels = vec![computed("clthighlim", "clt_exp ? 230 : 120")];
    let tune = Tune::new(Arc::new(def));

    assert_eq!(
        tune.bounds("cltBins").expect("must resolve"),
        (-40.0, 230.0)
    );
}

#[test]
fn pc_variable_without_default_value_resolves_to_zero() {
    // TunerStudio initializes pc_variables without a `defaultValue` to
    // 0 / the first option; MS3's `clt_exp` path then yields 120.
    let mut bins = scalar("cltBins", ScalarType::S16, 0, 1.0, -40.0, 0.0);
    bins.high = Number::Expr("clthighlim".to_string());
    let mut def = definition(Endianness::Little, vec![bins]);
    def.pc_variables = vec![pc_bits("clt_exp", &["Normal", "Expanded"])];
    def.output_channels = vec![computed("clthighlim", "clt_exp ? 230 : 120")];
    let tune = Tune::new(Arc::new(def));

    assert_eq!(
        tune.bounds("cltBins").expect("must resolve"),
        (-40.0, 120.0)
    );
}

#[test]
fn expr_scale_via_computed_channel_encodes_values() {
    // MS3: `launchvss_minvss = scalar, U16, ..., { msToPrefUnitsScale }, ...`
    // — the SCALE itself is a computed channel. Encoding must resolve it or
    // every write fails.
    let mut vss = scalar("launchvss_minvss", ScalarType::U16, 0, 1.0, 0.0, 200.0);
    vss.scale = Number::Expr("msScale".to_string());
    let mut def = definition(Endianness::Little, vec![vss]);
    def.pc_variables = vec![pc_bits("prefSpeedUnits", &["MPH", "KPH"])];
    def.pc_defaults = vec![("prefSpeedUnits".to_string(), "MPH".to_string())];
    def.output_channels = vec![computed("msScale", "prefSpeedUnits == 0 ? 0.5 : 2")];
    let mut tune = Tune::new(Arc::new(def));
    load1(&mut tune, &[0u8; PAGE_SIZE]);

    tune.set("launchvss_minvss", Value::Scalar(10.0))
        .expect("expr scale must resolve on encode");
    // raw = value / scale - translate = 10.0 / 0.5 = 20 (little-endian U16).
    assert_eq!(&tune.page_bytes(1)[0..2], &[20, 0]);
}

#[test]
fn self_referential_computed_channel_is_an_error_not_a_stack_overflow() {
    let mut c = scalar("victim", ScalarType::U08, 0, 1.0, 0.0, 0.0);
    c.high = Number::Expr("ouroboros".to_string());
    let mut def = definition(Endianness::Little, vec![c]);
    def.output_channels = vec![computed("ouroboros", "ouroboros + 1")];
    let tune = Tune::new(Arc::new(def));

    assert!(matches!(
        tune.bounds("victim"),
        Err(ModelError::UnresolvedExpr(_))
    ));
}

/// A scalar output channel at the given block offset (MS3's realtime `map`).
fn och_scalar(name: &str, offset: usize, scale: f64, translate: f64) -> OutputChannelDef {
    OutputChannelDef::Scalar {
        name: name.to_string(),
        kind: ScalarType::S16,
        offset,
        units: String::new(),
        scale,
        translate,
    }
}

#[test]
fn get_channel_by_offset_builtins_adopt_the_load_channels_scaling() {
    // MS3 generic PWM curves: `pwm_loadvals_a = array, S16, 12, [6], "%",
    // { getChannelScaleByOffset(pwm_opt_load_a_offset) }, ...` — the curve
    // axis adopts the scaling of whichever realtime channel the tuner
    // selected (offset 18 = `map = scalar, S16, 18, "kPa", 0.1, 0.0`).
    let mut loadvals = scalar("pwm_loadvals_a", ScalarType::S16, 0, 1.0, 0.0, 0.0);
    loadvals.scale = Number::Expr("getChannelScaleByOffset(pwm_opt_load_a_offset)".to_string());
    loadvals.low = Number::Expr("getChannelMinByOffset(pwm_opt_load_a_offset)".to_string());
    loadvals.high = Number::Expr("getChannelMaxByOffset(pwm_opt_load_a_offset)".to_string());
    // The offset selector itself is a page-backed constant holding 18.
    let selector = scalar(
        "pwm_opt_load_a_offset",
        ScalarType::U16,
        2,
        1.0,
        0.0,
        65535.0,
    );
    let mut def = definition(Endianness::Little, vec![loadvals, selector]);
    def.output_channels = vec![och_scalar("map", 18, 0.1, 0.0)];
    let mut tune = Tune::new(Arc::new(def));
    load1(&mut tune, &[0, 0, 18, 0]);

    // min/max approximate the channel's encodable range: S16 × 0.1.
    let (low, high) = tune.bounds("pwm_loadvals_a").expect("bounds resolve");
    assert!((low - (-3276.8)).abs() < 1e-9, "low {low}");
    assert!((high - 3276.7).abs() < 1e-9, "high {high}");

    tune.set("pwm_loadvals_a", Value::Scalar(40.0))
        .expect("expr scale via builtin must encode");
    // raw = 40.0 / 0.1 - 0 = 400 (little-endian S16).
    assert_eq!(&tune.page_bytes(1)[0..2], &[144, 1]);
}

#[test]
fn get_channel_digits_by_offset_resolves_to_zero() {
    let mut c = scalar("victim", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    c.high = Number::Expr("100 + getChannelDigitsByOffset(18)".to_string());
    let mut def = definition(Endianness::Little, vec![c]);
    def.output_channels = vec![och_scalar("map", 18, 0.1, 0.0)];
    let tune = Tune::new(Arc::new(def));
    assert_eq!(tune.bounds("victim").expect("resolves"), (0.0, 100.0));
}

#[test]
fn get_channel_by_offset_with_no_channel_at_offset_is_unresolved() {
    let mut c = scalar("victim", ScalarType::U08, 0, 1.0, 0.0, 255.0);
    c.high = Number::Expr("getChannelScaleByOffset(99)".to_string());
    let def = definition(Endianness::Little, vec![c]);
    let tune = Tune::new(Arc::new(def));
    assert!(matches!(
        tune.bounds("victim"),
        Err(ModelError::UnresolvedExpr(_))
    ));
}
