// SPDX-License-Identifier: GPL-3.0-or-later
//! The M2 `Tune` — an in-memory, editable snapshot of ECU page bytes.
//!
//! `Tune` decodes/encodes constants against its [`Definition`] (the scaled
//! accessors [`Tune::get`]/[`Tune::set`]), tracks RAM-vs-flash dirty state
//! per page, and keeps byte-level undo/redo stacks of [`Edit`] records.
//! Pure per-kind codec helpers live in [`crate::codec`].

use std::sync::Arc;

use opentune_ini::{ConstantDef, ConstantKind, Definition, Number, OutputChannelDef};

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
    /// The flash baseline: the bytes last known to be in flash, one buffer
    /// per page (same index as `pages`). Set at construction (zeroed),
    /// refreshed by [`Tune::load_page`] (a read establishes RAM == flash) and
    /// [`Tune::mark_burned`] (a burn commits RAM -> flash).
    ///
    /// A page is dirty *iff* its bytes differ from this baseline — derived,
    /// never tracked, so it cannot go stale. This is what makes
    /// load -> edit -> undo-back-to-loaded correctly report clean.
    baseline: Vec<Vec<u8>>,
    /// Edits available to undo, most-recent last.
    undo: Vec<Edit>,
    /// Edits available to redo, most-recent last.
    redo: Vec<Edit>,
}

impl Tune {
    /// Create a new tune with all pages zeroed, sized from `def.pages`.
    pub fn new(def: Arc<Definition>) -> Self {
        let pages: Vec<Vec<u8>> = def.pages.iter().map(|p| vec![0u8; p.size]).collect();
        Self {
            def,
            baseline: pages.clone(),
            pages,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Replace a page's bytes wholesale, e.g. after a protocol read.
    ///
    /// Loading is **not** an edit: it (re)establishes the flash baseline for
    /// this page — a fresh read means RAM == flash, so the loaded bytes
    /// become both the current bytes *and* the baseline, and the page reads
    /// clean. The undo/redo stacks **are** cleared — recorded edits reference
    /// byte state that no longer exists, so replaying them after a reload
    /// could silently corrupt the tune.
    ///
    /// ponytail: baseline-reset is safe only because `load_page` is called
    /// solely from a fresh-`Tune` full load (`Session::load_tune`) where the
    /// RAM==flash assumption holds. If a *partial* RAM refresh ever calls
    /// this on a tune with live edits, it would mark real divergence clean
    /// (losing edits). Upgrade path if that need arises: take an explicit
    /// `is_baseline: bool` and only reset the baseline on a full load.
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
        self.baseline[index] = bytes.clone();
        self.pages[index] = bytes;
        self.undo.clear();
        self.redo.clear();
    }

    /// Read and decode a constant's current value by name.
    ///
    /// Scalars and arrays return physical values (`(raw + translate) * scale`);
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
    /// A successful set that changes bytes records a byte-level [`Edit`] and
    /// clears the redo stack; the page then reads dirty because its bytes
    /// differ from the flash baseline. A set that encodes to the page's
    /// existing bytes records nothing (no edit, and the page stays clean).
    pub fn set(&mut self, name: &str, value: Value) -> Result<(), ModelError> {
        let c = self
            .def
            .constant(name)
            .ok_or_else(|| ModelError::UnknownConstant(name.to_string()))?
            .clone();
        let after = self.encode_value(&c, &value)?;

        let index = self.page_index(c.page);
        let before = self.pages[index][c.offset..c.offset + after.len()].to_vec();
        self.commit_bytes(c.page, c.offset, before, after);
        Ok(())
    }

    /// Set flat row-major cells of a named array constant. ONE undo [`Edit`]
    /// per call (a paste/smooth gesture is one undo step).
    ///
    /// Validates and encodes only the TOUCHED `(index, value)` pairs before
    /// touching any byte — an untouched stored cell outside the constant's
    /// declared `[low, high]` (a stale tune vs. a newer INI's bounds is a
    /// legitimate state; `load_page` never range-checks) must never fail a
    /// gesture that doesn't touch it, and its bytes stay byte-identical.
    pub fn set_cells(&mut self, name: &str, cells: &[(u32, f64)]) -> Result<(), ModelError> {
        if cells.is_empty() {
            return Ok(());
        }
        let c = self
            .def
            .constant(name)
            .ok_or_else(|| ModelError::UnknownConstant(name.to_string()))?
            .clone();
        let (elem, shape) = match &c.kind {
            ConstantKind::Array { elem, shape } => (*elem, *shape),
            _ => {
                return Err(ModelError::TypeMismatch(format!(
                    "`{name}` is not an array"
                )))
            }
        };
        let len = shape.rows * shape.cols;
        let width = codec::width(elem);
        let endian = self.def.comms.endianness;
        let scaling = self.scaling(&c)?;

        // Validate + encode only the touched indices — nothing is written
        // to `self.pages` until every touched cell has passed.
        let mut touched: Vec<(usize, Vec<u8>)> = Vec::with_capacity(cells.len());
        for (index, value) in cells {
            let i = *index as usize;
            if i >= len {
                return Err(ModelError::TypeMismatch(format!(
                    "`{name}`: cell index {i} out of bounds ({len} elements)"
                )));
            }
            let bytes = codec::encode_scalar(&c.name, &scaling, *value, elem, endian)?;
            touched.push((i, bytes));
        }

        let index = self.page_index(c.page);
        let region_len = len * width;
        let before = self.pages[index][c.offset..c.offset + region_len].to_vec();
        let mut after = before.clone();
        for (i, bytes) in &touched {
            let start = i * width;
            after[start..start + width].copy_from_slice(bytes);
        }
        self.commit_bytes(c.page, c.offset, before, after);
        Ok(())
    }

    /// Undo the most recent edit. Returns `false` if there was nothing to
    /// undo.
    ///
    /// Restores the edit's prior bytes. Dirty state follows the bytes: undoing
    /// back to the loaded/burned baseline reads clean, while undoing after a
    /// burn moves the page off the new baseline and re-dirties it.
    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.pop() else {
            return false;
        };
        self.apply(edit.page, edit.offset, &edit.before);
        self.redo.push(edit);
        true
    }

