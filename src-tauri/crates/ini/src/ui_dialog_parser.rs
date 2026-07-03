// SPDX-License-Identifier: GPL-3.0-or-later
//! `[UserDefined]` (`dialog`/`panel`/`field`/...) section parser — split out
//! of `ui_parser.rs` to keep each file focused (see sub-step 3.3).
//!
//! Port source (ADR-0006): [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini)
//! (MIT) — `parseDialogs` establishes `dialog`/`panel`/`field` field order
//! and the tolerated `field = "Label", name, {}, {cond}` 4-arg placeholder
//! form: hyper-tuner's own comment flags this as "probably a mistake" but
//! still takes the **last** brace group as the condition and ignores the
//! empty placeholder; this port does the same.
//!
//! Extended beyond hyper-tuner (its own `// TODO: missing fields` marks
//! `settingSelector`/`commandButton`/`displayOnlyField` as unimplemented;
//! `slider` is absent entirely) — real `speeduino.ini` uses all four. Per
//! the controller-resolution mapping recorded in the M2 task brief:
//! - `slider = "Label", constName, ...` and `displayOnlyField = "Label",
//!   constName, ...` reference a constant and degrade faithfully to
//!   [`FieldKind::Constant`] — a slider/display-only affordance over the
//!   same bound value the frozen shape already represents.
//! - `settingSelector` does not name a single bound constant (it presets
//!   several fields at once) and `commandButton` triggers an ECU command —
//!   neither has a faithful frozen `FieldKind`, so both are dropped with a
//!   [`Diagnostic`] rather than inventing a new field kind.
//! - Any other unrecognised leading keyword inside `[UserDefined]` (besides
//!   `dialog`/`panel`/`field`/`topicHelp`) degrades the same way: it produces
//!   no dialog field and is recorded as a [`Diagnostic`] naming the unknown
//!   keyword, rather than being silently dropped.

use crate::ui::{Diagnostic, DialogDef, DialogField, FieldKind};
use crate::ui_tokens::{brace_expr, is_brace_token, split_tokens, unquote};

pub(crate) fn parse_dialog_line(
    key: &str,
    value: &str,
    dialogs: &mut Vec<DialogDef>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match key {
        "dialog" => {
            let tokens = split_tokens(value);
            let Some(name) = tokens.first() else {
                return;
            };
            let title = tokens.get(1).map(|t| unquote(t)).unwrap_or_default();
            dialogs.push(DialogDef {
                name: name.clone(),
                title,
                fields: Vec::new(),
            });
        }
        "panel" => parse_panel_line(value, dialogs, diagnostics),
        "field" => parse_field_line(value, dialogs, diagnostics),
        "commandButton" => {
            record_unrepresentable_field(value, "commandButton", diagnostics);
        }
        "settingSelector" => {
            record_unrepresentable_field(value, "settingSelector", diagnostics);
        }
        "slider" => {
            parse_constant_backed_field(value, dialogs, diagnostics, "slider");
        }
        "displayOnlyField" => {
            parse_constant_backed_field(value, dialogs, diagnostics, "displayOnlyField");
        }
        "topicHelp" => {} // Informational only; no representable target.
        _ => record_unknown_keyword(key, value, diagnostics),
    }
}

/// `panel = name[, layout][, {cond}]` — layout is tolerated and dropped; a
/// trailing brace token (in either position) is the visibility condition.
fn parse_panel_line(value: &str, dialogs: &mut [DialogDef], diagnostics: &mut Vec<Diagnostic>) {
    let Some(dialog) = dialogs.last_mut() else {
        diagnostics.push(Diagnostic {
            section: "UserDefined".to_string(),
            detail: "panel outside any dialog".to_string(),
        });
        return;
    };
    let tokens = split_tokens(value);
    let Some(name) = tokens.first() else {
        return;
    };
    let visible = tokens.iter().skip(1).find_map(|t| brace_expr(t));
    dialog.fields.push(DialogField {
        kind: FieldKind::Panel(name.clone()),
        visible,
        enable: None,
    });
}

