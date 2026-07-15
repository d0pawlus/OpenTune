// SPDX-License-Identifier: GPL-3.0-or-later
//! Symbol-based INI preprocessor — sub-step 1.1.
//!
//! **Written fresh.** Neither reference source (ADR-0006) covers this
//! surface: `hyper-tuner/ini` only expands `#define` value lists, and the
//! real `speeduino.ini` gates dozens of constants behind `#if`/`#else`
//! blocks (`blockingFactor` ×57, `burnCommand` ×53 in the upstream
//! codebase) that hyper-tuner cannot resolve at all.
//!
//! Structural reference only: `adbancroft/TunerStudioIniParser`'s
//! `pre_processor.lark` grammar (LGPLv3) confirmed the directive set
//! (`#define`, `#set`, `#unset`, `#if`, `#ifdef`, `#ifndef`, `#elif`,
//! `#else`, `#endif`) and that conditionals may nest — no code was copied
//! from it.
//!
//! # Scope and limitations
//!
//! - `#if`/`#elif` conditions in real Speeduino INIs are always **bare
//!   symbols** (`LAMBDA`, `mcu_stm32`, `COMMS_COMPAT`), never arithmetic
//!   or boolean expressions, so this preprocessor only supports symbol
//!   membership tests (`SYMBOL` and `!SYMBOL`). No expression evaluator.
//! - `#include` is **not implemented**. Speeduino INIs use none; adding it
//!   would require a filesystem-aware API this crate does not have. This
//!   is a deliberate, documented limitation, not an oversight.
//! - `$name` references expand to the matching `#define`'s value text
//!   (rusEFI pin dictionaries: `#define gpio_list="NONE", ...` referenced
//!   as `$gpio_list` from bits option lists). A `$name` with no matching
//!   define is left untouched. This mirrors `hyper-tuner/ini`'s `#define`
//!   handling, the one preprocessor feature it does implement.

use crate::Diagnostic;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

/// One entry on the conditional-nesting stack while scanning.
struct Frame {
    /// Whether the currently-active branch of this frame should be kept,
    /// considering both this frame's own condition and every enclosing
    /// frame's `parent_active` state.
    branch_active: bool,
    /// Whether any branch in this `#if`/`#elif`/.../`#endif` chain has
    /// matched yet (subsequent `#elif`/`#else` become dead once true).
    matched: bool,
    /// Whether the *enclosing* scope is active. A nested frame can never
    /// emit lines the parent has already suppressed.
    parent_active: bool,
    /// Source line and directive that opened this frame, used for pointed
    /// diagnostics when EOF arrives before a matching `#endif`.
    opener_line: usize,
    opener: &'static str,
    /// Once `#else` has appeared, another `#else` or any `#elif` is invalid.
    saw_else: bool,
}

impl Frame {
    /// Whether lines under the current branch of this frame should be
    /// emitted, accounting for the enclosing scope.
    fn active(&self) -> bool {
        self.parent_active && self.branch_active
    }
}

/// Preprocess raw INI text against a set of active symbols.
///
/// Resolves `#define`/`#set`/`#unset` and `#if`/`#ifdef`/`#ifndef`/`#elif`/
/// `#else`/`#endif` blocks, emitting only the lines that survive. All
/// directive lines themselves are stripped from the output; downstream
/// parsers (comms, constants) never see a `#`.
///
/// `active_symbols` seeds the initial defined-symbol set (e.g. from a
/// build profile); `#define`/`#set`/`#unset` mutate a working copy as
/// scanning proceeds top-to-bottom.
pub fn preprocess(ini_text: &str, active_symbols: &HashSet<String>) -> String {
    preprocess_with_diagnostics(ini_text, active_symbols).text
}

