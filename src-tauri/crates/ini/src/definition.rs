// SPDX-License-Identifier: GPL-3.0-or-later
//! The M2 `Definition` contract — the frozen shape of a fully parsed INI.
//!
//! `Definition` is the seam every downstream M2 task (expression evaluation,
//! page I/O, the tune model, UI rendering) builds against. Parsing (filling
//! in [`parse_definition`]) is a separate task; this module only freezes the
//! shape.

use crate::constants_parser::parse_constants;
use crate::preprocessor::preprocess;
use crate::{
    CommsSettings, ConstantDef, CurveDef, Diagnostic, DialogDef, IniError, MenuDef, TableDef,
};
use std::collections::HashSet;

/// A single memory page (a contiguous block read from / written to the ECU).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct PageDef {
    /// The page number, as referenced by [`ConstantDef::page`].
    pub number: u16,
    /// The page size in bytes.
    pub size: usize,
}

/// A fully parsed firmware INI definition — the frozen M2 seam.
///
/// Holds everything needed to interpret a tune's raw bytes (`pages` +
/// `constants`) and to render the stock UI (`menus`, `dialogs`, `tables`,
/// `curves`). `diagnostics` surfaces INI sections that were skipped or
/// degraded during parsing rather than failing the whole parse.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct Definition {
    /// Communication settings, unchanged from the M1 contract.
    pub comms: CommsSettings,
    /// Every memory page declared by the INI.
    pub pages: Vec<PageDef>,
    /// Every tunable/lookup constant declared by the INI. Look up by name via
    /// [`Definition::constant`].
    pub constants: Vec<ConstantDef>,
    /// PC-side (host-only) variables — same shape as `constants` but never
    /// stored in ECU memory.
    pub pc_variables: Vec<ConstantDef>,
    /// Top-level menus for the stock UI.
    pub menus: Vec<MenuDef>,
    /// Dialogs referenced by menu items and panels.
    pub dialogs: Vec<DialogDef>,
    /// Table (2-D/3-D map) editor definitions.
    pub tables: Vec<TableDef>,
    /// Curve (2-D) editor definitions.
    pub curves: Vec<CurveDef>,
    /// Notes on INI sections that were skipped or could not be fully parsed.
    pub diagnostics: Vec<Diagnostic>,
}

impl Definition {
    /// Look up a constant by name.
    ///
    /// Searches [`Definition::constants`] only — [`Definition::pc_variables`]
    /// is a separate namespace and is not searched here.
    pub fn constant(&self, name: &str) -> Option<&ConstantDef> {
        self.constants.iter().find(|c| c.name == name)
    }
}

/// Parse a complete firmware INI into a [`Definition`].
///
/// Runs the symbol-based preprocessor first (see [`crate::preprocess`])
/// with an empty active-symbol set — real Speeduino `#if`/`#else` gates
/// reference build-profile symbols (`CELSIUS`, `mcu_stm32`, ...) that this
/// crate has no way to know without a target profile, so the `#else`
/// branch is taken wherever a plain `#if SYMBOL` gate appears. This
/// matches the "graceful degradation" contract: parsing still succeeds
/// and produces a usable `Definition`, just using the else-branch values.
///
/// UI sections (`menus`, `dialogs`, `tables`, `curves`) are left empty —
/// that is Task 3's scope. Expression *evaluation* (resolving
/// `Number::Expr` against other constants) is Task 2's scope; this
/// function only captures expressions as raw strings.
pub fn parse_definition(ini_text: &str) -> Result<Definition, IniError> {
    let active_symbols = HashSet::new();
    let preprocessed = preprocess(ini_text, &active_symbols);

    let comms = crate::parse_comms(&preprocessed)?;
    let parsed = parse_constants(&preprocessed)?;

    let endianness = parsed.endianness.unwrap_or(comms.endianness);
    let comms = CommsSettings {
        endianness,
        ..comms
    };

    Ok(Definition {
        comms,
        pages: parsed.pages,
        constants: parsed.constants,
        pc_variables: parsed.pc_variables,
        menus: Vec::new(),
        dialogs: Vec::new(),
        tables: Vec::new(),
        curves: Vec::new(),
        diagnostics: parsed.diagnostics,
    })
}