/// `field = "Label"` (label-only, no bound value) or `field = "Label",
/// name[, {}][, {cond}]`. Tolerates the 3- and 4-arg placeholder forms:
/// whichever trailing token is a non-empty brace expression is the
/// `visible` condition (the empty `{}` placeholder is ignored). A label
/// with no `name` token is a static [`FieldKind::Label`]; a completely
/// empty label is a [`FieldKind::Gap`].
fn parse_field_line(value: &str, dialogs: &mut [DialogDef], diagnostics: &mut Vec<Diagnostic>) {
    let Some(dialog) = dialogs.last_mut() else {
        diagnostics.push(Diagnostic {
            section: "UserDefined".to_string(),
            detail: "field outside any dialog".to_string(),
        });
        return;
    };
    let tokens = split_tokens(value);
    let Some(label_tok) = tokens.first() else {
        return;
    };
    let label = unquote(label_tok);

    // The visible condition is the LAST token if it's a non-empty brace
    // expression (matches hyper-tuner: it takes the final condition slot
    // regardless of how many placeholder braces precede it).
    let visible = tokens.last().and_then(|t| brace_expr(t));

    // The constant name is the second token, but only if it isn't itself a
    // brace expression (i.e. `field = "Label", {cond}` with no name).
    let name_tok = tokens.get(1).filter(|t| !is_brace_token(t));

    let kind = match name_tok {
        Some(name) => FieldKind::Constant(name.clone()),
        None if label.is_empty() => FieldKind::Gap,
        None => FieldKind::Label(label),
    };

    dialog.fields.push(DialogField {
        kind,
        visible,
        enable: None,
    });
}

/// `slider`/`displayOnlyField = "Label", name, ...` — both reference a bound
/// constant and degrade faithfully to [`FieldKind::Constant`] per the
/// controller-resolution mapping. Any trailing brace token is `visible`.
fn parse_constant_backed_field(
    value: &str,
    dialogs: &mut [DialogDef],
    diagnostics: &mut Vec<Diagnostic>,
    keyword: &str,
) {
    let Some(dialog) = dialogs.last_mut() else {
        diagnostics.push(Diagnostic {
            section: "UserDefined".to_string(),
            detail: format!("{keyword} outside any dialog"),
        });
        return;
    };
    let tokens = split_tokens(value);
    let Some(name_tok) = tokens.get(1).filter(|t| !is_brace_token(t)) else {
        diagnostics.push(Diagnostic {
            section: "UserDefined".to_string(),
            detail: format!("{keyword} has no bound constant name: `{value}`"),
        });
        return;
    };
    let visible = tokens.iter().skip(2).find_map(|t| brace_expr(t));
    dialog.fields.push(DialogField {
        kind: FieldKind::Constant(name_tok.clone()),
        visible,
        enable: None,
    });
}

/// `commandButton`/`settingSelector` have no faithful frozen `FieldKind` —
/// record a `Diagnostic` naming the keyword and (when present) the bound
/// name, and skip the field entirely.
fn record_unrepresentable_field(value: &str, keyword: &str, diagnostics: &mut Vec<Diagnostic>) {
    let tokens = split_tokens(value);
    let bound = tokens
        .get(1)
        .filter(|t| !is_brace_token(t))
        .cloned()
        .unwrap_or_default();
    let detail = if bound.is_empty() {
        format!("unrepresentable field kind `{keyword}`: `{value}`")
    } else {
        format!("unrepresentable field kind `{keyword}` (bound to `{bound}`): `{value}`")
    };
    diagnostics.push(Diagnostic {
        section: "UserDefined".to_string(),
        detail,
    });
}

/// Any leading keyword inside `[UserDefined]` that isn't one of the
/// recognised dialog keywords (`dialog`/`panel`/`field`/`commandButton`/
/// `settingSelector`/`slider`/`displayOnlyField`/`topicHelp`) — record a
/// `Diagnostic` naming the keyword and its raw value rather than silently
/// dropping the line.
fn record_unknown_keyword(keyword: &str, value: &str, diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.push(Diagnostic {
        section: "UserDefined".to_string(),
        detail: format!("unknown dialog keyword `{keyword}`: `{value}`"),
    });
}