    /// Redo the most recently undone edit. Returns `false` if there was
    /// nothing to redo. Dirty state follows the bytes, like [`Tune::undo`].
    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.redo.pop() else {
            return false;
        };
        self.apply(edit.page, edit.offset, &edit.after);
        self.undo.push(edit);
        true
    }

    /// Whether any page's bytes differ from the flash baseline.
    pub fn is_dirty(&self) -> bool {
        self.pages != self.baseline
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

    /// The page numbers whose bytes differ from the flash baseline, sorted
    /// ascending.
    pub fn dirty_pages(&self) -> Vec<u16> {
        let mut pages: Vec<u16> = self
            .def
            .pages
            .iter()
            .enumerate()
            .filter(|(i, _)| self.pages[*i] != self.baseline[*i])
            .map(|(_, p)| p.number)
            .collect();
        pages.sort_unstable();
        pages
    }

    /// Commit the current bytes as the new flash baseline after a successful
    /// burn (RAM -> flash), clearing dirty state.
    ///
    /// Undo history survives a burn: undoing afterwards moves bytes off the
    /// new baseline and re-dirties the page.
    pub fn mark_burned(&mut self) {
        self.baseline = self.pages.clone();
    }

    /// Re-baseline the whole tune after a non-ECU full load (e.g. an offline
    /// `.msq` import applied via repeated [`Tune::set`] calls), clearing
    /// dirty state and undo/redo history.
    ///
    /// Mirrors [`Tune::load_page`]'s documented "loading is not an edit"
    /// contract at the whole-tune level: the current bytes become the new
    /// flash baseline, and the undo/redo stacks are cleared because their
    /// recorded edits are pseudo-edits produced by the load itself, not
    /// something the user did — replaying (or undoing) them would silently
    /// walk the tune toward the loader's per-field default instead of
    /// leaving values exactly as read from the file.
    pub fn mark_loaded(&mut self) {
        self.baseline = self.pages.clone();
        self.undo.clear();
        self.redo.clear();
    }

    /// The raw bytes of a page, keyed by page **number** (`PageDef::number`),
    /// not by its index in `def.pages`.
    ///
    /// # Panics
    /// Panics if `page` is not declared in the definition.
    pub fn page_bytes(&self, page: u16) -> &[u8] {
        &self.pages[self.page_index(page)]
    }

    /// The definition this tune's pages/constants are shaped by.
    ///
    /// Public accessor for out-of-crate consumers (e.g. `project`'s `.msq`
    /// read/write) that need the signature, pages, and constants but must
    /// not reach into `Tune`'s private byte state.
    pub fn definition(&self) -> &Definition {
        &self.def
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

    /// Splice `after` into a constant's page region and record one undo
    /// [`Edit`], unless `after` is byte-identical to `before` (a true no-op:
    /// no undo entry, and the page stays clean) — shared by [`Tune::set`] and
    /// [`Tune::set_cells`]. Dirty state is derived from the flash baseline, so
    /// there is nothing to flag here.
    fn commit_bytes(&mut self, page: u16, offset: usize, before: Vec<u8>, after: Vec<u8>) {
        if before == after {
            return;
        }
        let index = self.page_index(page);
        self.pages[index][offset..offset + after.len()].copy_from_slice(&after);
        self.undo.push(Edit {
            page,
            offset,
            before,
            after,
        });
        self.redo.clear();
    }

    /// Splice `bytes` into a page (shared by undo/redo). Dirty state is
    /// derived from the flash baseline, so there is nothing to track here.
    fn apply(&mut self, page: u16, offset: usize, bytes: &[u8]) {
        let index = self.page_index(page);
        self.pages[index][offset..offset + bytes.len()].copy_from_slice(bytes);
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
                // `opentune_ini::eval_with_functions` is the sandboxed INI
                // expression evaluator (closed arithmetic grammar, no code
                // execution, no I/O — see `ini/src/expr.rs`), not a general
                // eval.
                let lookup = |var: &str| self.lookup_expr_var(var);
                let funcs = |name: &str, args: &[f64]| self.channel_function(name, args);
                opentune_ini::eval_with_functions(expr, &lookup, &funcs).map_err(|err| {
                    ModelError::UnresolvedExpr(format!("`{owner}`: `{{ {expr} }}`: {err}"))
                })
            }
        }
    }

    /// Expression-variable lookup, in the order TunerStudio resolves names:
    /// page-backed constant → `[PcVariables]` entry (via its
    /// `[ConstantsExtensions]` `defaultValue`) → computed `[OutputChannels]`
    /// entry (evaluated recursively, depth-capped).
    ///
    /// The MS3 dialect needs the full chain: `fc_rpm`'s `high = { rpmhigh }`
    /// reads a pc_variable, and `launchvss_minvss`'s `scale =
    /// { msToPrefUnitsScale }` reads a computed channel that itself reads
    /// the `prefSpeedUnits` pc_variable.
    fn lookup_expr_var(&self, var: &str) -> Option<f64> {
        /// Real MS3 chains are 2 deep (`{clthighlim}` → `clt_exp`); the cap
        /// turns a self-referential computed channel into `UnresolvedExpr`
        /// instead of unbounded recursion.
        const MAX_EXPR_LOOKUP_DEPTH: u8 = 4;
        self.lookup_expr_var_depth(var, MAX_EXPR_LOOKUP_DEPTH)
    }

    fn lookup_expr_var_depth(&self, var: &str, depth: u8) -> Option<f64> {
        if depth == 0 {
            return None;
        }
        if let Some(c) = self.def.constant(var) {
            return self.page_backed_value(c);
        }
        if let Some(pc) = self.def.pc_variables.iter().find(|c| c.name == var) {
            return self.pc_variable_value(pc);
        }
        if let Some(OutputChannelDef::Computed { expr, .. }) = self.def.output_channel(var) {
            let lookup = |v: &str| self.lookup_expr_var_depth(v, depth - 1);
            let funcs = |name: &str, args: &[f64]| self.channel_function(name, args);
            // `opentune_ini::eval_with_functions` is the sandboxed INI
            // expression evaluator (closed arithmetic grammar, no code
            // execution, no I/O — see `ini/src/expr.rs`), not a general eval.
            return opentune_ini::eval_with_functions(expr, &lookup, &funcs).ok();
        }
        None
    }

    /// TunerStudio's `getChannel*ByOffset(offset)` builtins: metadata of
    /// the `[OutputChannels]` scalar declared at byte `offset` of the
    /// realtime block. MS3's generic PWM curves use them so a curve axis
    /// adopts the scaling of whichever load channel the tuner selected
    /// (`{ getChannelScaleByOffset(pwm_opt_load_a_offset) }`).
    ///
    /// ponytail: Min/Max approximate as the channel's encodable physical
    /// range (storage limits through its scale/translate) and Digits as 0 —
    /// the och grammar carries no such metadata, and this never clips a
    /// value the channel could actually represent. Revisit if a real INI
    /// pins tighter semantics.
    fn channel_function(&self, name: &str, args: &[f64]) -> Option<f64> {
        let &[offset] = args else { return None };
        if offset < 0.0 || offset.fract() != 0.0 {
            return None;
        }
        let (kind, scale, translate) = self.def.output_channels.iter().find_map(|ch| match ch {
            OutputChannelDef::Scalar {
                offset: o,
                kind,
                scale,
                translate,
                ..
            } if *o == offset as usize => Some((*kind, *scale, *translate)),
            _ => None,
        })?;
        let (raw_min, raw_max) = codec::raw_range(kind);
        let (a, b) = ((raw_min + translate) * scale, (raw_max + translate) * scale);
        match name {
            "getChannelScaleByOffset" => Some(scale),
            "getChannelTranslateByOffset" => Some(translate),
            "getChannelMinByOffset" => Some(a.min(b)),
            "getChannelMaxByOffset" => Some(a.max(b)),
            "getChannelDigitsByOffset" => Some(0.0),
            _ => None,
        }
    }

    /// A pc_variable's value: its `[ConstantsExtensions]` `defaultValue`,
    /// or 0 when none is declared (TunerStudio initializes pc_variables to
    /// zero / the first option). Bits defaults are stored as the option
    /// *label* (`defaultValue = clt_exp, "Expanded"`); a numeric index is
    /// accepted as fallback. Array/text pc_variables have no numeric value.
    ///
    /// Loading per-project overrides (`pcVariableValues.msq`) is a known
    /// follow-up; defaults match the shipped example projects.
    fn pc_variable_value(&self, pc: &ConstantDef) -> Option<f64> {
        let default = self
            .def
            .pc_defaults
            .iter()
            .find(|(name, _)| name == &pc.name)
            .map(|(_, value)| value.as_str());
        match &pc.kind {
            ConstantKind::Scalar(_) => default.map_or(Some(0.0), |v| v.trim().parse::<f64>().ok()),
            ConstantKind::Bits { options, .. } => {
                let Some(text) = default else {
                    return Some(0.0);
                };
                if let Some(idx) = options.iter().position(|o| o == text) {
                    return Some(idx as f64);
                }
                text.trim().parse::<f64>().ok()
            }
            _ => None,
        }
    }

    /// A page-backed constant's current physical value.
    ///
    /// Deliberately **Lit-only**: a referenced scalar whose own
    /// `scale`/`translate` is an `Expr` resolves to `None` instead of
    /// recursing into the evaluator. This bounds expression nesting to one
    /// level, which real INIs satisfy (e.g. `{ 0.1 / stoich }` where
    /// `stoich` is plain-scaled); anything deeper surfaces as
    /// `UnresolvedExpr` rather than risking unbounded recursion. `Bits`
    /// constants resolve to their selected index; arrays and text do not
    /// resolve.
    fn page_backed_value(&self, c: &ConstantDef) -> Option<f64> {
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

    /// The constant's resolved inclusive `[low, high]` range — the same
    /// bounds [`Tune::set`] validates against. Load paths (`.msq` import)
    /// use this to clamp file values like TunerStudio does, instead of
    /// rejecting them.
    pub fn bounds(&self, name: &str) -> Result<(f64, f64), ModelError> {
        let c = self
            .def
            .constant(name)
            .ok_or_else(|| ModelError::UnknownConstant(name.to_string()))?;
        Ok((
            self.resolve(&c.name, &c.low)?,
            self.resolve(&c.name, &c.high)?,
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
