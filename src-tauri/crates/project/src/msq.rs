// SPDX-License-Identifier: GPL-3.0-or-later
//! `.msq` (TunerStudio tune XML) read/write against a `Definition` + `Tune`.
//!
//! ponytail: round-trips `<constant name→value>` pairs + `<versionInfo signature>`
//! with TunerStudio-compatible quoting and array rows. Settings groups, comments,
//! CRC, bibliography metadata, and PC variables are intentionally skipped.

use opentune_ini::ConstantKind;
use opentune_model::{Tune, Value};
use quick_xml::events::Event;
use quick_xml::Reader;

#[derive(Debug, thiserror::Error)]
pub enum MsqError {
    #[error("malformed .msq XML: {0}")]
    Xml(String),
    #[error("tune signature mismatch: file is for {found:?}, definition is {expected:?}")]
    SignatureMismatch { expected: String, found: String },
}

#[derive(Debug, Default)]
pub struct MsqReport {
    pub applied: usize,
    /// Constants in the file that the definition doesn't declare.
    pub skipped: Vec<String>,
    /// Constants that parsed to a value the model rejected (unknown bit
    /// label, unparseable number, storage overflow). `(name, reason)`.
    pub failed: Vec<(String, String)>,
    /// Constants whose file value fell outside the INI's `[low, high]` and
    /// was clamped into range (TunerStudio's load behavior). Counted in
    /// `applied` too.
    pub clamped: Vec<String>,
}

/// Serialize the whole tune to `.msq` XML.
pub fn tune_to_msq(tune: &Tune) -> String {
    let def = tune.definition();
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<msq xmlns=\"http://www.msefi.com/:msq\">\n");
    out.push_str(&format!(
        "  <versionInfo fileFormat=\"5.0\" signature=\"{}\"/>\n",
        xml_escape(&def.comms.signature)
    ));
    for page in &def.pages {
        out.push_str("  <page>\n");
        for c in def.constants.iter().filter(|c| c.page == page.number) {
            let value = match tune.get(&c.name) {
                Ok(v) => v,
                Err(_) => continue, // ponytail: constant unreadable → omit, not fatal on save
            };
            match (&value, &c.kind) {
                (Value::Array(values), ConstantKind::Array { shape, .. }) => {
                    let units = if c.units.is_empty() {
                        String::new()
                    } else {
                        format!(" units=\"{}\"", xml_escape(&c.units))
                    };
                    out.push_str(&format!(
                        "    <constant name=\"{}\" cols=\"{}\" rows=\"{}\" digits=\"{}\"{}>\n",
                        xml_escape(&c.name),
                        shape.cols,
                        shape.rows,
                        c.digits,
                        units
                    ));
                    for row in values.chunks(shape.cols.max(1)) {
                        let row = row
                            .iter()
                            .map(|n| fmt_num(*n))
                            .collect::<Vec<_>>()
                            .join(" ");
                        out.push_str(&format!("      {row}\n"));
                    }
                    out.push_str("    </constant>\n");
                }
                _ => {
                    let value_text = value_to_text(&value, &c.kind);
                    out.push_str(&format!(
                        "    <constant name=\"{}\">{}</constant>\n",
                        xml_escape(&c.name),
                        xml_escape(&value_text)
                    ));
                }
            }
        }
        out.push_str("  </page>\n");
    }
    out.push_str("</msq>\n");
    out
}

/// Parse `.msq`, validate the signature against `tune`'s definition, and apply
/// every constant by name. Unknown constants are collected in `skipped`.
pub fn load_msq_into(tune: &mut Tune, xml: &str) -> Result<MsqReport, MsqError> {
    let expected = tune.definition().comms.signature.clone();
    let parsed = parse_constants(xml)?;
    // Fail closed: the signature guard is the one mandatory hard error on
    // load. A file with NO `<versionInfo signature>` is rejected too — we
    // never apply an unsigned/unknown-provenance tune unchecked.
    match parsed.signature.as_deref() {
        Some(found) if found == expected => {}
        Some(found) => {
            return Err(MsqError::SignatureMismatch {
                expected,
                found: found.to_string(),
            })
        }
        None => {
            return Err(MsqError::SignatureMismatch {
                expected,
                found: "(no versionInfo signature)".to_string(),
            })
        }
    }
    let mut report = MsqReport::default();
    for (name, text) in parsed.constants {
        // Resolve the value in a scope that ends the immutable `definition()`
        // borrow before the mutable `tune.set` below (no clone of the kind).
        let resolved = tune
            .definition()
            .constant(&name)
            .map(|c| text_to_value(&text, &c.kind));
        match resolved {
            None => report.skipped.push(name),
            Some(Ok(value)) => {
                let (value, was_clamped) = clamp_to_bounds(tune, &name, value);
                match tune.set(&name, value) {
                    Ok(()) => {
                        report.applied += 1;
                        if was_clamped {
                            report.clamped.push(name);
                        }
                    }
                    // `ModelError` has no `Display` impl (only `Debug`) — see
                    // the model crate's `tune::ModelError`. Debug-format the
                    // reason rather than pulling `thiserror`/`Display` into
                    // `model` just for this caller.
                    Err(e) => report.failed.push((name, format!("{e:?}"))),
                }
            }
            // Per-constant failure is collected, never fatal — the rest of the
            // file still applies and the tune stays fully defined.
            Some(Err(detail)) => report.failed.push((name, detail)),
        }
    }
    Ok(report)
}

