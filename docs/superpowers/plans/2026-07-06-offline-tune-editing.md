# Offline Tune Editing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user pick an ECU's INI, start or open a saved tune, edit it fully offline, save it back to `.msq`, and push it to the ECU once connected.

**Architecture:** Make the *connection* the optional part of a `Session` (`conn: Option<ActiveConnection>`) so the tune is always present and the existing editors work with no live link. Add `.msq` read/write in the `project` crate, offline lifecycle commands modeled on `layout.rs`, an attach-on-connect path that never overwrites offline edits, and two signature guards.

**Tech Stack:** Rust (Tauri v2, Cargo workspace crates `ini`/`model`/`project`/`protocol`), `quick-xml` for `.msq`, React 19 + Zustand + TypeScript, `tauri-plugin-dialog`, `tauri-specta` for generated IPC bindings.

## Global Constraints

- **Branch/worktree:** all work on branch `offline-tune` in the worktree `/Users/dopawlus/Projects/private/TuningSoftware/.worktrees/offline-tune`. The parallel M4 work stays on `m4-table-editors` — do not touch that branch.
- **Rust gates every task:** `cargo fmt --all` clean, `cargo clippy --all-targets -- -D warnings` clean, `cargo test` green before each commit. Line width 100 (rustfmt default). No `unwrap()`/`expect()` outside tests.
- **Library errors** use `thiserror` typed enums; **app/command errors** surface as `Result<_, String>` the way existing `#[tauri::command]`s already do (mirror the current pattern exactly).
- **TDD:** write the failing test first, watch it fail, implement minimally, watch it pass, commit. One deliverable per task.
- **Commit style:** conventional (`feat:`/`fix:`/`test:`/`docs:`/`chore:`), imperative, no attribution footer.
- **`.msq` scope:** round-trip `<constant name→value>` pairs + `<versionInfo signature>` only. Skip settings-groups / comments / CRC / bibliography (ponytail-comment the ceiling). Full `.msq` fidelity stays M6.
- **Push target = simulator.** Reuse the existing `write_deltas`/`burn` path; do NOT implement real serial write (`SERIAL_UNSUPPORTED` stays as-is).
- **Two signature guards are mandatory:** `.msq`↔INI on load; tune↔ECU on attach/push (mismatch refuses; no override in MVP).
- **Regenerate IPC bindings** (`src/ipc/bindings.ts`) whenever a `#[tauri::command]` is added/changed; never hand-edit that file.

## File Structure

**New files:**
- `src-tauri/crates/project/src/msq.rs` — `.msq` XML read/write against a `Definition` + `Tune`. One responsibility: format translation.
- `src-tauri/crates/project/src/lib.rs` — re-export the `msq` module (currently a 2-line placeholder).
- `src-tauri/crates/project/tests/msq.rs` — `.msq` round-trip tests (integration binary, mirrors `model/tests/common` pattern).
- `src-tauri/crates/project/tests/common/mod.rs` — copy of the model test Definition builders (`comms`/`scalar`/`array_on`/`bits_on`/`tune`).
- `src-tauri/src/offline_commands.rs` — `new_tune`/`open_tune`/`save_tune`/`write_tune_to_ecu` Tauri commands.
- `src/components/offline/OfflinePanel.tsx` — offline entry UI (pick INI, New blank / Open `.msq` / Save / Write to ECU).
- `src/components/offline/offline.css` — panel styling (tokens from `styles/tokens.css`).

**Modified files:**
- `src-tauri/crates/project/Cargo.toml` — add `quick-xml`, `opentune-model`, `opentune-ini` deps.
- `src-tauri/src/connection.rs` — `Session.conn: Option<ActiveConnection>`.
- `src-tauri/src/session.rs` — gate live ops on `conn`; `attach_conn`; `write_all_pages`.
- `src-tauri/src/owner.rs` / `owner_ops.rs` — new `Command` variants + handlers; `connect` ATTACH branch.
- `src-tauri/src/lib.rs` — register commands; `.plugin(tauri_plugin_dialog::init())`.
- `src-tauri/Cargo.toml` — `tauri-plugin-dialog`; ensure `project` crate is a workspace dep of the app.
- `package.json` — `@tauri-apps/plugin-dialog`.
- `src/stores/tune.ts` — `hasTune` bit decoupled from `linkAlive`.
- `src/components/dialogs/TunePanel.tsx` — reset store only when no conn AND no tune.
- `src/ipc/bindings.ts` — regenerated (not hand-edited).

---

## Task 1: `.msq` read/write in the `project` crate

The most self-contained piece — pure Rust, no Tauri, no connection. Translates
between a `.msq` XML document and a `Tune` against its `Definition`. This is the
foundation Tasks 3–4 build on.

**Files:**
- Create: `src-tauri/crates/project/src/msq.rs`
- Modify: `src-tauri/crates/project/src/lib.rs`
- Modify: `src-tauri/crates/project/Cargo.toml`
- Test: `src-tauri/crates/project/tests/msq.rs`
- Test: `src-tauri/crates/project/tests/common/mod.rs`

**Interfaces:**
- Consumes: `opentune_model::{Tune, Value}`, `opentune_ini::{Definition, ConstantKind}`, `Tune::get(&str) -> Result<Value, _>`, `Tune::set(&str, Value) -> Result<(), _>`, `Tune::page_bytes`, `def.comms.signature: String`, `def.constant(name)`.
- Produces:
  - `pub fn tune_to_msq(tune: &Tune) -> String` — serialize the whole tune to `.msq` XML.
  - `pub fn load_msq_into(tune: &mut Tune, xml: &str) -> Result<MsqReport, MsqError>` — parse `.msq`, validate signature vs the tune's definition, apply every constant by name.
  - `pub struct MsqReport { pub applied: usize, pub skipped: Vec<String>, pub failed: Vec<(String, String)> }` — a load never aborts on a per-constant problem: unknown constants go to `skipped`, value/range/label failures to `failed` (name, reason). The tune is always left fully defined.
  - `pub enum MsqError { Xml(String), SignatureMismatch { expected: String, found: String } }` (derive `Debug`, `thiserror::Error`). The **only** hard error is the whole-file signature guard.

- [ ] **Step 1: Add crate dependencies**

