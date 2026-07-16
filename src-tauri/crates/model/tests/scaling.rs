// SPDX-License-Identifier: GPL-3.0-or-later
//! Scale/translate semantics — pinned to TunerStudio's documented formula
//! (MegaTune-era INI header, restated in every MS-family INI):
//!
//! ```text
//! msValue   = userValue / scale - translate
//! userValue = (msValue + translate) * scale
//! ```
//!
//! NOT `userValue = msValue * scale + translate`. The two agree whenever
//! `translate == 0` (all of Speeduino) or `scale == 1` (most MS1 temps),
//! which is how the mirrored-wrong form survived until MS3's
//! `cel_warmtime = scalar, U16, 316, "mins", 0.01666, 9, 1, 20, 1`:
//! TunerStudio stores raw 300 for 5.14794 mins; the wrong form computed a
//! negative raw and rejected the value as OutOfRange.

mod common;

use common::{load1, scalar, tune, PAGE_SIZE};
use opentune_ini::{Endianness, ScalarType};
use opentune_model::Value;

const CEL_SCALE: f64 = 0.01666;
const CEL_TRANSLATE: f64 = 9.0;

fn cel_warmtime_tune() -> opentune_model::Tune {
    let mut c = scalar("cel_warmtime", ScalarType::U16, 0, CEL_SCALE, 1.0, 20.0);
    c.translate = opentune_ini::Number::Lit(CEL_TRANSLATE);
    let mut t = tune(Endianness::Little, vec![c]);
    load1(&mut t, &[0u8; PAGE_SIZE]);
    t
}

#[test]
fn decode_applies_translate_before_scale() {
    let mut t = cel_warmtime_tune();
    // raw 300 little-endian
    load1(&mut t, &[44, 1]);
    let Value::Scalar(v) = t.get("cel_warmtime").expect("get") else {
        panic!("expected scalar");
    };
    // (300 + 9) * 0.01666 = 5.14794 — TunerStudio's own stored user value.
    assert!((v - 5.14794).abs() < 1e-9, "got {v}");
}

#[test]
fn encode_applies_scale_before_translate() {
    let mut t = cel_warmtime_tune();
    t.set("cel_warmtime", Value::Scalar(5.14794))
        .expect("in-bounds value must encode");
    // raw = 5.14794 / 0.01666 - 9 = 300 (little-endian U16).
    assert_eq!(&t.page_bytes(1)[0..2], &[44, 1]);
}

#[test]
fn inverted_declared_bounds_normalize_instead_of_rejecting_everything() {
    // Real MS3 typo fallout (`psInitValue`: missing comma shifts fields,
    // leaving low=1, high=0). TunerStudio still accepts the 0/1 flag
    // values; an inverted declaration must not reject every value.
    let mut t = tune(
        Endianness::Little,
        vec![scalar("psInitValue", ScalarType::U08, 0, 1.0, 1.0, 0.0)],
    );
    load1(&mut t, &[0u8; PAGE_SIZE]);
    t.set("psInitValue", Value::Scalar(0.0))
        .expect("0 in [0,1]");
    t.set("psInitValue", Value::Scalar(1.0))
        .expect("1 in [0,1]");
    assert!(t.set("psInitValue", Value::Scalar(2.0)).is_err());
}
