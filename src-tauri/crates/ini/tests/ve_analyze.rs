// SPDX-License-Identifier: GPL-3.0-or-later
//! [VeAnalyze] parser tests. Grammar truth source: reference/speeduino.ini
//! @ 0832dc1d l.5984-6010 (GPL-3, quoted verbatim below). WRITE FRESH per
//! ADR-0006 (hyper-tuner does not parse this section).
use opentune_ini::{parse_definition, AnalyzeFilterDef, FilterOp};

#[test]
fn parses_ve_analyze_binding() {
    let ini = r#"
[MegaTune]
   signature      = "test"
   queryCommand   = "Q"
   versionInfo    = "S"
   ochGetCommand  = "r"
   pageReadCommand = "p%2i%2o%2c"
   pageValueWrite = "M%2i%2o%2c%v"
   burnCommand    = "b%2i"
   blockingFactor = 121
   blockReadTimeout = 2000

[VeAnalyze]
#if LAMBDA
     veAnalyzeMap = veTable1Tbl, lambdaTable1Tbl, lambda, egoCorrection
     lambdaTargetTables = lambdaTable1Tbl, afrTSCustom
#else
     veAnalyzeMap = veTable1Tbl, afrTable1Tbl, afr, egoCorrection
     lambdaTargetTables = afrTable1Tbl, afrTSCustom
#endif
         filter = std_xAxisMin ; Auto build
         filter = std_xAxisMax ; Auto build
         filter = std_DeadLambda ; Auto build
#if CELSIUS
         filter = minCltFilter, "Minimum CLT", coolant,       <       , 71,       , true
#else
         filter = minCltFilter, "Minimum CLT", coolant,       <       , 160,      , true
#endif
         filter = accelFilter, "Accel Flag" , engine,         &       , 16,       , false
         filter = overrunFilter, "Overrun"    , pulseWidth,  =       , 0,        , false
         filter = std_Custom ; Standard Custom Expression Filter.
"#;
    let def = parse_definition(ini).expect("parses");
    let va = def.ve_analyze.as_ref().expect("[VeAnalyze] parsed");
    assert_eq!(va.maps.len(), 1, "#else branch only");
    let m = &va.maps[0];
    assert_eq!(
        (
            m.table.as_str(),
            m.target_table.as_str(),
            m.lambda_channel.as_str(),
            m.ego_channel.as_str()
        ),
        ("veTable1Tbl", "afrTable1Tbl", "afr", "egoCorrection")
    );
    assert_eq!(va.filters.len(), 7);
    assert!(matches!(&va.filters[0], AnalyzeFilterDef::Std(s) if s == "std_xAxisMin"));
    match &va.filters[3] {
        AnalyzeFilterDef::Custom {
            id,
            label,
            channel,
            op,
            value,
            default_on,
        } => {
            assert_eq!(id, "minCltFilter");
            assert_eq!(label, "Minimum CLT");
            assert_eq!(channel, "coolant");
            assert_eq!(*op, FilterOp::Lt);
            assert!((value - 160.0).abs() < 1e-9, "#else branch value");
            assert!(default_on);
        }
        other => panic!("expected Custom, got {other:?}"),
    }
    match &va.filters[4] {
        AnalyzeFilterDef::Custom {
            op,
            value,
            default_on,
            ..
        } => {
            assert_eq!(*op, FilterOp::And);
            assert!((value - 16.0).abs() < 1e-9);
            assert!(!default_on);
        }
        other => panic!("expected Custom, got {other:?}"),
    }
    match &va.filters[5] {
        AnalyzeFilterDef::Custom { op, .. } => assert_eq!(*op, FilterOp::Eq),
        other => panic!("expected Custom, got {other:?}"),
    }
}