Edit `src-tauri/crates/project/Cargo.toml` `[dependencies]` (mirror sibling crates' version pins):

```toml
[dependencies]
opentune-ini = { path = "../ini" }
opentune-model = { path = "../model" }
quick-xml = "0.36"
thiserror = "1"

[dev-dependencies]
opentune-ini = { path = "../ini" }
opentune-model = { path = "../model" }
```

- [ ] **Step 2: Create the shared test Definition builders**

Create `src-tauri/crates/project/tests/common/mod.rs` — a trimmed copy of the model crate's helper (`src-tauri/crates/model/tests/common/mod.rs`), adding `array_on` and `bits_on` builders:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared Definition/Tune builders for the `project` crate's `.msq` tests.
#![allow(dead_code)]

use std::sync::Arc;

use opentune_ini::{
    ArrayShape, CommsSettings, ConstantDef, ConstantKind, Definition, Endianness, EnvelopeFormat,
    FrontPageDef, Number, PageDef, ScalarType,
};
use opentune_model::Tune;

pub const PAGE_SIZE: usize = 64;
pub const SIGNATURE: &str = "speeduino 202504-dev";

pub fn comms() -> CommsSettings {
    CommsSettings {
        signature: SIGNATURE.to_string(),
        query_command: "Q".to_string(),
        version_info: "S".to_string(),
        och_get_command: "r".to_string(),
        page_read_command: "p%2i%2o%2c".to_string(),
        page_value_write: "M%2i%2o%2c%v".to_string(),
        burn_command: "b%2i".to_string(),
        blocking_factor: 251,
        page_activation_delay_ms: 10,
        block_read_timeout_ms: 2_000,
        inter_write_delay_ms: 10,
        endianness: Endianness::Little,
        envelope: EnvelopeFormat::MsEnvelope10,
        och_block_size: 0,
    }
}

pub fn scalar(name: &str, ty: ScalarType, offset: usize, scale: f64, high: f64) -> ConstantDef {
    ConstantDef {
        name: name.to_string(),
        page: 1,
        offset,
        kind: ConstantKind::Scalar(ty),
        scale: Number::Lit(scale),
        translate: Number::Lit(0.0),
        units: String::new(),
        low: Number::Lit(0.0),
        high: Number::Lit(high),
        digits: 0,
    }
}

pub fn array_on(name: &str, offset: usize, rows: usize, cols: usize) -> ConstantDef {
    ConstantDef {
        name: name.to_string(),
        page: 1,
        offset,
        kind: ConstantKind::Array {
            elem: ScalarType::U08,
            shape: ArrayShape { rows, cols },
        },
        scale: Number::Lit(1.0),
        translate: Number::Lit(0.0),
        units: String::new(),
        low: Number::Lit(0.0),
        high: Number::Lit(255.0),
        digits: 0,
    }
}

pub fn bits_on(name: &str, offset: usize, options: &[&str]) -> ConstantDef {
    ConstantDef {
        name: name.to_string(),
        page: 1,
        offset,
        kind: ConstantKind::Bits {
            storage: ScalarType::U08,
            bit_lo: 0,
            bit_hi: 1,
            options: options.iter().map(|s| s.to_string()).collect(),
        },
        scale: Number::Lit(1.0),
        translate: Number::Lit(0.0),
        units: String::new(),
        low: Number::Lit(0.0),
        high: Number::Lit(3.0),
        digits: 0,
    }
}

pub fn tune(constants: Vec<ConstantDef>) -> Tune {
    let pages = vec![PageDef { number: 1, size: PAGE_SIZE }];
    Tune::new(Arc::new(Definition {
        comms: comms(),
        pages,
        constants,
        pc_variables: vec![],
        menus: vec![],
        dialogs: vec![],
        tables: vec![],
        curves: vec![],
        diagnostics: vec![],
        output_channels: Vec::new(),
        gauges: Vec::new(),
        frontpage: FrontPageDef { gauge_slots: Vec::new(), indicators: Vec::new() },
        ve_analyze: None,
    }))
}
```

> NOTE for the implementer: confirm the exact field set of `Definition`, `CommsSettings`, and `ConstantKind::Array{shape}` against `src-tauri/crates/model/tests/common/mod.rs` and `src-tauri/crates/ini/src/{constants.rs,lib.rs,definition.rs}` before running — copy the model crate's helper verbatim and only add `array_on`/`bits_on`. Field names must match the current structs (this plan mirrors them as of commit `344b2ea`).

- [ ] **Step 3: Write the failing round-trip + bit-field tests**

Create `src-tauri/crates/project/tests/msq.rs`:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
mod common;

use common::{array_on, bits_on, scalar, tune, SIGNATURE};
use opentune_ini::ScalarType;
use opentune_model::Value;
use opentune_project::msq::{load_msq_into, tune_to_msq, MsqError};

#[test]
fn scalar_array_bits_text_round_trip() {
    let mut t = tune(vec![
        scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0),
        array_on("veTable", 4, 2, 2),
        bits_on("algorithm", 8, &["Speed Density", "Alpha-N", "MAP"]),
    ]);
    t.set("crankingRPM", Value::Scalar(40.0)).unwrap();
    t.set("veTable", Value::Array(vec![10.0, 20.0, 30.0, 40.0])).unwrap();
    t.set("algorithm", Value::Enum(1)).unwrap(); // "Alpha-N"

    let xml = tune_to_msq(&t);
    assert!(xml.contains(&format!("signature=\"{SIGNATURE}\"")));

    // Re-load into a fresh zeroed tune with the same definition.
    let mut fresh = tune(vec![
        scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0),
        array_on("veTable", 4, 2, 2),
        bits_on("algorithm", 8, &["Speed Density", "Alpha-N", "MAP"]),
    ]);
    let report = load_msq_into(&mut fresh, &xml).unwrap();
    assert_eq!(report.applied, 3);
    assert!(report.skipped.is_empty());
    assert!(report.failed.is_empty());
    assert_eq!(fresh.get("crankingRPM").unwrap(), Value::Scalar(40.0));
    assert_eq!(fresh.get("veTable").unwrap(), Value::Array(vec![10.0, 20.0, 30.0, 40.0]));
    assert_eq!(fresh.get("algorithm").unwrap(), Value::Enum(1));
}

#[test]
fn bad_bit_label_is_collected_not_fatal() {
    // A real .msq may carry a bit-field label the INI's parsed options don't
    // match exactly. That one constant must fail into the report, not abort
    // the whole load — the good constants still apply.
    let good = tune(vec![
        scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0),
        bits_on("algorithm", 8, &["Speed Density", "Alpha-N"]),
    ]);
    // Serialize a valid tune, then corrupt only the bit-field's label text.
    let xml = {
        let mut t = good;
        t.set("crankingRPM", Value::Scalar(40.0)).unwrap();
        t.set("algorithm", Value::Enum(1)).unwrap();
        tune_to_msq(&t).replace(">Alpha-N<", ">Nope-Not-An-Option<")
    };
    let mut fresh = tune(vec![
        scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0),
        bits_on("algorithm", 8, &["Speed Density", "Alpha-N"]),
    ]);
    let report = load_msq_into(&mut fresh, &xml).unwrap();
    assert_eq!(report.applied, 1); // crankingRPM applied
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].0, "algorithm");
    assert_eq!(fresh.get("crankingRPM").unwrap(), Value::Scalar(40.0));
}

#[test]
fn bit_field_serializes_as_label_not_index() {
    let mut t = tune(vec![bits_on("algorithm", 0, &["Speed Density", "Alpha-N"])]);
    t.set("algorithm", Value::Enum(1)).unwrap();
    let xml = tune_to_msq(&t);
    assert!(xml.contains(">Alpha-N<"), "bit field must serialize the option label, got: {xml}");
    assert!(!xml.contains(">1<"), "must not serialize the raw index");
}

#[test]
fn signature_mismatch_is_rejected() {
    let t = tune(vec![scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0)]);
    let bad = tune_to_msq(&t).replace(SIGNATURE, "rusEFI 2024");
    let mut fresh = tune(vec![scalar("crankingRPM", ScalarType::U08, 0, 1.0, 255.0)]);
    let err = load_msq_into(&mut fresh, &bad).unwrap_err();
    assert!(matches!(err, MsqError::SignatureMismatch { .. }));
}
```

- [ ] **Step 4: Run the tests — verify they fail to compile (module missing)**

Run: `cargo test -p opentune-project --test msq`
Expected: FAIL — `unresolved import opentune_project::msq`.

- [ ] **Step 5: Implement `msq.rs`**

Create `src-tauri/crates/project/src/msq.rs`. Read constants by name from the tune, dispatch on `ConstantKind`, and emit/parse `.msq`. Use `quick-xml`'s reader for parsing and a hand-written writer for output (the document is tiny and flat).

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
//! `.msq` (TunerStudio tune XML) read/write against a `Definition` + `Tune`.
//!
//! ponytail: round-trips `<constant name→value>` pairs + `<versionInfo signature>`
//! only. Settings groups, comments, CRC, and bibliography metadata are skipped —
//! full `.msq` fidelity is the M6 `project` goal.

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
    /// Constants that parsed to a value the model rejected (out of range,
    /// unknown bit label, unparseable number). `(name, reason)`.
    pub failed: Vec<(String, String)>,
}

