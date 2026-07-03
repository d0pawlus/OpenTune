// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared line-tokenising helpers for the `[Menu]`/`[UserDefined]`/
//! `[TableEditor]`/`[CurveEditor]` parsers — split out of `ui_parser.rs` to
//! keep each file focused (see sub-step 3.3).
//!
//! Structurally mirrors `constants_fields::split_fields` (quote/brace-aware
//! comma splitting), duplicated here rather than shared since the UI
//! grammar and the constants grammar are parsed independently and a shared
//! helper would couple the two modules for no real benefit.

/// Split a value tail into comma-separated tokens, respecting quoted
/// strings and `{ ... }` expressions (which may themselves be the whole
/// token, e.g. a trailing `{ cond }`).
pub(crate) fn split_tokens(value: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut brace_depth = 0u32;

    for ch in value.chars() {
        match ch {
            '"' => {
                in_quote = !in_quote;
                current.push(ch);
            }
            '{' if !in_quote => {
                brace_depth += 1;
                current.push(ch);
            }
            '}' if !in_quote => {
                brace_depth = brace_depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if !in_quote && brace_depth == 0 => {
                fields.push(current.trim().to_string());
                current = String::new();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() || !fields.is_empty() {
        fields.push(current.trim().to_string());
    }
    fields
}

/// Strip a trailing `; …` inline comment, honoring quoted strings.
pub(crate) fn strip_inline_comment(s: &str) -> &str {
    let mut in_quote = false;
    for (i, ch) in s.char_indices() {
        match ch {
            '"' => in_quote = !in_quote,
            ';' if !in_quote => return &s[..i],
            _ => {}
        }
    }
    s
}

pub(crate) fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Strip a `{ ... }` expression wrapper, trimming inner whitespace. Returns
/// `None` if `s` isn't a brace expression (including the empty `{}`
/// placeholder — callers that want to distinguish "absent" from "empty
/// placeholder" check [`is_brace_token`] directly).
pub(crate) fn brace_expr(s: &str) -> Option<String> {
    let inner = s.trim().strip_prefix('{')?.strip_suffix('}')?;
    let inner = inner.trim();
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_string())
    }
}

pub(crate) fn is_brace_token(s: &str) -> bool {
    let t = s.trim();
    t.starts_with('{') && t.ends_with('}')
}
