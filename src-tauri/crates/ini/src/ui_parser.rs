// SPDX-License-Identifier: GPL-3.0-or-later
//! `[Menu]` / `[UserDefined]` / `[TableEditor]` / `[CurveEditor]` section
//! parser — sub-step 3.3.
//!
//! This module owns the section dispatch loop and `[Menu]` parsing;
//! `[UserDefined]` lives in `ui_dialog_parser.rs` and
//! `[TableEditor]`/`[CurveEditor]` in `ui_table_curve_parser.rs` — split out
//! to keep each file focused. Shared line-tokenising helpers live in
//! `ui_tokens.rs`. See those modules' doc comments for the full port-vs-
//! extension breakdown per section.
//!
//! Port source (ADR-0006): [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini)
//! (MIT) — `parseMenu` establishes the `subMenu = name, "Title", page,
//! {cond}` field order this module tolerates: hyper-tuner tries the fullest
//! form first and falls back to shorter forms; this port mirrors that by
//! parsing trailing tokens positionally and tolerating their absence.
//!
//! `subMenu = std_separator` and `groupMenu`/`groupChildMenu` lines have no
//! representable target under the frozen [`MenuItem`] shape (a label-only
//! separator; a two-level grouping the frozen tree has no slot for) and are
//! skipped silently — a separator is not a parse failure worth surfacing as
//! a `Diagnostic`, and `groupMenu` is out of this task's fixture scope.

use crate::ui::{CurveDef, Diagnostic, DialogDef, MenuDef, MenuItem, TableDef};
use crate::ui_dialog_parser::parse_dialog_line;
use crate::ui_table_curve_parser::{parse_curve_line, parse_table_line};
use crate::ui_tokens::{split_tokens, strip_inline_comment, unquote};
use crate::ConstantDef;

/// The result of parsing the `[Menu]`/`[UserDefined]`/`[TableEditor]`/
/// `[CurveEditor]` sections.
pub struct ParsedUi {
    pub menus: Vec<MenuDef>,
    pub dialogs: Vec<DialogDef>,
    pub tables: Vec<TableDef>,
    pub curves: Vec<CurveDef>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Section {
    Menu,
    UserDefined,
    TableEditor,
    CurveEditor,
    Other,
}

/// Parse every `[Menu]`, `[UserDefined]`, `[TableEditor]`, and
/// `[CurveEditor]` section in the (already-preprocessed) INI text.
///
/// `constants` is used only for the table/curve bin cross-reference check —
/// a missing reference degrades to a `Diagnostic`, never a hard error.
pub fn parse_ui(ini_text: &str, constants: &[ConstantDef]) -> ParsedUi {
    let mut menus: Vec<MenuDef> = Vec::new();
    let mut dialogs: Vec<DialogDef> = Vec::new();
    let mut tables: Vec<TableDef> = Vec::new();
    let mut curves: Vec<CurveDef> = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    let mut section = Section::Other;

    for raw_line in ini_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = match inner.trim() {
                "Menu" => Section::Menu,
                "UserDefined" => Section::UserDefined,
                "TableEditor" => Section::TableEditor,
                "CurveEditor" => Section::CurveEditor,
                _ => Section::Other,
            };
            continue;
        }

        let line = strip_inline_comment(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = strip_inline_comment(value).trim();

        match section {
            Section::Menu => parse_menu_line(key, value, &mut menus),
            Section::UserDefined => parse_dialog_line(key, value, &mut dialogs, &mut diagnostics),
            Section::TableEditor => {
                parse_table_line(key, value, &mut tables, constants, &mut diagnostics)
            }
            Section::CurveEditor => {
                parse_curve_line(key, value, &mut curves, constants, &mut diagnostics)
            }
            Section::Other => {}
        }
    }

    ParsedUi {
        menus,
        dialogs,
        tables,
        curves,
        diagnostics,
    }
}

// ── [Menu] ───────────────────────────────────────────────────────────────

/// Parse one `[Menu]` line. Only `menu = "Label"` (starts a new top-level
/// menu) and `subMenu = name, "Title", ...` (appends a [`MenuItem`] to the
/// current menu) are represented under the frozen [`MenuDef`]/[`MenuItem`]
/// shapes. `menuDialog = main` (the root dialog pointer), `subMenu =
/// std_separator` (no title, a layout separator), and `groupMenu`/
/// `groupChildMenu` (a grouping level the frozen tree has no slot for) are
/// tolerated and skipped.
fn parse_menu_line(key: &str, value: &str, menus: &mut Vec<MenuDef>) {
    match key {
        "menu" => {
            // `menu = "&Tuning"` — the `&` mnemonic-accelerator marker (a
            // Windows menu convention TunerStudio inherits) is stripped;
            // it has no meaning as a plain label.
            let tokens = split_tokens(value);
            let Some(title_tok) = tokens.first() else {
                return;
            };
            let label = unquote(title_tok).replace('&', "");
            menus.push(MenuDef {
                label,
                items: Vec::new(),
            });
        }
        "subMenu" => parse_submenu_line(value, menus),
        _ => {} // menuDialog, groupMenu, groupChildMenu: no representable target (see module doc).
    }
}

fn parse_submenu_line(value: &str, menus: &mut [MenuDef]) {
    let Some(current) = menus.last_mut() else {
        return;
    };
    let tokens = split_tokens(value);
    let Some(name) = tokens.first() else {
        return;
    };
    if name == "std_separator" {
        return;
    }
    // `subMenu = name, "Title", page, {cond}` — title is required for a
    // representable MenuItem; page and condition are tolerated but dropped
    // (MenuItem only carries label + dialog).
    let Some(title_tok) = tokens.get(1) else {
        return;
    };
    let title = unquote(title_tok);
    current.items.push(MenuItem {
        label: title,
        dialog: name.clone(),
    });
}
