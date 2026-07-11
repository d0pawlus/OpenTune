// SPDX-License-Identifier: GPL-3.0-or-later
//! The M2 `Tune` — an in-memory, editable snapshot of ECU page bytes.
//!
//! `Tune` decodes/encodes constants against its [`Definition`] (the scaled
//! accessors [`Tune::get`]/[`Tune::set`]), tracks RAM-vs-flash dirty state
//! per page, and keeps byte-level undo/redo stacks of [`Edit`] records.
//! Pure per-kind codec helpers live in [`crate::codec`].

use std::sync::Arc;

use opentune_ini::{ConstantDef, ConstantKind, Definition, Number};

use crate::codec;
use crate::edit::Edit;
use crate::value::Value;

/// Errors [`Tune::get`]/[`Tune::set`] can produce.
///
/// Derives `serde::Serialize` + `specta::Type` because the frontend surfaces
/// these over IPC when a get/set command fails.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub enum ModelError {
    /// No constant with this name exists in the [`Definition`].
    UnknownConstant(String),
    /// The physical value is outside the constant's declared `low..=high` range.
    OutOfRange {
        /// The constant's name.
        name: String,
        /// The out-of-range physical value that was rejected.
        value: f64,
    },
    /// The `Value` variant does not match the constant's `ConstantKind`.
    TypeMismatch(String),
    /// The constant's `scale`/`translate`/`low`/`high` is an unresolved
    /// [`Number::Expr`](opentune_ini::Number::Expr) and no evaluator was
    /// available to resolve it.
    UnresolvedExpr(String),
}

/// An in-memory, editable snapshot of an ECU tune's page bytes.
///
/// Holds one contiguous byte buffer per page (zeroed on construction),
/// plus dirty tracking and an undo/redo stack of [`Edit`] records.
#[derive(Debug, Clone, PartialEq)]
pub struct Tune {
    /// The definition this tune's pages/constants are shaped by.
    /// `pub(crate)` so `diff` (a sibling module) can walk its constants.
    pub(crate) def: Arc<Definition>,
    /// One byte buffer per page, indexed in the same order as `def.pages`.
    pages: Vec<Vec<u8>>,
    /// Pages with at least one edit since the last [`Tune::mark_burned`].
    ///
    /// Tracks page numbers only; the changed byte ranges themselves are
    /// recoverable from the `undo` stack (each [`Edit`] records its page,
    /// offset, and byte span).
    dirty: Vec<u16>,
    /// Edits available to undo, most-recent last.
    undo: Vec<Edit>,
    /// Edits available to redo, most-recent last.
    redo: Vec<Edit>,
}

impl Tune {
    /// Create a new tune with all pages zeroed, sized from `def.pages`.
    pub fn new(def: Arc<Definition>) -> Self {
        let pages = def.pages.iter().map(|p| vec![0u8; p.size]).collect();
        Self {
            def,
            pages,
            dirty: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Replace a page's bytes wholesale, e.g. after a protocol read.
    ///
    /// Loading is **not** an edit: dirty state is left untouched (re-reading
    /// ECU RAM does not change whether that RAM differs from flash). The
    /// undo/redo stacks **are** cleared — recorded edits reference byte
    /// state that no longer exists, so replaying them after a reload could
    /// silently corrupt the tune.
    ///
    /// # Panics
    /// Panics if `page` is not declared in the definition or `bytes` does
    /// not match the declared page size (both are caller bugs).
    pub fn load_page(&mut self, page: u16, bytes: Vec<u8>) {
        let index = self.page_index(page);
        assert_eq!(
            bytes.len(),
            self.def.pages[index].size,
            "page {page} load size mismatch"
        );
        self.pages[index] = bytes;
        self.undo.clear();
        self.redo.clear();
    }

    /// Read and decode a constant's current value by name.
    ///
    /// Scalars and arrays return physical values (`raw * scale + translate`);
    /// bits return the selected option index; text returns the bytes up to
    /// the first NUL, lossily decoded as UTF-8.
    pub fn get(&self, name: &str) -> Result<Value, ModelError> {
        let c = self
            .def
            .constant(name)
            .ok_or_else(|| ModelError::UnknownConstant(name.to_string()))?;
        let endian = self.def.comms.endianness;
        match &c.kind {
            ConstantKind::Scalar(ty) => {
                let (scale, translate) = self.scale_translate(c)?;
                let region = self.region(c, codec::width(*ty));
                Ok(codec::decode_scalar(region, *ty, endian, scale, translate))
            }
            ConstantKind::Array { elem, shape } => {
                let (scale, translate) = self.scale_translate(c)?;
                let region = self.region(c, codec::width(*elem) * shape.rows * shape.cols);
                Ok(codec::decode_array(region, *elem, endian, scale, translate))
            }
            ConstantKind::Bits {
                storage,
                bit_lo,
                bit_hi,
                ..
            } => {
                let region = self.region(c, codec::width(*storage));
                codec::decode_bits(&c.name, region, *storage, *bit_lo, *bit_hi, endian)
            }
            ConstantKind::Text { len } => Ok(codec::decode_text(self.region(c, *len))),
        }
    }

    /// Encode and write a constant's value by name.
    ///
    /// Physical values outside the constant's inclusive `[low, high]` range
    /// — or whose inverse-scaled raw does not fit the storage type — are
    /// rejected with [`ModelError::OutOfRange`]; no bytes change on any
    /// error. Integer raws round half away from zero; `F32` stores
    /// unrounded.
    ///
    /// A successful set that changes bytes records a byte-level [`Edit`],
    /// marks the page dirty, and clears the redo stack. A set that encodes
    /// to the page's existing bytes records nothing (no dirty, no edit).
    pub fn set(&mut self, name: &str, value: Value) -> Result<(), ModelError> {
        let c = self
            .def
            .constant(name)
            .ok_or_else(|| ModelError::UnknownConstant(name.to_string()))?
            .clone();
        let after = self.encode_value(&c, &value)?;

        let index = self.page_index(c.page);
        let range = c.offset..c.offset + after.len();
        let before = self.pages[index][range.clone()].to_vec();
        if before == after {
            return Ok(());
        }
        self.pages[index][range].copy_from_slice(&after);
        if !self.dirty.contains(&c.page) {
            self.dirty.push(c.page);
        }
        self.undo.push(Edit {
            page: c.page,
            offset: c.offset,
            before,
            after,
        });
        self.redo.clear();
        Ok(())
    }

    /// Undo the most recent edit. Returns `false` if there was nothing to
    /// undo.
    ///
    /// Restores the edit's prior bytes and marks the page dirty — including
    /// after a burn, where undoing correctly re-dirties the page (RAM
    /// differs from flash again).
    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.pop() else {
            return false;
        };
        self.apply(edit.page, edit.offset, &edit.before);
        self.redo.push(edit);
        true
    }

    /// Redo the most recently undone edit. Returns `false` if there was
    /// nothing to redo. Marks the page dirty, like [`Tune::undo`].
    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.redo.pop() else {
            return false;
        };
        self.apply(edit.page, edit.offset, &edit.after);
        self.undo.push(edit);
        true
    }

