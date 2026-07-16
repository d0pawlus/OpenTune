// SPDX-License-Identifier: GPL-3.0-or-later
//! The M2 `Definition` contract — the frozen shape of a fully parsed INI.
//!
//! `Definition` is the seam every downstream M2 task (expression evaluation,
//! page I/O, the tune model, UI rendering) builds against. Parsing (filling
//! in [`parse_definition`]) is a separate task; this module only freezes the
//! shape.

use crate::constants_parser::parse_constants;
use crate::gauges_parser::parse_gauges;
use crate::output_channels_parser::parse_output_channels;
use crate::preprocessor::preprocess_with_diagnostics;
use crate::ui_parser::parse_ui;
use crate::ve_analyze_parser::parse_ve_analyze;
use crate::{
    CommsSettings, ConstantDef, CurveDef, Diagnostic, DialogDef, FrontPageDef, GaugeDef, IniError,
    MenuDef, OutputChannelDef, TableDef, VeAnalyzeDef,
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
    /// `[ConstantsExtensions]` `defaultValue = name, value` pairs — the
    /// starting values of [`Self::pc_variables`] (labels unquoted). MS3-era
    /// bounds/scale expressions (`{ rpmhigh }`) resolve against these.
    pub pc_defaults: Vec<(String, String)>,
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
    /// `[OutputChannels]` entries. Realtime frames (M3) decode against these.
    /// Look up by name via [`Definition::output_channel`].
    pub output_channels: Vec<OutputChannelDef>,
    /// `[GaugeConfigurations]` entries backing the default dashboard.
    pub gauges: Vec<GaugeDef>,
    /// `[FrontPage]` — the default dashboard layout. Empty `Vec`s when the INI
    /// declares no `[FrontPage]`.
    pub frontpage: FrontPageDef,
    /// `[VeAnalyze]` binding; `None` when the INI declares none.
    pub ve_analyze: Option<VeAnalyzeDef>,
}

impl Definition {
    /// Look up a constant by name.
    ///
    /// Searches [`Definition::constants`] only — [`Definition::pc_variables`]
    /// is a separate namespace and is not searched here.
    pub fn constant(&self, name: &str) -> Option<&ConstantDef> {
        self.constants.iter().find(|c| c.name == name)
    }

    /// Look up an output channel by name (mirrors [`Definition::constant`]).
    pub fn output_channel(&self, name: &str) -> Option<&OutputChannelDef> {
        self.output_channels.iter().find(|c| c.name() == name)
    }

    /// Look up a table editor by name (mirrors [`Definition::constant`]).
    pub fn table(&self, name: &str) -> Option<&TableDef> {
        self.tables.iter().find(|t| t.name == name)
    }

    /// Look up a curve editor by name (mirrors [`Definition::constant`]).
    pub fn curve(&self, name: &str) -> Option<&CurveDef> {
        self.curves.iter().find(|c| c.name == name)
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
/// When the caller *does* know the active symbols (TunerStudio projects
/// persist them in `project.properties` → `ecuSettings`), use
/// [`parse_definition_with_symbols`] instead.
///
/// UI sections (`menus`, `dialogs`, `tables`, `curves`) are parsed by
/// [`crate::ui_parser::parse_ui`] (Task 3). Expression *evaluation*
/// (resolving `Number::Expr` against other constants) is Task 2's scope;
/// this function only captures expressions as raw strings.
pub fn parse_definition(ini_text: &str) -> Result<Definition, IniError> {
    parse_definition_with_symbols(ini_text, &HashSet::new())
}

/// [`parse_definition`] with an explicit active-symbol set for the
/// preprocessor's `#if` gates. MS1's `mapBins`/`tpsBins` live behind
/// `#if SPEED_DENSITY`/`#elif ALPHA_N` — with an empty set BOTH vanish and
/// the VE table's yBins reference dangles.
pub fn parse_definition_with_symbols(
    ini_text: &str,
    active_symbols: &HashSet<String>,
) -> Result<Definition, IniError> {
    let preprocessed = preprocess_with_diagnostics(ini_text, active_symbols);
    let preprocessor_diagnostics = preprocessed.diagnostics;
    let preprocessed = preprocessed.text;

    let comms = crate::parse_comms(&preprocessed)?;
    let parsed = parse_constants(&preprocessed)?;
    let ui = parse_ui(&preprocessed, &parsed.constants, &parsed.pc_variables);
    let output_channels = parse_output_channels(&preprocessed, comms.och_block_size)?;
    let gauges = parse_gauges(&preprocessed, &output_channels.channels);
    let ve_analyze = parse_ve_analyze(&preprocessed);

    let endianness = parsed.endianness.unwrap_or(comms.endianness);
    // `[OutputChannels]` may declare its own `ochGetCommand` (the windowed
    // template TunerStudio actually sends); it overrides the bare
    // `[MegaTune]`/`[TunerStudio]` value when present (M3 Task 6 blocker a).
    let och_get_command = crate::parser::extract_och_get_command(&preprocessed)
        .unwrap_or_else(|| comms.och_get_command.clone());
    let comms = CommsSettings {
        endianness,
        och_get_command,
        ..comms
    };

    let mut diagnostics = preprocessor_diagnostics;
    diagnostics.extend(parsed.diagnostics);
    diagnostics.extend(ui.diagnostics);
    diagnostics.extend(output_channels.diagnostics);
    diagnostics.extend(gauges.diagnostics);
    diagnostics.extend(ve_analyze.diagnostics);

    Ok(Definition {
        comms,
        pages: parsed.pages,
        constants: parsed.constants,
        pc_variables: parsed.pc_variables,
        pc_defaults: parsed.pc_defaults,
        menus: ui.menus,
        dialogs: ui.dialogs,
        tables: ui.tables,
        curves: ui.curves,
        diagnostics,
        output_channels: output_channels.channels,
        gauges: gauges.gauges,
        frontpage: gauges.frontpage,
        ve_analyze: ve_analyze.def,
    })
}