/// Serialize the whole tune to `.msq` XML.
pub fn tune_to_msq(tune: &Tune) -> String {
    let def = tune.definition();
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?>\n");
    out.push_str("<msq xmlns=\"http://www.msefi.com/:msq\">\n");
    out.push_str(&format!(
        "  <versionInfo fileFormat=\"5.0\" signature=\"{}\"/>\n",
        xml_escape(&def.comms.signature)
    ));
    for page in &def.pages {
        out.push_str("  <page>\n");
        for c in def.constants.iter().filter(|c| c.page == page.number) {
            let value_text = match tune.get(&c.name) {
                Ok(v) => value_to_text(&v, &c.kind),
                Err(_) => continue, // ponytail: constant unreadable → omit, not fatal on save
            };
            out.push_str(&format!(
                "    <constant name=\"{}\">{}</constant>\n",
                xml_escape(&c.name),
                xml_escape(&value_text)
            ));
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
    if let Some(found) = &parsed.signature {
        if found != &expected {
            return Err(MsqError::SignatureMismatch {
                expected,
                found: found.clone(),
            });
        }
    }
    let mut report = MsqReport::default();
    for (name, text) in parsed.constants {
        // Resolve the value in a scope that ends the immutable `definition()`
        // borrow before the mutable `tune.set` below (no clone of the kind).
        let resolved = match tune.definition().constant(&name) {
            Some(c) => Some(text_to_value(&text, &c.kind)),
            None => None,
        };
        match resolved {
            None => report.skipped.push(name),
            Some(Ok(value)) => match tune.set(&name, value) {
                Ok(()) => report.applied += 1,
                Err(e) => report.failed.push((name, e.to_string())),
            },
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
    Ok(Parsed { signature, constants })
}

fn attr(e: &quick_xml::events::BytesStart, key: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        (a.key.as_ref() == key.as_bytes())
            .then(|| String::from_utf8_lossy(&a.value).to_string())
    })
}

fn value_to_text(v: &Value, kind: &ConstantKind) -> String {
    match (v, kind) {
        (Value::Enum(idx), ConstantKind::Bits { options, .. }) => options
            .get(*idx as usize)
            .cloned()
            .unwrap_or_else(|| idx.to_string()),
        (Value::Enum(idx), _) => idx.to_string(),
        (Value::Scalar(n), _) => fmt_num(*n),
        (Value::Array(xs), _) => xs.iter().map(|n| fmt_num(*n)).collect::<Vec<_>>().join(" "),
        (Value::Text(s), _) => s.clone(),
    }
}

fn text_to_value(text: &str, kind: &ConstantKind) -> Result<Value, String> {
    let text = text.trim();
    match kind {
        ConstantKind::Bits { options, .. } => {
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
        ConstantKind::Text { .. } => Ok(Value::Text(text.to_string())),
        ConstantKind::Scalar(_) => text.parse::<f64>().map(Value::Scalar).map_err(|e| e.to_string()),
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
```

> IMPLEMENTER NOTES:
> - `tune_to_msq`/`load_msq_into` call `tune.definition()` — a public accessor for `Arc<Definition>`. `Tune` currently exposes `def` as `pub(crate)`. **Add** `pub fn definition(&self) -> &Definition { &self.def }` to `src-tauri/crates/model/src/tune.rs` (one-line accessor; commit it with this task). Confirm the exact `Value` enum variant names in `src-tauri/crates/model/src/value.rs` and adjust the match arms if they differ.
> - `quick-xml` 0.36 uses `reader.config_mut().trim_text(true)`. If the pinned version differs, use that version's trim API. Verify the version resolves in the workspace before implementing.

- [ ] **Step 6: Wire the module in `lib.rs`**

Replace `src-tauri/crates/project/src/lib.rs` (currently a placeholder) with:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
//! Project-file persistence. Currently: `.msq` tune read/write (offline tuning).
//! Full project bundles (INI + tune + dashboard + settings) remain M6.

pub mod msq;
```

- [ ] **Step 7: Run the tests — verify they pass**

Run: `cargo test -p opentune-project --test msq`
Expected: PASS (3 tests). Then `cargo clippy -p opentune-project --all-targets -- -D warnings` and `cargo fmt --all`.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/crates/project src-tauri/crates/model/src/tune.rs
git commit -m "feat(project): .msq tune read/write with signature guard"
```

---

## Task 2: Make `Session.conn` optional and decouple editing from the wire

Today all four mutators (`set_value`/`set_cells`/`undo`/`redo`) write to the wire
via `write_deltas`, and `load_tune`/`burn` go through `protocol_for` (which errors
on serial). This task makes `conn` an `Option`: editing commits to the model and
only writes the wire **when a connection is present**; the ECU-only ops error when
it isn't.

**Files:**
- Modify: `src-tauri/src/connection.rs:49-60` (Session struct)
- Modify: `src-tauri/src/session.rs` (mutators + `load_tune`/`burn`, add `NO_CONNECTION`)
- Modify: `src-tauri/src/owner_ops.rs:44-50` (`build_session` wraps `conn`)
- Modify: `src-tauri/src/owner.rs:326-334` (`DebugSimulator` match arm)
- Test: `src-tauri/src/session.rs` `#[cfg(test)]` module (offline edit works; burn errors)

**Interfaces:**
- Consumes: `ActiveConnection`, `Tune::{set,get,page_bytes}`, `page_deltas`, `write_deltas`, `protocol_for`.
- Produces: `Session { conn: Option<ActiveConnection>, def, tune, snapshot }`; `pub(crate) const NO_CONNECTION`; mutators that no-op the wire when `conn` is `None`.

- [ ] **Step 1: Write the failing offline-editing test**

Add to the `#[cfg(test)] mod tests` in `src-tauri/src/session.rs` (create the module if absent). This builds an offline `Session` with a hand-made one-scalar definition so the value/range is controlled:

```rust
#[cfg(test)]
mod offline_tests {
    use super::*;
    use opentune_ini::{
        CommsSettings, ConstantDef, ConstantKind, Definition, Endianness, EnvelopeFormat,
        FrontPageDef, Number, PageDef, ScalarType,
    };
    use opentune_model::{Tune, Value};
    use std::sync::Arc;

    fn offline_session() -> Session {
        let comms = CommsSettings {
            signature: "test-sig".into(),
            query_command: "Q".into(),
            version_info: "S".into(),
            och_get_command: "r".into(),
            page_read_command: "p".into(),
            page_value_write: "M".into(),
            burn_command: "b".into(),
            blocking_factor: 251,
            page_activation_delay_ms: 0,
            block_read_timeout_ms: 1000,
            inter_write_delay_ms: 0,
            endianness: Endianness::Little,
            envelope: EnvelopeFormat::MsEnvelope10,
            och_block_size: 0,
        };
        let def = Arc::new(Definition {
            comms,
            pages: vec![PageDef { number: 1, size: 8 }],
            constants: vec![ConstantDef {
                name: "rpm".into(),
                page: 1,
                offset: 0,
                kind: ConstantKind::Scalar(ScalarType::U08),
                scale: Number::Lit(1.0),
                translate: Number::Lit(0.0),
                units: String::new(),
                low: Number::Lit(0.0),
                high: Number::Lit(255.0),
                digits: 0,
            }],
            pc_variables: vec![],
            menus: vec![],
            dialogs: vec![],
            tables: vec![],
            curves: vec![],
            diagnostics: vec![],
            output_channels: vec![],
            gauges: vec![],
            frontpage: FrontPageDef { gauge_slots: vec![], indicators: vec![] },
            ve_analyze: None,
        });
        let tune = Tune::new(Arc::clone(&def));
        Session { conn: None, def, tune: Some(tune), snapshot: None }
    }

    #[test]
    fn set_value_commits_offline_without_a_wire() {
        let mut s = offline_session();
        s.set_value("rpm", Value::Scalar(42.0)).unwrap();
        assert_eq!(s.tune.as_ref().unwrap().get("rpm").unwrap(), Value::Scalar(42.0));
    }

    #[test]
    fn burn_offline_reports_no_connection() {
        let mut s = offline_session();
        s.set_value("rpm", Value::Scalar(42.0)).unwrap();
        let err = s.burn().unwrap_err();
        assert_eq!(err, NO_CONNECTION);
    }
}
```

> NOTE: confirm the `Definition`/`CommsSettings` field set against `src-tauri/crates/model/tests/common/mod.rs` (it is the authoritative literal) and adjust if a field name differs at implementation time.

- [ ] **Step 2: Run the test — verify it fails to compile**

Run: `cargo test --manifest-path src-tauri/Cargo.toml offline_tests`
Expected: FAIL — `Session` field `conn` expects `ActiveConnection`, not `Option`; `NO_CONNECTION` undefined.

- [ ] **Step 3: Make `conn` optional**

In `src-tauri/src/connection.rs`, change the `Session` field (line 51):

```rust
    /// The live connection, if any. `None` = an offline session (a tune loaded
    /// from a file or created blank) that has no ECU link yet.
    pub conn: Option<ActiveConnection>,
```

- [ ] **Step 4: Add `NO_CONNECTION` and gate the wire in `session.rs`**

Add the constant next to `NO_TUNE` (near line 32):

```rust
pub(crate) const NO_CONNECTION: &str = "no ECU connection — this operation needs a live link";
```

In `set_value`, replace the unconditional write (the two lines computing `deltas` + `write_deltas`) with a guarded block:

```rust
        // Wire the change only when connected; offline sessions edit the model
        // in place (the RAM-vs-flash distinction collapses to model-only).
        if let Some(conn) = conn.as_ref() {
            let deltas = page_deltas(tune, &probe, &def.pages);
            write_deltas(conn, &def.comms, &deltas)?;
        }
```

Apply the same guard in `set_cells` (wrap its `deltas`/`write_deltas` pair identically).

In `undo`, replace the `write_deltas` block (lines 183-187) with:

```rust
        if let Some(conn) = conn.as_ref() {
            let deltas = page_deltas(&before, tune, &def.pages);
            if let Err(e) = write_deltas(conn, &def.comms, &deltas) {
                tune.redo(); // reverse the undo so tune matches the ECU
                return Err(e);
            }
        }
```

Apply the mirror in `redo` (lines 202-206), calling `tune.undo()` on the write error.

In `load_tune` and `burn`, require a connection. For `load_tune`, after destructuring, add:

```rust
        let conn = conn.as_ref().ok_or_else(|| NO_CONNECTION.to_string())?;
```

(then `protocol_for(conn, &def.comms)` takes the unwrapped `&ActiveConnection`). Do the same in `burn` (add the `conn` unwrap before `protocol_for`).

In `poll_frame` (the realtime path), guard the `protocol_for(conn, ...)` call the same way but **fail open** (offline yields no frame): `let Some(conn) = conn.as_ref() else { return Ok(None) };` — locate the `conn` use in `poll_frame` and add this early return.

- [ ] **Step 5: Fix the two construction/match sites**

`src-tauri/src/owner_ops.rs` `build_session` return (line ~45):

```rust
    Ok(Session {
        conn: Some(conn),
        def,
        tune: None,
        snapshot: None,
    })
```

`src-tauri/src/owner.rs` `DebugSimulator` arm (line 328):

```rust
                    Some(Session {
                        conn: Some(ActiveConnection::Sim { simulator, .. }),
                        ..
                    }) => Ok(Arc::clone(simulator)),
```

Then grep for any other site that builds or matches a `Session`/`conn` and wrap in `Some(...)`:

Run: `grep -rn "conn: ActiveConnection\|conn:.*ActiveConnection::\|Session {" src-tauri/src src-tauri/tests`
Fix each existing site (including any `session.rs` unit tests **and** `src-tauri/tests/*` integration tests that build a sim `Session`) to use `conn: Some(...)`. (The full `cargo test` in Step 6 compiles the integration tests, so a missed site surfaces there — but grep both trees now so it isn't a surprise.)

- [ ] **Step 6: Run the tests — verify pass, plus the whole suite**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS (new offline tests green; existing tests still green). Then `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` and `cargo fmt --all`.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/connection.rs src-tauri/src/session.rs src-tauri/src/owner.rs src-tauri/src/owner_ops.rs
git commit -m "feat(app): optional Session.conn — edit a tune with no live link"
```

---

## Task 3: Offline lifecycle commands (`new_tune`, `open_tune`, `save_tune`)

Adds owner commands that build an offline session (`conn: None`) from an INI, optionally
loading a `.msq`, and save the current tune back to `.msq`. Wires the `project` crate
into the app.

**Files:**
- Modify: `src-tauri/Cargo.toml` (`[dependencies]` add `opentune-project`)
- Modify: `src-tauri/src/owner.rs` (Command variants + serve arms + `reset_session`/`new_tune`/`open_tune`)
- Modify: `src-tauri/src/owner_ops.rs` (`build_offline_session`, `build_offline_session_from_msq`)
- Create: `src-tauri/src/offline_commands.rs`
- Modify: `src-tauri/src/lib.rs` (`mod offline_commands;` + register 3 commands)
- Test: `src-tauri/src/owner_ops.rs` `#[cfg(test)]` module

**Interfaces:**
- Consumes: `load_definition_from_path`, `Tune::new`, `opentune_project::msq::{load_msq_into, tune_to_msq}`, `DefinitionDto::from(&Definition)`, `request`, `Command`.
- Produces:
  - `Command::{NewTune{ini_path,reply:Reply<DefinitionDto>}, OpenTune{ini_path,msq_path,reply:Reply<DefinitionDto>}, SaveTune{path,reply:Reply<()>}}`
  - `build_offline_session(&str) -> Result<Session, String>`, `build_offline_session_from_msq(&str, &str) -> Result<Session, String>`
  - Commands `new_tune`, `open_tune`, `save_tune` (TS: `commands.newTune/openTune/saveTune`).

- [ ] **Step 1: Wire the `project` crate as an app dependency**

In `src-tauri/Cargo.toml` `[dependencies]`, add:

```toml
opentune-project = { path = "crates/project" }
```

- [ ] **Step 2: Write the failing offline-session test**

Add to `src-tauri/src/owner_ops.rs` (create a `#[cfg(test)] mod tests` if absent):

```rust
#[cfg(test)]
mod offline_build_tests {
    use super::*;

    const INI: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/speeduino.sample.ini");

    #[test]
    fn build_offline_session_has_no_conn_and_a_tune() {
        let s = build_offline_session(INI).unwrap();
        assert!(s.conn.is_none());
        assert!(s.tune.is_some());
        assert!(!s.def.comms.signature.is_empty());
    }
}
```

- [ ] **Step 3: Run it — verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml build_offline_session_has_no_conn`
Expected: FAIL — `build_offline_session` undefined.

- [ ] **Step 4: Implement the offline builders in `owner_ops.rs`**

Add near `build_session` (ensure `use opentune_model::Tune;` is present in the module):

```rust
/// Build an offline session (no ECU link) around a blank tune from `ini_path`.
pub(super) fn build_offline_session(ini_path: &str) -> Result<Session, String> {
    let def = Arc::new(load_definition_from_path(ini_path)?);
    let tune = Tune::new(Arc::clone(&def));
    Ok(Session {
        conn: None,
        def,
        tune: Some(tune),
        snapshot: None,
    })
}

/// Build an offline session and load a `.msq` into its tune (signature-checked).
pub(super) fn build_offline_session_from_msq(
    ini_path: &str,
    msq_path: &str,
) -> Result<Session, String> {
    let mut session = build_offline_session(ini_path)?;
    let xml = std::fs::read_to_string(msq_path)
        .map_err(|e| format!("cannot read `{msq_path}`: {e}"))?;
    let tune = session.tune.as_mut().expect("offline session always has a tune");
    opentune_project::msq::load_msq_into(tune, &xml).map_err(|e| e.to_string())?;
    Ok(session)
}
```

- [ ] **Step 5: Add the owner Command variants + serve arms + methods**

In `src-tauri/src/owner.rs`, add to the `Command` enum:

```rust
    NewTune { ini_path: String, reply: Reply<DefinitionDto> },
    OpenTune { ini_path: String, msq_path: String, reply: Reply<DefinitionDto> },
    SaveTune { path: String, reply: Reply<()> },
```

Add serve arms (inside `serve`'s match):

```rust
            Command::NewTune { ini_path, reply } => {
                let _ = reply.send(self.new_tune(ini_path).await);
            }
            Command::OpenTune { ini_path, msq_path, reply } => {
                let _ = reply.send(self.open_tune(ini_path, msq_path).await);
            }
            Command::SaveTune { path, reply } => {
                let r = self
                    .with_session(move |s| {
                        let tune = s.tune.as_ref().ok_or_else(|| crate::session::NO_TUNE.to_string())?;
                        let xml = opentune_project::msq::tune_to_msq(tune);
                        std::fs::write(&path, xml).map_err(|e| format!("cannot write `{path}`: {e}"))
                    })
                    .await;
                let _ = reply.send(r);
            }
```

Add a `reset_session` helper and the two async methods to `impl Owner`:

```rust
    fn reset_session(&mut self) {
        self.session = None;
        self.polling = false;
        self.poller = None;
    }

    async fn new_tune(&mut self, ini_path: String) -> Result<DefinitionDto, String> {
        // Build FIRST — a bad INI must not wipe the user's current session.
        let session = tokio::task::spawn_blocking(move || ops::build_offline_session(&ini_path))
            .await
            .map_err(|e| format!("new_tune panicked: {e}"))??;
        let dto = DefinitionDto::from(session.def.as_ref());
        self.reset_session();
        self.session = Some(session);
        Ok(dto)
    }

    async fn open_tune(&mut self, ini_path: String, msq_path: String) -> Result<DefinitionDto, String> {
        // Build FIRST — a bad INI/.msq must not wipe the user's current session.
        let session =
            tokio::task::spawn_blocking(move || ops::build_offline_session_from_msq(&ini_path, &msq_path))
                .await
                .map_err(|e| format!("open_tune panicked: {e}"))??;
        let dto = DefinitionDto::from(session.def.as_ref());
        self.reset_session();
        self.session = Some(session);
        Ok(dto)
    }
```

> NOTE: `ops` is the module name for `owner_ops.rs` (`#[path = "owner_ops.rs"] mod ops;`). Ensure `use crate::dto::DefinitionDto;` and the `opentune_project` path resolve in `owner.rs` (add `use` lines as the compiler directs).

- [ ] **Step 6: Create the Tauri commands**

Create `src-tauri/src/offline_commands.rs`:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
//! Offline tuning: create / open / save a tune with no live ECU link.
use tauri::State;

use crate::dto::DefinitionDto;
use crate::owner::{request, Command, OwnerHandle};

#[tauri::command]
#[specta::specta]
pub async fn new_tune(
    ini_path: String,
    owner: State<'_, OwnerHandle>,
) -> Result<DefinitionDto, String> {
    request(&owner, |reply| Command::NewTune { ini_path, reply }).await
}

#[tauri::command]
#[specta::specta]
pub async fn open_tune(
    ini_path: String,
    msq_path: String,
    owner: State<'_, OwnerHandle>,
) -> Result<DefinitionDto, String> {
    request(&owner, |reply| Command::OpenTune { ini_path, msq_path, reply }).await
}

#[tauri::command]
#[specta::specta]
pub async fn save_tune(path: String, owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::SaveTune { path, reply }).await
}
```

- [ ] **Step 7: Register the module + commands in `lib.rs`**

Add `mod offline_commands;` with the other `mod` lines, and add to `collect_commands![...]` (after the `tune_commands::*` block):

```rust
            offline_commands::new_tune,
            offline_commands::open_tune,
            offline_commands::save_tune,
```

- [ ] **Step 8: Build, test, regenerate TS bindings**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS. The `binding_gen` test re-exports `src/ipc/bindings.ts` with the three new commands (`newTune`/`openTune`/`saveTune`). Confirm they appear:

Run: `grep -E "newTune|openTune|saveTune" src/ipc/bindings.ts`
Expected: all three present. Then clippy + fmt.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/owner.rs src-tauri/src/owner_ops.rs src-tauri/src/offline_commands.rs src-tauri/src/lib.rs src/ipc/bindings.ts
git commit -m "feat(app): offline tune lifecycle commands (new/open/save .msq)"
```

---

## Task 4: Connect-while-offline (ATTACH) + push to ECU

Makes `connect` keep an offline tune instead of replacing it, verifies the signature,
and adds a `write_tune_to_ecu` push that writes all pages + burns.

**Files:**
- Modify: `src-tauri/src/session.rs` (`write_all_to_ecu`)
- Modify: `src-tauri/src/owner_ops.rs` (`attach_connection`, `verify_signature`)
- Modify: `src-tauri/src/owner.rs` (`connect` ATTACH branch; `WriteTuneToEcu` command + serve arm + method)
- Modify: `src-tauri/src/offline_commands.rs` (`write_tune_to_ecu` command)
- Modify: `src-tauri/src/lib.rs` (register command)
- Test: `src-tauri/src/owner_ops.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: `connect_simulator`, `connect_serial`, `protocol_for`, `Tune::{page_bytes,mark_burned}`, `EcuIdentity::matches`, `ConnectionState`.
- Produces: `Session::write_all_to_ecu(&mut self) -> Result<TuneDirtyEvent, String>`; `Command::WriteTuneToEcu`; command `write_tune_to_ecu` (TS `commands.writeTuneToEcu`).

- [ ] **Step 1: Write the failing ATTACH + push test**

Add to `owner_ops.rs`'s test module:

```rust
    #[test]
    fn attach_keeps_the_offline_tune_and_pushes_to_sim() {
        use crate::owner::Emitter;
        use std::sync::Arc;

        let mut s = build_offline_session(INI).unwrap();
        let emit: Emitter = Arc::new(|_| {});
        attach_connection(&mut s, ConnectSource::Simulator { ini_path: None }, &emit).unwrap();
        assert!(s.conn.is_some(), "attach adds a connection");
        assert!(s.tune.is_some(), "attach never drops the offline tune");
        // The simulator accepts a whole-tune write + burn.
        s.write_all_to_ecu().unwrap();
    }
```

- [ ] **Step 2: Run it — verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml attach_keeps_the_offline_tune`
Expected: FAIL — `attach_connection` / `write_all_to_ecu` undefined.

- [ ] **Step 3: Implement `write_all_to_ecu` in `session.rs`**

```rust
    /// Push the entire tune to the ECU: write every page's bytes, then burn
    /// each page. Used by the offline "Write to ECU" action, which has no
    /// read baseline to diff against. Requires a live connection.
    pub fn write_all_to_ecu(&mut self) -> Result<TuneDirtyEvent, String> {
        let Session { conn, def, tune, .. } = self;
        let conn = conn.as_ref().ok_or_else(|| NO_CONNECTION.to_string())?;
        let tune = tune.as_mut().ok_or_else(|| NO_TUNE.to_string())?;
        let mut proto = protocol_for(conn, &def.comms)?;
        for page in &def.pages {
            proto
                .write(page.number, 0, tune.page_bytes(page.number))
                .map_err(|e| e.to_string())?;
        }
        for page in &def.pages {
            proto.burn(page.number).map_err(|e| e.to_string())?;
        }
        tune.mark_burned();
        Ok(dirty_event(tune))
    }
```

> NOTE: confirm `Protocol::write(&mut self, page: u16, offset: u16, bytes: &[u8])` signature (from `write_deltas`, `proto.write(*page, offset, bytes)`); `0` is the page-start offset. Against the simulator this succeeds; against serial it returns `SERIAL_UNSUPPORTED` (expected — scope boundary #1).
>
> BLOCKING-FACTOR CAVEAT: this writes a whole page in one `proto.write` — a new access pattern (M2 only ever wrote small deltas). A page may exceed `comms.blocking_factor` (251). The Step 1 test against the simulator confirms the sim accepts it; **if the sim rejects an oversized write, chunk the page into `blocking_factor`-sized spans** (`for chunk_offset in (0..len).step_by(blocking_factor) { proto.write(page, chunk_offset, &bytes[..]) }`). Regardless: when real serial write lands (lifting `SERIAL_UNSUPPORTED`), it **must** chunk by `blocking_factor` — leave a one-line comment saying so.

- [ ] **Step 4: Implement `attach_connection` + `verify_signature` in `owner_ops.rs`**

```rust
use opentune_protocol::ConnectionState;

/// Attach a live link to an existing offline session **without** reading the
/// tune (which would overwrite the user's offline edits). Verifies the ECU
/// signature matches the offline tune's INI before attaching.
pub(super) fn attach_connection(
    session: &mut Session,
    source: ConnectSource,
    emit: &Emitter,
) -> Result<(), String> {
    let emit_cs =
        |cs: ConnectionState| emit(OwnerEvent::Connection(ConnectionStateEvent::from(cs)));
    let conn = match source {
        // ATTACH ignores the source's ini_path: the offline def is authoritative.
        ConnectSource::Simulator { .. } => connect_simulator(session.def.as_ref(), &emit_cs)?,
        ConnectSource::Serial { ref port_name, .. } => {
            connect_serial(port_name.clone(), session.def.comms.clone(), &emit_cs)?
        }
    };
    verify_signature(&conn, &session.def)?;
    session.conn = Some(conn);
    Ok(())
}

/// Guard #2: the connected ECU's signature must match the tune's INI.
fn verify_signature(conn: &ActiveConnection, def: &Definition) -> Result<(), String> {
    match conn {
        // The simulator is built from `def`, so its identity always matches.
        ActiveConnection::Sim { .. } => Ok(()),
        ActiveConnection::Serial { manager } => match manager.state() {
            ConnectionState::Connected { identity } if identity.matches(&def.comms) => Ok(()),
            ConnectionState::Connected { identity } => Err(format!(
                "connected ECU signature `{}` does not match your tune's INI `{}`",
                identity.signature, def.comms.signature
            )),
            _ => Err("ECU did not report a signature; cannot verify tune compatibility".to_string()),
        },
    }
}
```

> NOTE: confirm the manager state accessor — `ConnectionManager::state()` returning `&ConnectionState` (adapt to the real name, e.g. `current_state()`, if different). `EcuIdentity::matches(&CommsSettings)` is `protocol/src/lib.rs:47`. Ensure `owner_ops.rs` imports `Definition`, `ActiveConnection`, `Emitter`, `OwnerEvent`, `ConnectionStateEvent`, `ConnectionState`.

- [ ] **Step 5: Branch `connect` for ATTACH in `owner.rs`**

Replace the body of `Owner::connect` (lines 420-432) with:

```rust
    async fn connect(&mut self, source: ConnectSource) -> Result<(), String> {
        // ATTACH: an offline tune is loaded — keep it, just add the link.
        if matches!(&self.session, Some(s) if s.conn.is_none() && s.tune.is_some()) {
            let mut session = self.session.take().expect("checked by matches! above");
            let emit = Arc::clone(&self.emit);
            let session = tokio::task::spawn_blocking(move || {
                ops::attach_connection(&mut session, source, &emit)?;
                Ok::<_, String>(session)
            })
            .await
            .map_err(|e| format!("attach panicked: {e}"))??;
            self.session = Some(session);
            return Ok(());
        }
        // FRESH: replace the session and read the tune from the ECU (unchanged).
        self.reset_session();
        let emit = Arc::clone(&self.emit);
        let session = tokio::task::spawn_blocking(move || ops::build_session(source, &emit))
            .await
            .map_err(|e| format!("connect panicked: {e}"))??;
        self.session = Some(session);
        Ok(())
    }
```

- [ ] **Step 6: Add the `WriteTuneToEcu` command + serve arm + method**

`Command` variant:

```rust
    WriteTuneToEcu { reply: Reply<()> },
```

serve arm:

```rust
            Command::WriteTuneToEcu { reply } => {
                let r = self.with_session(Session::write_all_to_ecu).await;
                self.emit_dirty(&r);
                let _ = reply.send(r.map(|_| ()));
            }
```

Command in `offline_commands.rs`:

```rust
#[tauri::command]
#[specta::specta]
pub async fn write_tune_to_ecu(owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::WriteTuneToEcu { reply }).await
}
```

Register in `lib.rs` `collect_commands!`:

```rust
            offline_commands::write_tune_to_ecu,
```

- [ ] **Step 7: Build, test, regenerate bindings**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS (ATTACH test green). Confirm the binding:

Run: `grep -E "writeTuneToEcu" src/ipc/bindings.ts`
Expected: present. Then clippy + fmt.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/session.rs src-tauri/src/owner.rs src-tauri/src/owner_ops.rs src-tauri/src/offline_commands.rs src-tauri/src/lib.rs src/ipc/bindings.ts
git commit -m "feat(app): attach-on-connect keeps offline edits + write-to-ECU push"
```

---

## Task 5: Frontend plumbing — dialog plugin, `offline` store flag, TunePanel gating

Adds the file-dialog plugin and reworks the tune store + `TunePanel` so a tune loaded
with no live link (offline) is shown and survives disconnect, while a live-read tune
still resets on disconnect exactly as today.

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `tauri-plugin-dialog`)
- Modify: `package.json` (add `@tauri-apps/plugin-dialog`)
- Modify: `src-tauri/src/lib.rs` (`.plugin(tauri_plugin_dialog::init())`)
- Modify: `src-tauri/capabilities/*.json` (grant dialog permission)
- Modify: `src/stores/tune.ts` (`offline` flag + `setOfflineDefinition`)
- Modify: `src/components/dialogs/TunePanel.tsx` (loader effect + reset effect + render gate)
- Test: `src/stores/tune.test.ts` (vitest)

**Interfaces:**
- Consumes: `useTuneStore`, `commands`, `isLinkAlive`.
- Produces: store fields `offline: boolean` + action `setOfflineDefinition(def)`; `setDefinition` now also sets `offline:false`; `TunePanel` renders on `definition != null` and resets only on `!linkAlive && !offline`.

- [ ] **Step 1: Install the dialog plugin (Rust + npm)**

`src-tauri/Cargo.toml` `[dependencies]`:

```toml
tauri-plugin-dialog = "2"
```

`package.json` `dependencies`:

```json
"@tauri-apps/plugin-dialog": "^2",
```

Run `npm install`. Register in `src-tauri/src/lib.rs` `run()` next to the opener plugin:

```rust
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
```

Grant the permission: in the app's capabilities file (find it — `grep -rl "permissions" src-tauri/capabilities`), add `"dialog:default"` to the `permissions` array. Without this, `open`/`save` reject at runtime.

- [ ] **Step 2: Write the failing store test**

Create `src/stores/tune.test.ts`:

```ts
import { beforeEach, describe, expect, it } from "vitest";
import { useTuneStore } from "./tune";
import type { DefinitionDto } from "../ipc/bindings";

const DEF = {
  signature: "x",
  menus: [],
  dialogs: [],
  constants: [],
  tables: [],
  curves: [],
  gauges: [],
  frontpage: { gaugeSlots: [], indicators: [] },
} as unknown as DefinitionDto;

describe("tune store offline flag", () => {
  beforeEach(() => useTuneStore.getState().reset());

  it("is offline=false initially", () => {
    expect(useTuneStore.getState().offline).toBe(false);
    expect(useTuneStore.getState().definition).toBeNull();
  });

  it("setOfflineDefinition marks the tune offline", () => {
    useTuneStore.getState().setOfflineDefinition(DEF);
    expect(useTuneStore.getState().offline).toBe(true);
    expect(useTuneStore.getState().definition).not.toBeNull();
  });

  it("setDefinition (online) is not offline", () => {
    useTuneStore.getState().setDefinition(DEF);
    expect(useTuneStore.getState().offline).toBe(false);
  });

  it("reset clears offline + definition", () => {
    useTuneStore.getState().setOfflineDefinition(DEF);
    useTuneStore.getState().reset();
    expect(useTuneStore.getState().offline).toBe(false);
    expect(useTuneStore.getState().definition).toBeNull();
  });
});
```

- [ ] **Step 3: Run it — verify it fails**

Run: `npm test -- tune.test`
Expected: FAIL — `offline` / `setOfflineDefinition` do not exist.

- [ ] **Step 4: Add `offline` + `setOfflineDefinition` to the store**

In `src/stores/tune.ts`: add `offline: boolean;` and `setOfflineDefinition: (definition: DefinitionDto) => void;` to the `TuneStore` interface; add `offline: false` to `INITIAL`; change/add the actions:

```ts
  setDefinition: (definition) => set({ definition, offline: false }),
  setOfflineDefinition: (definition) => set({ definition, offline: true }),
```

- [ ] **Step 5: Run the store test — verify pass**

Run: `npm test -- tune.test`
Expected: PASS (4 tests).

- [ ] **Step 6: Rework `TunePanel` for offline**

In `src/components/dialogs/TunePanel.tsx`:

(a) subscribe to the flag near the other store reads (after line 55):

```tsx
  const offline = useTuneStore((s) => s.offline);
```

(b) Replace the loader effect (lines 89-117) with a version that loads the definition from the ECU only on a fresh online connect, and otherwise just refreshes values (wire-free, so it also serves the offline case):

```tsx
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const store = useTuneStore.getState();
      if (store.definition) {
        // Definition already present (offline via OfflinePanel, or a prior
        // connect). Re-read values + conditions; never touch the wire, never
        // reload the tune (that would overwrite offline edits on attach).
        if (!cancelled) await refresh(store.definition);
        return;
      }
      if (!linkAlive) return; // nothing loaded yet and no link — show nothing
      const defRes = await commands.getDefinition();
      if (defRes.status !== "ok" || cancelled) {
        if (defRes.status === "error") setError(defRes.error);
        return;
      }
      const def = defRes.data;
      store.setDefinition(def);
      const firstDialog =
        def.menus[0]?.items[0]?.dialog ?? def.dialogs[0]?.name ?? null;
      store.setActiveDialog(firstDialog);
      const loadRes = await commands.loadTune();
      if (loadRes.status === "error") {
        setError(loadRes.error);
        return;
      }
      if (!cancelled) await refresh(def);
    })();
    return () => {
      cancelled = true;
    };
  }, [linkAlive, definition, refresh]);

  // Reset on a *true* disconnect — but only for a live-read tune. A file-backed
  // offline tune survives so the user can keep editing after unplugging.
  useEffect(() => {
    if (!linkAlive && !useTuneStore.getState().offline) {
      useTuneStore.getState().reset();
    }
  }, [linkAlive]);
```

(c) Relax the render gate (line 155) so an offline tune (no link) still renders:

```tsx
  if (!definition) {
    return null;
  }
```

> NOTE: `Dashboard` must hide when the link is down so a persisted-offline `definition` doesn't leave stale gauges on screen. Confirm `Dashboard` gates its own mount on `isLinkAlive` (per the TunePanel doc it does); if it gates only on `definition`, add an `isLinkAlive` guard to it in this step.

- [ ] **Step 7: Full frontend + backend check**

Run: `npm test` then `npm run lint` and `npm run build` (tsc). Expected: green.
Run: `cargo test --manifest-path src-tauri/Cargo.toml` (bindings unchanged here). Expected: green.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/lib.rs src-tauri/capabilities package.json package-lock.json src/stores/tune.ts src/components/dialogs/TunePanel.tsx src/stores/tune.test.ts
git commit -m "feat(app): offline-aware tune store + dialog plugin; TunePanel survives disconnect"
```

---

## Task 6: Offline entry UI + Write-to-ECU button

The user-facing surface: a panel to pick an INI and start a blank tune, open a `.msq`,
or save the current tune; plus a "Write to ECU" button on `TunePanel` for pushing an
offline tune to a connected ECU.

**Files:**
- Create: `src/components/offline/OfflinePanel.tsx`
- Create: `src/components/offline/offline.css`
- Modify: `src/App.tsx` (render `OfflinePanel`)
- Modify: `src/components/dialogs/TunePanel.tsx` (Write-to-ECU button; enable undo/redo offline)
- Modify: `src/i18n/*` (labels)
- Test: `src/components/offline/OfflinePanel.test.tsx` (vitest + mocked commands/dialog)

**Interfaces:**
- Consumes: `@tauri-apps/plugin-dialog` `open`/`save`; `commands.{newTune,openTune,saveTune,writeTuneToEcu}`; `useTuneStore.{setOfflineDefinition,setActiveDialog}`.
- Produces: `OfflinePanel` React component.

- [ ] **Step 1: Write the failing component test**

Create `src/components/offline/OfflinePanel.test.tsx`. Mock the dialog plugin and the IPC commands, click "New tune", assert `commands.newTune` is called with the picked path and the store gets an offline definition:

```tsx
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { OfflinePanel } from "./OfflinePanel";
import { useTuneStore } from "../../stores/tune";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => "/tmp/my.ini"),
  save: vi.fn(async () => "/tmp/out.msq"),
}));
vi.mock("../../ipc/bindings", () => ({
  commands: {
    newTune: vi.fn(async () => ({ status: "ok", data: { signature: "s", menus: [], dialogs: [], constants: [], tables: [], curves: [], gauges: [], frontpage: { gaugeSlots: [], indicators: [] } } })),
    openTune: vi.fn(async () => ({ status: "ok", data: {} })),
    saveTune: vi.fn(async () => ({ status: "ok" })),
    writeTuneToEcu: vi.fn(async () => ({ status: "ok" })),
  },
}));

describe("OfflinePanel", () => {
  beforeEach(() => useTuneStore.getState().reset());

  it("new tune picks an INI and loads an offline definition", async () => {
    const { commands } = await import("../../ipc/bindings");
    render(<OfflinePanel locale="en" />);
    fireEvent.click(screen.getByText(/new tune/i));
    await vi.waitFor(() => expect(commands.newTune).toHaveBeenCalledWith("/tmp/my.ini"));
    await vi.waitFor(() => expect(useTuneStore.getState().offline).toBe(true));
  });
});
```

- [ ] **Step 2: Run it — verify it fails**

Run: `npm test -- OfflinePanel`
Expected: FAIL — `OfflinePanel` does not exist.

- [ ] **Step 3: Implement `OfflinePanel`**

Create `src/components/offline/OfflinePanel.tsx`:

```tsx
// SPDX-License-Identifier: GPL-3.0-or-later
import { useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { commands } from "../../ipc/bindings";
import type { DefinitionDto } from "../../ipc/bindings";
import { useTuneStore } from "../../stores/tune";
import { t, type Locale } from "../../i18n";
import "./offline.css";

async function pickFile(name: string, ext: string): Promise<string | null> {
  const picked = await open({ multiple: false, filters: [{ name, extensions: [ext] }] });
  return typeof picked === "string" ? picked : null;
}

function loadDefinition(def: DefinitionDto): void {
  const store = useTuneStore.getState();
  store.setOfflineDefinition(def);
  const firstDialog = def.menus[0]?.items[0]?.dialog ?? def.dialogs[0]?.name ?? null;
  store.setActiveDialog(firstDialog);
}

export function OfflinePanel({ locale }: { locale: Locale }) {
  const [error, setError] = useState<string | null>(null);

  const newTune = async () => {
    setError(null);
    const ini = await pickFile("INI", "ini");
    if (!ini) return;
    const res = await commands.newTune(ini);
    if (res.status === "error") return setError(res.error);
    loadDefinition(res.data);
  };

  const openTune = async () => {
    setError(null);
    const ini = await pickFile("INI", "ini");
    if (!ini) return;
    const msq = await pickFile("Tune", "msq");
    if (!msq) return;
    const res = await commands.openTune(ini, msq);
    if (res.status === "error") return setError(res.error);
    loadDefinition(res.data);
  };

  const saveTune = async () => {
    setError(null);
    const path = await save({ filters: [{ name: "Tune", extensions: ["msq"] }] });
    if (typeof path !== "string") return;
    const res = await commands.saveTune(path);
    if (res.status === "error") setError(res.error);
  };

  const hasTune = useTuneStore((s) => s.definition !== null);

  return (
    <section className="offline-panel" aria-label={t("offline.title", locale)}>
      <h2>{t("offline.title", locale)}</h2>
      <div className="offline-actions">
        <button type="button" onClick={newTune}>{t("offline.new", locale)}</button>
        <button type="button" onClick={openTune}>{t("offline.open", locale)}</button>
        <button type="button" onClick={saveTune} disabled={!hasTune}>
          {t("offline.save", locale)}
        </button>
      </div>
      {error && <p className="offline-error">{error}</p>}
    </section>
  );
}
```

Create `src/components/offline/offline.css` (use existing tokens; keep it small):

```css
.offline-panel { display: flex; flex-direction: column; gap: var(--space-2, 0.5rem); padding: var(--space-3, 0.75rem); }
.offline-actions { display: flex; gap: var(--space-2, 0.5rem); flex-wrap: wrap; }
.offline-error { color: var(--color-danger, #c0392b); }
```

Add i18n labels (`offline.title`/`offline.new`/`offline.open`/`offline.save` and `tune.writeToEcu`) to `src/i18n` for both `en` and `pl`, matching the existing label structure.

- [ ] **Step 4: Add the Write-to-ECU button + enable offline undo/redo in `TunePanel`**

In `TunePanel.tsx`, inside `.tune-actions` (after the redo button), add:

```tsx
          <button
            type="button"
            onClick={() => runAndRefresh(() => commands.writeTuneToEcu())}
            disabled={!offline || !isConnected}
          >
            {t("tune.writeToEcu", locale)}
          </button>
```

Undo/redo work offline now (Task 2), so drop their `!isConnected` gate — change both to
`disabled={false}` (or remove the `disabled` prop). Leave **burn** gated on `!dirty || !isConnected` (burn needs a link).

- [ ] **Step 5: Render `OfflinePanel` in `App.tsx`**

Import `OfflinePanel` and render it alongside `Connect` (the pre-connection surface). Read `src/App.tsx` first to match its layout/props (it passes `locale` to child panels); add `<OfflinePanel locale={locale} />` near `<Connect .../>`.

- [ ] **Step 6: Run everything**

Run: `npm test` (OfflinePanel + store green), `npm run lint`, `npm run build`.
Run: `cargo test --manifest-path src-tauri/Cargo.toml`.
Expected: all green.

- [ ] **Step 7: Manual smoke (the demo)**

`npm run tauri dev`, then: **New tune** → pick `src-tauri/resources/speeduino.sample.ini` → edit a table/dialog value → **Save** to a `.msq` → **Open** it back → connect to the **Simulator** → **Write to ECU** (succeeds against the sim). Confirm disconnecting keeps the tune editable.

- [ ] **Step 8: Commit**

```bash
git add src/components/offline src/components/dialogs/TunePanel.tsx src/App.tsx src/i18n
git commit -m "feat(app): offline entry panel + write-to-ECU button"
```

---

## Self-review notes

- **Spec coverage:** `.msq` read/write (Task 1) · optional-conn editing (Task 2) · new/open/save (Task 3) · attach + push + both signature guards (Tasks 1 & 4) · dialog plugin + store + gating (Task 5) · UI + write button (Task 6). `.msq`↔INI guard = Task 1 `SignatureMismatch`; tune↔ECU guard = Task 4 `verify_signature`. Simulator-only push = scope boundary honored (Task 4 note). `hasTune` realized as `definition !== null` + `offline` flag (deviation recorded in Task 5 insight).
- **Robustness:** `load_msq_into` never aborts mid-apply — unknown constants → `skipped`, bad values/labels → `failed`; only a whole-file signature mismatch is fatal (Task 1, `bad_bit_label_is_collected_not_fatal` test). This matters because real `.msq` bit-field labels may not byte-match the INI's parsed options.
- **Open confirmations for the implementer (flagged inline):** exact `Definition`/`CommsSettings` field set; `quick-xml` 0.36 trim API; `ConnectionManager` state accessor name; whole-page write vs `blocking_factor` (Task 4 caveat — verify against the sim); `Dashboard` link-gating; capabilities permission string; `App.tsx` layout.