struct Parsed {
    signature: Option<String>,
    constants: Vec<(String, String)>,
}

fn parse_constants(xml: &str) -> Result<Parsed, MsqError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut signature = None;
    let mut constants = Vec::new();
    let mut current_name: Option<String> = None;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(MsqError::Xml(e.to_string())),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let tag = e.name();
                let tag = String::from_utf8_lossy(tag.as_ref()).to_string();
                if tag == "versionInfo" {
                    signature = attr(&e, "signature");
                } else if tag == "constant" {
                    current_name = attr(&e, "name");
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(name) = current_name.take() {
                    let text = t.unescape().map_err(|e| MsqError::Xml(e.to_string()))?;
                    constants.push((name, text.to_string()));
                }
            }
            Ok(Event::End(_)) => current_name = None,
            _ => {}
        }
        buf.clear();
    }
    Ok(Parsed {
        signature,
        constants,
    })
}

fn attr(e: &quick_xml::events::BytesStart, key: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        (a.key.as_ref() == key.as_bytes()).then(|| String::from_utf8_lossy(&a.value).to_string())
    })
}

fn value_to_text(v: &Value, kind: &ConstantKind) -> String {
    match (v, kind) {
        (Value::Enum(idx), ConstantKind::Bits { options, .. }) => options
            .get(*idx as usize)
            .map(|label| format!("\"{label}\""))
            .unwrap_or_else(|| idx.to_string()),
        (Value::Enum(idx), _) => idx.to_string(),
        (Value::Scalar(n), _) => fmt_num(*n),
        (Value::Array(xs), _) => xs.iter().map(|n| fmt_num(*n)).collect::<Vec<_>>().join(" "),
        (Value::Text(s), _) => format!("\"{s}\""),
    }
}

/// TunerStudio clamps out-of-range numeric values into the constant's
/// `[low, high]` on load rather than rejecting the file (rusEFI files
/// routinely store 0 for constants whose INI minimum is 1). Mirror that on
/// the load path only — interactive edits still validate strictly in
/// `Tune::set`. Unresolvable or inverted bounds fall through to `set`,
/// which reports its own error.
fn clamp_to_bounds(tune: &Tune, name: &str, value: Value) -> (Value, bool) {
    let Ok((low, high)) = tune.bounds(name) else {
        return (value, false);
    };
    if low > high {
        return (value, false);
    }
    let out_of_range = |x: &f64| *x < low || *x > high;
    match value {
        Value::Scalar(x) if out_of_range(&x) => (Value::Scalar(x.clamp(low, high)), true),
        Value::Array(xs) if xs.iter().any(out_of_range) => (
            Value::Array(xs.iter().map(|x| x.clamp(low, high)).collect()),
            true,
        ),
        v => (v, false),
    }
}

/// TunerStudio wraps enum labels and string values in double quotes on save;
/// our own writer doesn't. Both forms must load.
fn unquote(text: &str) -> &str {
    text.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(text)
}

fn text_to_value(text: &str, kind: &ConstantKind) -> Result<Value, String> {
    let text = text.trim();
    match kind {
        ConstantKind::Bits { options, .. } => {
            let text = unquote(text);
            // ponytail: label→index; fall back to a numeric index if the file
            // stored one. Corruption risk lives here — covered by a unit test.
            if let Some(idx) = options.iter().position(|o| o == text) {
                Ok(Value::Enum(idx as u32))
            } else if let Ok(idx) = text.parse::<u32>() {
                Ok(Value::Enum(idx))
            } else {
                Err(format!("unknown option {text:?}"))
            }
        }
        ConstantKind::Array { .. } => {
            let nums = text
                .split_whitespace()
                .map(|s| s.parse::<f64>())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            Ok(Value::Array(nums))
        }
        ConstantKind::Text { .. } => Ok(Value::Text(unquote(text).to_string())),
        ConstantKind::Scalar(_) => text
            .parse::<f64>()
            .map(Value::Scalar)
            .map_err(|e| e.to_string()),
    }
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
