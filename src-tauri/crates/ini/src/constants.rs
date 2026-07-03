// SPDX-License-Identifier: GPL-3.0-or-later
//! M2 `[Constants]` types — a single tunable/lookup value inside a page.
//!
//! Real INIs put *expressions* where numbers are expected (e.g.
//! `scale = { 0.1 / stoich }`, `high = { boostTableLimit }` in `speeduino.ini`).
//! [`Number`] captures both the common literal case and the deferred
//! expression case so the seam can freeze before the Task 2 expression
//! evaluator exists.

/// A number that is either a resolved literal or an unevaluated expression.
///
/// `Lit` is the common case and is resolved fully on the critical path.
/// `Expr` defers evaluation to the Task 2 expression evaluator, which
/// resolves the raw string against a value lookup (e.g. other constants).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub enum Number {
    /// A literal numeric value, already resolved at parse time.
    Lit(f64),
    /// A raw, unevaluated expression string (e.g. `"0.1 / stoich"`).
    Expr(String),
}

/// The primitive scalar storage type of a constant, array element, or bit
/// field, matching the INI `type` keyword (e.g. `U08`, `S16`, `F32`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
pub enum ScalarType {
    /// Unsigned 8-bit integer.
    U08,
    /// Signed 8-bit integer.
    S08,
    /// Unsigned 16-bit integer.
    U16,
    /// Signed 16-bit integer.
    S16,
    /// Unsigned 32-bit integer.
    U32,
    /// Signed 32-bit integer.
    S32,
    /// IEEE-754 32-bit float.
    F32,
}

/// The dimensions of an array-typed constant. `cols == 1` means a 1-D array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct Shape {
    /// Number of rows (or element count, for a 1-D array).
    pub rows: usize,
    /// Number of columns; `1` for a 1-D array.
    pub cols: usize,
}

/// How the raw bytes of a constant are interpreted.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub enum ConstantKind {
    /// A single scalar value.
    Scalar(ScalarType),
    /// A fixed-shape array of scalars (1-D or 2-D).
    Array {
        /// The element type stored in the array.
        elem: ScalarType,
        /// The array's dimensions.
        shape: Shape,
    },
    /// A bitfield packed into a wider storage type, with named options for
    /// each possible value (e.g. an enum-like selector).
    Bits {
        /// The underlying storage type the bits are packed into.
        storage: ScalarType,
        /// Lowest bit index (inclusive) occupied by this field.
        bit_lo: u8,
        /// Highest bit index (inclusive) occupied by this field.
        bit_hi: u8,
        /// Human-readable labels for each possible bit-field value, indexed
        /// by the raw value.
        options: Vec<String>,
    },
    /// A fixed-length ASCII/text field.
    Text {
        /// Length in bytes of the text field.
        len: usize,
    },
}

/// A single named constant (a tunable value or lookup) defined inside a page.
///
/// Looked up by name via [`Definition::constant`](crate::Definition::constant).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct ConstantDef {
    /// The constant's name, as declared in the INI (e.g. `"rpmk"`).
    pub name: String,
    /// The page number this constant lives on.
    pub page: u16,
    /// Byte offset within the page. Resolves the INI `lastOffset` keyword to
    /// the running offset counter at parse time — this is a concrete byte
    /// offset, not an expression.
    pub offset: usize,
    /// The storage/interpretation kind (scalar, array, bits, or text).
    pub kind: ConstantKind,
    /// Multiplier applied to the raw stored value: `physical = raw * scale + translate`.
    pub scale: Number,
    /// Offset applied to the raw stored value: `physical = raw * scale + translate`.
    pub translate: Number,
    /// The unit label shown in the UI (e.g. `"RPM"`, `"deg"`).
    pub units: String,
    /// Lower bound of the physical value's valid range.
    pub low: Number,
    /// Upper bound of the physical value's valid range.
    pub high: Number,
    /// Number of decimal digits to display in the UI.
    pub digits: u8,
}
