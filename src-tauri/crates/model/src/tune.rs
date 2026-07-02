// SPDX-License-Identifier: GPL-3.0-or-later
//! The M2 `Tune` contract — an in-memory, editable snapshot of ECU page bytes.
//!
//! `Tune` is the frozen seam every downstream M2 task (page I/O, constant
//! get/set, burn, undo/redo) builds against. Only [`Tune::new`],
//! [`Tune::is_dirty`], and [`Tune::page_bytes`] are implemented here; the
//! remaining methods are `todo!()` until their owning tasks land.

use std::sync::Arc;

use opentune_ini::Definition;

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
    def: Arc<Definition>,
    /// One byte buffer per page, indexed in the same order as `def.pages`.
    pages: Vec<Vec<u8>>,
    /// Pages with at least one edit since the last [`Tune::mark_burned`].
    dirty: Vec<u16>,
    /// Edits available to undo, most-recent last.
    // Unread until `Tune::set`/`Tune::undo` are implemented (see their
    // `todo!()` bodies below); this is the stub-body warning the task brief
    // calls out, not dead functionality.
    #[allow(dead_code)]
    undo: Vec<Edit>,
    /// Edits available to redo, most-recent last.
    #[allow(dead_code)]
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
    /// Not yet implemented.
    pub fn load_page(&mut self, page: u16, bytes: Vec<u8>) {
        let _ = (page, bytes);
        todo!("load_page: replace page bytes from a protocol read")
    }

    /// Read and decode a constant's current value by name.
    ///
    /// Not yet implemented.
    pub fn get(&self, name: &str) -> Result<Value, ModelError> {
        let _ = name;
        todo!("get: decode a constant's value from page bytes")
    }

    /// Encode and write a constant's value by name.
    ///
    /// Not yet implemented.
    pub fn set(&mut self, name: &str, value: Value) -> Result<(), ModelError> {
        let _ = (name, value);
        todo!("set: encode and write a constant's value into page bytes")
    }

    /// Undo the most recent edit. Returns `false` if there was nothing to undo.
    ///
    /// Not yet implemented.
    pub fn undo(&mut self) -> bool {
        todo!("undo: pop and reverse-apply the most recent edit")
    }

    /// Redo the most recently undone edit. Returns `false` if there was
    /// nothing to redo.
    ///
    /// Not yet implemented.
    pub fn redo(&mut self) -> bool {
        todo!("redo: pop and reapply the most recently undone edit")
    }

    /// Whether any page has unburned edits.
    pub fn is_dirty(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// The page numbers with unburned edits.
    ///
    /// Not yet implemented.
    pub fn dirty_pages(&self) -> Vec<u16> {
        todo!("dirty_pages: return the page numbers with unburned edits")
    }

    /// Clear dirty tracking after a successful burn (RAM -> flash).
    ///
    /// Not yet implemented.
    pub fn mark_burned(&mut self) {
        todo!("mark_burned: clear dirty tracking after a successful burn")
    }

    /// The raw bytes of a page, keyed by page **number** (`PageDef::number`),
    /// not by its index in `def.pages`.
    ///
    /// # Panics
    /// Panics if `page` is not declared in the definition.
    pub fn page_bytes(&self, page: u16) -> &[u8] {
        let index = self
            .def
            .pages
            .iter()
            .position(|p| p.number == page)
            .unwrap_or_else(|| panic!("page {page} not present in definition"));
        &self.pages[index]
    }
}
