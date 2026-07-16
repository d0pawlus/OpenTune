// SPDX-License-Identifier: GPL-3.0-or-later
//! The [`Value`] type — a decoded tune value, physical-unit representation.
//!
//! `Value` is what `Tune::get`/`Tune::set` exchange with callers (the Task 2
//! evaluator, and ultimately the frontend over IPC), as opposed to the raw
//! bytes stored in a page.

/// A decoded constant value, in its physical (already scaled) representation.
///
/// Derives `serde::Serialize` + `serde::Deserialize` + `specta::Type`: the
/// frontend both receives values read from a [`Tune`](crate::Tune) and sends
/// them back over IPC (the `set_value` command), so `Value` crosses the seam
/// in both directions. It stays externally tagged (`{ "Scalar": 12.5 }`),
/// which is the shape `specta` generates for the TS union.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum Value {
    /// A single physical scalar (already `(raw + translate) * scale`).
    Scalar(f64),
    /// A physical array/table of scalars, row-major.
    Array(Vec<f64>),
    /// A bitfield/enum's raw selected index (see `ConstantKind::Bits::options`).
    Enum(u32),
    /// A fixed-length text value.
    Text(String),
}