pub(crate) struct Preprocessed {
    pub(crate) text: String,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

pub(crate) fn preprocess_with_diagnostics(
    ini_text: &str,
    active_symbols: &HashSet<String>,
) -> Preprocessed {
    let mut symbols: HashSet<String> = active_symbols.clone();
    let mut defines: HashMap<String, String> = HashMap::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut out_lines: Vec<Cow<'_, str>> = Vec::new();
    let mut diagnostics = Vec::new();

    for (line_index, raw_line) in ini_text.lines().enumerate() {
        let line_number = line_index + 1;
        let trimmed = raw_line.trim_start();
        let currently_active = stack.last().is_none_or(Frame::active);

        if let Some(directive) = parse_directive(trimmed) {
            apply_directive(
                directive,
                &mut symbols,
                &mut defines,
                &mut stack,
                currently_active,
                line_number,
                &mut diagnostics,
            );
            continue;
        }

        if currently_active {
            out_lines.push(expand_defines(raw_line, &defines));
        }
    }

    for frame in stack {
        diagnostics.push(preprocessor_diagnostic(format!(
            "line {}: unclosed {} directive (missing #endif)",
            frame.opener_line, frame.opener
        )));
    }

    Preprocessed {
        text: out_lines.join("\n"),
        diagnostics,
    }
}

/// A recognized preprocessor directive, with its payload already
/// extracted (but not yet interpreted against the symbol table).
enum Directive<'a> {
    Define {
        name: &'a str,
        /// The text after `=`, when present — the define's value list.
        value: Option<&'a str>,
    },
    Set(&'a str),
    Unset(&'a str),
    If(Condition<'a>),
    Ifdef(&'a str),
    Ifndef(&'a str),
    Elif(Condition<'a>),
    Else,
    Endif,
}

/// A bare symbol condition, optionally negated (`!SYMBOL`).
struct Condition<'a> {
    symbol: &'a str,
    negated: bool,
}

fn parse_directive(trimmed: &str) -> Option<Directive<'_>> {
    let rest = trimmed.strip_prefix('#')?;
    let rest = rest.trim_start();

    if let Some(arg) = strip_keyword(rest, "ifndef") {
        return Some(Directive::Ifndef(arg));
    }
    if let Some(arg) = strip_keyword(rest, "ifdef") {
        return Some(Directive::Ifdef(arg));
    }
    if let Some(arg) = strip_keyword(rest, "if") {
        return Some(Directive::If(parse_condition(arg)));
    }
    if let Some(arg) = strip_keyword(rest, "elif") {
        return Some(Directive::Elif(parse_condition(arg)));
    }
    if strip_keyword(rest, "else").is_some() {
        return Some(Directive::Else);
    }
    if strip_keyword(rest, "endif").is_some() {
        return Some(Directive::Endif);
    }
    if let Some(arg) = strip_keyword(rest, "unset") {
        return Some(Directive::Unset(first_token(arg)));
    }
    if let Some(arg) = strip_keyword(rest, "set") {
        return Some(Directive::Set(first_token(arg)));
    }
    if let Some(arg) = strip_keyword(rest, "define") {
        return Some(Directive::Define {
            name: first_token(arg),
            value: arg.split_once('=').map(|(_, v)| v.trim()),
        });
    }

    None
}

/// Strip a directive keyword and the whitespace after it, returning the
/// remainder. Requires the keyword to be followed by whitespace or
/// end-of-line so `#ifdef` isn't mistaken for `#if`.
fn strip_keyword<'a>(rest: &'a str, keyword: &str) -> Option<&'a str> {
    let after = rest.strip_prefix(keyword)?;
    if after.is_empty() || after.starts_with(char::is_whitespace) {
        Some(after.trim_start())
    } else {
        None
    }
}

/// The first whitespace-delimited token, stopping at `=` too (so
/// `#define NAME = ...` and `#define NAME ...` both yield `NAME`).
fn first_token(s: &str) -> &str {
    s.split(|c: char| c.is_whitespace() || c == '=')
        .find(|tok| !tok.is_empty())
        .unwrap_or("")
}

fn parse_condition(arg: &str) -> Condition<'_> {
    let arg = arg.trim();
    if let Some(symbol) = arg.strip_prefix('!') {
        Condition {
            symbol: symbol.trim(),
            negated: true,
        }
    } else {
        Condition {
            symbol: first_token(arg),
            negated: false,
        }
    }
}

fn condition_holds(cond: &Condition<'_>, symbols: &HashSet<String>) -> bool {
    let defined = symbols.contains(cond.symbol);
    defined != cond.negated
}

fn apply_directive(
    directive: Directive<'_>,
    symbols: &mut HashSet<String>,
    defines: &mut HashMap<String, String>,
    stack: &mut Vec<Frame>,
    currently_active: bool,
    line_number: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match directive {
        Directive::Define { name, value } => {
            if currently_active {
                symbols.insert(name.to_string());
                if let Some(value) = value {
                    // Expand at definition time against earlier defines, so
                    // chained references resolve without cycle risk.
                    let expanded = expand_defines(value, defines).into_owned();
                    defines.insert(name.to_string(), expanded);
                }
            }
        }
        Directive::Set(name) => {
            if currently_active {
                symbols.insert(name.to_string());
            }
        }
        Directive::Unset(name) => {
            if currently_active {
                symbols.remove(name);
            }
        }
        Directive::If(cond) => {
            let holds = condition_holds(&cond, symbols);
            stack.push(Frame {
                branch_active: holds,
                matched: holds,
                parent_active: currently_active,
                opener_line: line_number,
                opener: "#if",
                saw_else: false,
            });
        }
        Directive::Ifdef(name) => {
            let holds = symbols.contains(name);
            stack.push(Frame {
                branch_active: holds,
                matched: holds,
                parent_active: currently_active,
                opener_line: line_number,
                opener: "#ifdef",
                saw_else: false,
            });
        }
        Directive::Ifndef(name) => {
            let holds = !symbols.contains(name);
            stack.push(Frame {
                branch_active: holds,
                matched: holds,
                parent_active: currently_active,
                opener_line: line_number,
                opener: "#ifndef",
                saw_else: false,
            });
        }
        Directive::Elif(cond) => {
            if let Some(frame) = stack.last_mut() {
                if frame.saw_else {
                    frame.branch_active = false;
                    diagnostics.push(preprocessor_diagnostic(format!(
                        "line {line_number}: #elif cannot appear after #else"
                    )));
                } else if frame.matched {
                    frame.branch_active = false;
                } else {
                    let holds = condition_holds(&cond, symbols);
                    frame.branch_active = holds;
                    frame.matched = holds;
                }
            } else {
                diagnostics.push(preprocessor_diagnostic(format!(
                    "line {line_number}: unmatched #elif"
                )));
            }
        }
        Directive::Else => {
            if let Some(frame) = stack.last_mut() {
                if frame.saw_else {
                    frame.branch_active = false;
                    diagnostics.push(preprocessor_diagnostic(format!(
                        "line {line_number}: duplicate #else"
                    )));
                } else {
                    frame.branch_active = !frame.matched;
                    frame.matched = true;
                    frame.saw_else = true;
                }
            } else {
                diagnostics.push(preprocessor_diagnostic(format!(
                    "line {line_number}: unmatched #else"
                )));
            }
        }
        Directive::Endif => {
            if stack.pop().is_none() {
                diagnostics.push(preprocessor_diagnostic(format!(
                    "line {line_number}: unmatched #endif"
                )));
            }
        }
    }
}

/// Replace each `$name` whose `name` matches a known `#define` with that
/// define's value text. Unknown references (and bare `$`) pass through
/// unchanged, so e.g. `"$100"` in a help string is never mangled.
fn expand_defines<'a>(line: &'a str, defines: &HashMap<String, String>) -> Cow<'a, str> {
    if defines.is_empty() || !line.contains('$') {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut changed = false;
    let mut rest = line;
    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let end = after
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(after.len());
        let name = &after[..end];
        match defines.get(name) {
            Some(value) => {
                out.push_str(value);
                changed = true;
            }
            None => {
                out.push('$');
                out.push_str(name);
            }
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(line)
    }
}

fn preprocessor_diagnostic(detail: String) -> Diagnostic {
    Diagnostic {
        section: "Preprocessor".to_string(),
        detail,
    }
}