    /// Whether any page has unburned edits.
    pub fn is_dirty(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// Evaluate a `visible`/`enable` expression against this tune's current
    /// constant values, reusing the Task 2 sandboxed evaluator.
    ///
    /// A referenced name resolves to its current physical value via the same
    /// [`Self::lookup_expr_var`] used for `Expr`-scaled numbers, so dialog
    /// visibility is driven by exactly one source of truth (the Rust
    /// evaluator) rather than a duplicated TS port. Returns the evaluator's
    /// error on an unparseable/undefined expression; callers typically fail
    /// **open** (treat a broken condition as visible) so a bad INI expression
    /// never silently hides a field.
    pub fn eval_condition(&self, expr: &str) -> Result<bool, opentune_ini::ExprError> {
        let lookup = |var: &str| self.lookup_expr_var(var);
        opentune_ini::eval_bool(expr, &lookup)
    }

    /// Resolve one INI number/expression against the current tune.
    ///
    /// Used by backend-owned UI projections such as gauge bounds so the
    /// frontend never needs a second expression evaluator.
    pub fn resolve_number(&self, owner: &str, number: &Number) -> Result<f64, ModelError> {
        self.resolve(owner, number)
    }

    /// The page numbers with unburned edits, sorted ascending.
    pub fn dirty_pages(&self) -> Vec<u16> {
        let mut pages = self.dirty.clone();
        pages.sort_unstable();
        pages
    }

    /// Clear dirty tracking after a successful burn (RAM -> flash).
    ///
    /// Undo history survives a burn: undoing afterwards re-dirties the page.
    pub fn mark_burned(&mut self) {
        self.dirty.clear();
    }

    /// The raw bytes of a page, keyed by page **number** (`PageDef::number`),
    /// not by its index in `def.pages`.
    ///
    /// # Panics
    /// Panics if `page` is not declared in the definition.
    pub fn page_bytes(&self, page: u16) -> &[u8] {
        &self.pages[self.page_index(page)]
    }

    /// Index into `self.pages` for a page **number**.
    ///
    /// # Panics
    /// Panics if `page` is not declared in the definition.
    fn page_index(&self, page: u16) -> usize {
        self.def
            .pages
            .iter()
            .position(|p| p.number == page)
            .unwrap_or_else(|| panic!("page {page} not present in definition"))
    }

    /// The `len`-byte footprint of constant `c` within its page.
    ///
    /// # Panics
    /// Panics if the constant extends past its page — a definition
    /// inconsistency the parser never produces.
    fn region(&self, c: &ConstantDef, len: usize) -> &[u8] {
        let page = &self.pages[self.page_index(c.page)];
        assert!(
            c.offset + len <= page.len(),
            "constant `{}` extends past page {}",
            c.name,
            c.page
        );
        &page[c.offset..c.offset + len]
    }

    /// Splice `bytes` into a page and mark it dirty (shared by undo/redo).
    fn apply(&mut self, page: u16, offset: usize, bytes: &[u8]) {
        let index = self.page_index(page);
        self.pages[index][offset..offset + bytes.len()].copy_from_slice(bytes);
        if !self.dirty.contains(&page) {
            self.dirty.push(page);
        }
    }

    /// Encode `value` into the byte image of constant `c` (the constant's
    /// full footprint, ready to splice into its page). Pure with respect to
    /// `self` — no bytes are written here.
    fn encode_value(&self, c: &ConstantDef, value: &Value) -> Result<Vec<u8>, ModelError> {
        let endian = self.def.comms.endianness;
        match (&c.kind, value) {
            (ConstantKind::Scalar(ty), Value::Scalar(x)) => {
                codec::encode_scalar(&c.name, &self.scaling(c)?, *x, *ty, endian)
            }
            (ConstantKind::Array { elem, shape }, Value::Array(xs)) => {
                codec::encode_array(&c.name, &self.scaling(c)?, xs, *elem, *shape, endian)
            }
            (
                ConstantKind::Bits {
                    storage,
                    bit_lo,
                    bit_hi,
                    options,
                },
                Value::Enum(index),
            ) => {
                let current = self.region(c, codec::width(*storage));
                codec::encode_bits(
                    &c.name, current, *storage, *bit_lo, *bit_hi, options, *index, endian,
                )
            }
            (ConstantKind::Text { len }, Value::Text(text)) => {
                codec::encode_text(&c.name, text, *len)
            }
            (kind, _) => Err(ModelError::TypeMismatch(format!(
                "`{}`: expected a {} value",
                c.name,
                codec::kind_label(kind)
            ))),
        }
    }

    /// Resolve a [`Number`] to a concrete value.
    ///
    /// `Lit` is returned directly — the common numeric path **never**
    /// invokes the evaluator. `Expr` goes through the Task 2 evaluator with
    /// a `Tune`-backed lookup; any failure surfaces as
    /// [`ModelError::UnresolvedExpr`] (a diagnostic, never a panic).
    fn resolve(&self, owner: &str, number: &Number) -> Result<f64, ModelError> {
        match number {
            Number::Lit(value) => Ok(*value),
            Number::Expr(expr) => {
                // `opentune_ini::eval` is the sandboxed INI expression
                // evaluator (closed arithmetic grammar, no code execution,
                // no I/O — see `ini/src/expr.rs`), not a general eval.
                let lookup = |var: &str| self.lookup_expr_var(var);
                opentune_ini::eval(expr, &lookup).map_err(|err| {
                    ModelError::UnresolvedExpr(format!("`{owner}`: `{{ {expr} }}`: {err}"))
                })
            }
        }
    }

    /// Expression-variable lookup: a constant name resolves to its current
    /// physical value.
    ///
    /// Deliberately **Lit-only**: a referenced scalar whose own
    /// `scale`/`translate` is an `Expr` resolves to `None` instead of
    /// recursing into the evaluator. This bounds expression nesting to one
    /// level, which real INIs satisfy (e.g. `{ 0.1 / stoich }` where
    /// `stoich` is plain-scaled); anything deeper surfaces as
    /// `UnresolvedExpr` rather than risking unbounded recursion. `Bits`
    /// constants resolve to their selected index; arrays, text, and PC
    /// variables do not resolve.
    fn lookup_expr_var(&self, var: &str) -> Option<f64> {
        let c = self.def.constant(var)?;
        let index = self.def.pages.iter().position(|p| p.number == c.page)?;
        let page = &self.pages[index];
        let endian = self.def.comms.endianness;
        match &c.kind {
            ConstantKind::Scalar(ty) => {
                let width = codec::width(*ty);
                let region = page.get(c.offset..c.offset + width)?;
                let (Number::Lit(scale), Number::Lit(translate)) = (&c.scale, &c.translate) else {
                    return None;
                };
                match codec::decode_scalar(region, *ty, endian, *scale, *translate) {
                    Value::Scalar(v) => Some(v),
                    _ => None,
                }
            }
            ConstantKind::Bits {
                storage,
                bit_lo,
                bit_hi,
                ..
            } => {
                let region = page.get(c.offset..c.offset + codec::width(*storage))?;
                match codec::decode_bits(&c.name, region, *storage, *bit_lo, *bit_hi, endian) {
                    Ok(Value::Enum(v)) => Some(f64::from(v)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Resolve the two read-path numbers for a constant.
    fn scale_translate(&self, c: &ConstantDef) -> Result<(f64, f64), ModelError> {
        Ok((
            self.resolve(&c.name, &c.scale)?,
            self.resolve(&c.name, &c.translate)?,
        ))
    }

    /// Resolve all four write-path numbers for a constant.
    fn scaling(&self, c: &ConstantDef) -> Result<codec::Scaling, ModelError> {
        Ok(codec::Scaling {
            scale: self.resolve(&c.name, &c.scale)?,
            translate: self.resolve(&c.name, &c.translate)?,
            low: self.resolve(&c.name, &c.low)?,
            high: self.resolve(&c.name, &c.high)?,
        })
    }
}
