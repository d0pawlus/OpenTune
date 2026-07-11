# M4 — Table Editors & Auto-tune Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Edit VE/ignition/AFR tables and improve them from data: make the *unmodified real*
`speeduino.ini` load diagnostic-clean (the two parser walls), parse the full
`[TableEditor]`/`[CurveEditor]`/`[VeAnalyze]` grammar, edit tables in a semantic-DOM 2D
heatmap grid (interpolate/smooth/scale/copy-paste/keyboard) and 1D curve editors, view a
lazy-loaded three.js 3D surface with a live operating-point dot, and run the first
deterministic, auditable `opentune-analysis::ve_analyze` over an owner-side realtime capture
ring — demoable end-to-end against the simulator, whose new cell-dependent VE-error surface
the analysis visibly flattens.

**Architecture:** Contracts freeze first (Task 0): the `TableDef`/`CurveDef` extension +
`VeAnalyzeDef` in `ini`, the new `opentune-analysis` crate seams (`SampleSet`, `TableGrid`,
`ve_analyze`, `VeAnalysisReport` with per-cell confidence), the owner `Command` variants for
cell writes/capture/analyze, and the DTOs — pinned by contract tests with a struct-literal
fixup sweep (M2/M3 precedent). Real-INI ingestion (Task 1) lands **before any editor UI**:
wall #1 (comms keys scattered into `[Constants]`/`[OutputChannels]`) and wall #2
(`lastOffset` = previous field's *start*, the AFR↔Lambda aliasing bug) are both parser-local;
the gate is the vendored, unmodified `speeduino.ini` @ `0832dc1d` parsing with zero
non-allowlisted diagnostics. Editing reuses the M2 wire path: a new `Tune::set_cells`
(decode-modify-set, one undo `Edit` per gesture) rides the existing validate-on-clone →
minimal-span page write → commit pipeline — **no protocol change** (the session's
`page_deltas` already writes only changed bytes). The 2D grid is a semantic DOM
`<table role="grid">` (keyboard/a11y/clipboard for free — recorded exception to
ARCHITECTURE §3's canvas wording); cell ops are pure frontend functions over a selection.
The 3D surface is raw three.js behind `React.lazy` (chunk-split; the operating-point dot
reads the M3 reflect-only realtime store via rAF). `ve_analyze` consumes a bounded
column-oriented ring captured in the owner during live sessions (the minimal seam that does
not constrain M5's datalog design, ARCHITECTURE §5.5); analysis runs backend-side — raw
samples never cross IPC, only the report does. Per [ADR-0002](../../adr/0002-data-driven-ini.md)
everything is driven by the parsed `Definition` — including the `[VeAnalyze]` binding.

**Tech Stack:** Rust (stable), `tokio` (existing owner task), `thiserror`, `serde` +
`specta`/`tauri-specta` (generated IPC types — never hand-edit `src/ipc/bindings.ts`),
React + TS + Zustand, **`three` (the single ROADMAP-sanctioned new frontend dep;
`@types/three` dev-dep rides the same allowance)**, vitest + @testing-library/react.

## Global Constraints

These apply to **every task**. Values from [ARCHITECTURE.md](../../ARCHITECTURE.md),
[ROADMAP.md §M4](../../ROADMAP.md#m4--table-editors--auto-tune-), the
[M4 research dossier](../../notes/m4-research.md), [m3-decisions.md](../../notes/m3-decisions.md),
[m2-decisions.md](../../notes/m2-decisions.md), the
[AI tuning & analysis design](../specs/2026-06-21-ai-tuning-and-analysis-design.md), and the ADRs.

- **Port, don't re-derive.** Per [ADR-0006](../../adr/0006-reuse-existing-parsers.md): the
  richer `[TableEditor]`/`[CurveEditor]` shape **ports** from
  [`hyper-tuner/ini` + `hyper-tuner/types`](https://github.com/hyper-tuner/ini) (MIT,
  Piotr Rogowski — `parseTables`/`parseCurves`, `config.ts:153-176`; keep the MIT copyright
  line in port-note headers, the M3 license lesson). `[VeAnalyze]` parsing, the `lastOffset`
  fix, the comms-scatter fix, table math, the 2D/1D/3D editors, `ve_analyze`, and the
  simulator VE-error surface are **write-fresh** (hyper-tuner stops at `[Datalog]`;
  hypertuner-cloud MIT is read-only — nothing to lift; LibreTune GPL-2 and hypertuner GPL
  are **study only**, ADR-0007; TunerStudio/MegaLogViewer are behavioral references only —
  no code; Speeduino firmware + `speeduino.ini` GPL-3 are the **semantics truth source**).
  **Each port task's first sub-step confirms the source actually covers that surface; if
  not, write fresh and record the choice.** Record each source + license in the test/module
  header.
- **TDD is mandatory.** Failing test first, minimal code to pass, then refactor. No
  implementation without a failing test. Every existing M1/M2/M3 test must stay green.
- **Determinism (analysis).** `ve_analyze` is a pure function: same input → identical
  output. **No RNG; no HashMap iteration in float accumulation** (fixed sample order, fixed
  cell indexing, no parallel reduce); explicit thresholds as params; every result carries
  *why* (per-cell hit weight/count/confidence + per-filter reject counts).
- **Immutable `Definition`, owned `Tune`.** The parsed `Definition` stays frozen and shared
  (`Arc`); editors and analysis read it, never mutate it.
- **Single conversation.** All wire access stays serialized through the M3 owner task
  (ARCHITECTURE §9); cell writes are owner commands; capture taps the owner's own poll tick.
- **Fail-open, per item.** One malformed table/curve/filter row records a `Diagnostic` and
  is skipped — it never fails the parse. One missing channel in a capture row degrades to
  `NaN` (and a filter reason in analysis) — it never aborts the run.
- **IPC types are generated from Rust** via `tauri-specta` — never hand-write
  `src/ipc/bindings.ts`. **specta 0.0.12 forbids `usize`/`u64` over IPC** → all
  IPC-reachable numeric fields use `u32`/`f64` (the `dto.rs` pattern). New commands/events
  **must** be added to `collect_commands!`/`collect_events!` **and** to the `binding_gen`
  needle tests in `src-tauri/src/lib.rs`.
- **Frontend deps: `three` only.** `npm i three` (+ `@types/three` as dev-dep) is the one
  sanctioned addition (ROADMAP M4). Nothing else — clipboard is `navigator.clipboard`, the
  curve preview is inline SVG, the grid is DOM.
- **Bundle budget (app page < 300 kB gz total).** three@0.182 is ~175 kB gz for a full
  import — the ROADMAP's "~150 kB" is a tree-shaking floor, so the budget only closes with
  **lazy loading**: the eager main chunk must stay **< 125 kB gz** and three must land in a
  **separate lazy chunk** (≤ 180 kB gz), loaded only when the user opens the 3D view. Any
  static `import ... from 'three'` reachable from the eager graph is a build failure for
  this plan (Task 7 measures both chunks).
- **License header** on every new source file: `// SPDX-License-Identifier: GPL-3.0-or-later`
  (Rust) / the TS equivalent.
- **Small focused files** (<400 lines for new files); immutable patterns; `cargo fmt` +
  `cargo clippy -- -D warnings` clean; `prettier` + `eslint` clean on TS.
- **i18n:** new UI strings go through the typed dictionaries; `pl` must mirror `en` keys
  exactly.
- **Commits:** conventional-commit format, scoped per component, **no attribution trailers**.
- **`cargo` is not on PATH** — prefix every cargo command with `. "$HOME/.cargo/env" &&`.
- **Frontend tooling is `npm`** (not pnpm): `npm test`, `npm run lint`, `npm run build`.

---

## Env facts (carry into every task)

- Workspace crates live at **`src-tauri/crates/*`**; package names `opentune-{ini,model,protocol,transport,simulator,realtime,datalog,project}` — M4 adds `opentune-analysis` at `src-tauri/crates/analysis`.
- `binding_gen` test in `src-tauri/src/lib.rs`: `build_specta()` is the single command/event registration list; a `BINDINGS_LOCK` mutex serializes the export→read tests. New commands/events need a needle assertion there.
- Backend commands are thin async senders: `#[tauri::command] #[specta::specta] async fn` + `State<'_, OwnerHandle>` + oneshot reply (`owner.rs` `Command` enum; M3 pattern).
- The owner task owns `Option<Session>` + the realtime poll tick; `Session` owns `conn` + `def: Arc<Definition>` + `tune: Option<Tune>` + `snapshot`; all wire I/O goes through `&mut Session` methods, run inside `spawn_blocking`.
- Frontend: vitest + @testing-library/react; Zustand stores (reflect-only realtime store read imperatively via `getState()` in rAF loops); typed i18n dicts (`src/i18n/{en,pl}.ts`); `tokens.css` design tokens; `isLinkAlive` predicate gates live-only UI.
- The real INI pin: `noisymime/speeduino @ 0832dc1d25b108cf33b30167284c44e3edd3d35a`, file `reference/speeduino.ini` (6 026 lines, ~463 kB, `signature "speeduino 202504-dev"`, `nPages = 15`, `ochBlockSize = 139` l.5353, `ochGetCommand` l.5352 **in `[OutputChannels]`**, comms keys scattered at l.240-274 **in `[Constants]`** as per-page comma lists, 5 `lastOffset` uses — l.642/676/678/1176/1221, `[VeAnalyze]` l.5984).
- **Branch prerequisite:** `m4-table-editors` forked **before** the M3 hot-fix
  `34097f3 fix(simulator): auto-tick engine on realtime requests` (it sits on
  `m3-realtime-dashboard`). Task 0 step 0.0 brings it in — the M4 capture/E2E tasks depend on
  polls actually advancing the simulated engine.
- `npm test` runs vitest (jsdom, config inline in `vite.config.ts`, **no setup file**);
  `t(key, locale)` is a plain function (not a hook), `en` keys are the type source; the four
  M3 explorer reports with verbatim seam quotes live at `.superpowers/sdd/m4-*-exploration.md`
  (ini / backend / frontend / sim) — consult them before touching a seam.

---

## Decisions locked for M4 (record in `docs/notes/m4-decisions.md` as they land)

Resolving the dossier's 8 open decisions + the M3 follow-ups. Criterion (as M2/M3): optimal,
non-blocking for future development.

1. **Real-INI ingestion is the M4 gate, and it lands first** (Task 1, before any editor UI).
   Wall #1: `parse_comms` learns to collect scattered comms keys from `[Constants]`/
   `[OutputChannels]` (first-wins per key; comma-list values take the first element;
   `ochGetCommand` from `[OutputChannels]` keeps winning per the M3 override). Wall #2:
   `lastOffset` resolves to the **previous field's start offset** (TS semantics — all 5 real
   uses are AFR↔Lambda aliases), not the running end. Acceptance: the vendored, unmodified
   real `speeduino.ini` parses with **zero non-allowlisted diagnostics** (allowlist =
   recorded-deferred constructs only). (Dossier decision 3 → yes.)
2. **Vendor the full real INI as a fixture** (~463 kB, GPL-3 into this GPL-3-or-later repo —
   license-compatible; byte-identical to upstream, provenance in the test header, fetched at
   the pinned SHA). A trimmed fixture would dodge exactly the constructs that broke us twice.
3. **`opentune-analysis` seeded now** (crate dir `crates/analysis`, §5.9 name `analysis`
   wins over the design doc's `tuning` float), with `ve_analyze` **only** — M5 grows it.
   Zero deps; pure functions over self-contained `SampleSet`/`TableGrid` inputs.
   (Dossier decision 4.)
4. **Capture = owner-side bounded ring** (dossier decision 5): column-oriented
   (`columns: Vec<String>` pinned at start + one `Vec<f64>` row per emitted frame,
   missing channel → `NaN`), capacity 27 000 rows (~15 min at the ~25 Hz emit rate),
   oldest-drops-first. Commands: `start_capture`/`stop_capture`/`capture_status`.
   **Raw samples never cross IPC** — `run_ve_analyze` executes in the owner (it has the
   `Definition`, the `Tune`, and the ring) and only the `VeAnalysisReport` DTO crosses.
5. **`[VeAnalyze]` is parsed, not hardcoded** (dossier decision 6 → parse; ADR-0002):
   `veAnalyzeMap` rows + `filter` rows (std + custom with `<`/`>`/`=`/`&` ops) become
   `VeAnalyzeDef`. `lambdaTargetTables` and `[WueAnalyze]` are known-but-deferred (silently
   skipped, recorded). The trailing filter flag is captured verbatim as `default_on`
   (semantics unconfirmed against TS — all parsed filters are applied regardless; params
   can disable by id).
6. **2D grid substrate = semantic DOM `<table role="grid">`** (dossier decision 1 → DOM):
   256 cells is trivial for DOM; keyboard nav, TSV clipboard, and ARIA come free. Recorded
   as an explicit exception to ARCHITECTURE §3's "canvas for 2D grids" (that verdict was for
   30 Hz gauge redraw). Canvas/WebGL stays for the 3D surface + live overlay.
7. **Cell ops are pure frontend functions** over the selection (dispatch decision — this
   deliberately overrides the dossier's Rust-core recommendation; recorded): interpolate/
   smooth/scale/set-equal in `tableOps.ts`, vitest-covered, shared by the 2D grid and the
   curve editor. `ve_analyze`'s math stays in Rust (one deterministic engine for corrections).
8. **Cell writes: `Tune::set_cells(name, &[(index, value)])` decode-modify-set** (dossier
   decision 2 → model seam, minimal form): validates + re-encodes via the existing
   `set(name, Value::Array)` path, so undo stays **one `Edit` per gesture** and the wire
   already writes only the **minimal changed byte span** (the session's `page_deltas` diffing
   from M2). No protocol change ⇒ the M3 `pages.rs`-split follow-up does **not** fire
   (recorded as still armed). Multi-cell gestures (paste/smooth/interpolate) coalesce into
   one `set_cells` call = one undo step = one span write.
9. **three.js raw + `React.lazy`** (dossier decision 7 → raw): no `@react-three/fiber`.
   `OrbitControls` from `three/examples/jsm` (in-package, no extra dep). WKWebView pitfalls
   pinned: `setPixelRatio(Math.min(devicePixelRatio, 2))`, `webglcontextlost`/`restored`
   handlers, dispose-on-unmount, no per-frame allocation. Budget gate measured in Task 7.
10. **Curve editor reuses the grid machinery** (dossier decision 8 → grid path first): a
    1×N `TableGrid` over `yBins` values + an inline-SVG line preview + rAF live cursor.
    **Axis-bin editing (rebinning) is deferred for both tables and curves** — M4 edits
    z-values/y-values only (recorded).
11. **Simulator closes the loop:** measured `afr = afrTarget × trueVE(rpm, load) /
    currentVE(rpm, load)` where `currentVE` is bilinear-read from the sim's **own memory
    image** (the veTable page) and `trueVE` is a fixed deterministic surface. Applying the
    analysis moves `currentVE` toward `trueVE` ⇒ measured AFR converges to target — the
    demo *provably* flattens the error. `egoCorrection` is emitted at a constant 100 (no
    trim; Speeduino's channel is 100-centered) — EGO-neutralization math is unit-tested in
    `analysis` with synthetic samples instead.
12. **M3 follow-ups folded/excluded:** `get_values` unknown-name→NaN sentinel — **already
    closed in M3** (recorded, nothing to do). Serial-only follow-ups (persistent serial
    `MsProtocol`, poll backoff, reconnect gating of field/diff/merge, windowed-INI keepalive)
    stay **out** — recorded as M3-serial; table writes over serial do **not** block sim-first
    M4. `pages.rs`/`constants_fields.rs` split threshold: no M4 task adds to `pages.rs`;
    Task 1's `constants_fields.rs` change is a localized fix, not an addition — both splits
    stay armed for the next real addition (recorded).

---

## File structure (created / modified)

| Crate / area | Files | Responsibility |
| --- | --- | --- |
| `ini` | `src/parser.rs` (comms scatter), `src/constants_fields.rs` (`lastOffset`), `src/ui.rs` (extend `TableDef`/`CurveDef`), `src/ui_table_curve_parser.rs` (richer parse), `src/ve_analyze.rs` (new — `VeAnalyzeDef` + parser), `src/definition.rs` (wire + `ve_analyze` field), `src/lib.rs` (re-exports) | real-INI walls; full table/curve/analyze grammar |
| `analysis` (new crate) | `Cargo.toml`, `src/lib.rs` (seams), `src/ve_analyze.rs` (algorithm), `src/grid.rs` (`TableGrid` + bilinear) | deterministic `ve_analyze` + report |
| `model` | `src/tune.rs` (`set_cells`) | per-gesture cell writes |
| `simulator` | `src/och_codec.rs` (afr/ego channels), `src/engine/mod.rs` (VE-error surface), `src/ve_model.rs` (new — memory-backed VE lookup), `src/ecu.rs` (wire VE context), `resources/speeduino.sample.ini` (pages 2-4 + new sections) | measured-AFR loop the analysis can flatten |
| `src-tauri` | `src/owner.rs` (Command variants + capture tap + analyze handler), `src/capture.rs` (new — ring), `src/session.rs` (`set_cells`), `src/tune_commands.rs` (+`set_cells`), `src/analysis_commands.rs` (new), `src/dto.rs` (Table/Curve/Capture/Report DTOs), `src/lib.rs` (registrations + needles) | cell-write + capture + analyze command surface |
| frontend | `src/components/table-editor/{TableEditor,TableGrid,tableOps,selection,tsv,heatmap,table-editor.css}`, `src/components/curve-editor/{CurveEditor,curveMath,curve-editor.css}`, `src/components/surface/{SurfaceView.tsx,surfaceGeometry.ts,surface.css}`, `src/components/autotune/{AutoTunePanel,autotune.css}`, `src/stores/tune.ts` (setCells + editor state), `TunePanel` nav integration, i18n dicts | 2D/1D editors, 3D surface, AutoTune UI |

---

## Task 0 — Freeze the M4 contracts (`TableDef`/`CurveDef` extension, `VeAnalyzeDef`, `opentune-analysis` seams, owner/DTO seams)

**Why first:** M2/M3 parallelized only because their seams were frozen + contract-tested.
Extending `TableDef`/`CurveDef` and adding `Definition.ve_analyze` breaks every struct
literal in the tree; the `analysis` crate's `ve_analyze` signature, the owner `Command`
variants, and the DTOs are what Tasks 2-12 build against. Land all of it with stub bodies,
fix every literal site, and pin each seam with a contract test.

**Files:**
- Modify: `src-tauri/crates/ini/src/ui.rs` (extend `TableDef`/`CurveDef`, add `CurveAxis`)
- Create: `src-tauri/crates/ini/src/ve_analyze.rs` (types only; parser is Task 2)
- Modify: `src-tauri/crates/ini/src/definition.rs` (`ve_analyze` field + `table`/`curve` accessors), `src-tauri/crates/ini/src/lib.rs` (re-exports)
- Create: `src-tauri/crates/analysis/Cargo.toml`, `src-tauri/crates/analysis/src/lib.rs`, `src-tauri/crates/analysis/src/grid.rs` (seam types, stub bodies)
- Modify: `src-tauri/crates/model/src/tune.rs` (`set_cells` signature, stub)
- Modify: `src-tauri/src/dto.rs` (M4 DTOs), `src-tauri/src/owner.rs` (`Command` variants + `Err("not implemented")` stub arms)
- Modify (struct-literal fixups): `src-tauri/crates/ini/src/ui_table_curve_parser.rs`, every `TableDef {`/`CurveDef {`/`Definition {` literal found in step 0.1
- Test: `src-tauri/crates/ini/tests/contract.rs` (extend), `src-tauri/crates/analysis/tests/contract.rs` (new)

**Interfaces — Produces (the frozen seams):**

```rust
// ── ini: ui.rs — TableDef gains the fields the M2 parser dropped ────────────
/// A 2-D/3-D table editor definition. Port shape: hyper-tuner/types
/// `Table` (config.ts:153-166, MIT © Piotr Rogowski). Bins stay lazy string
/// names resolved against `[Constants]` by the consumer.
pub struct TableDef {
    pub name: String,          // editor id, e.g. "veTable1Tbl"
    pub map3d_id: String,      // 3-D map id, e.g. "veTable1Map" ("" when absent)
    pub title: String,         // display title ("" when absent)
    pub page: u32,             // page number from the table header (0 when absent)
    pub x_bins: String,        // X-axis bin constant name
    pub x_channel: String,     // live-cursor output channel (2nd xBins token; "" when absent)
    pub y_bins: String,
    pub y_channel: String,
    pub z: String,             // cell (Z) array constant name
    pub xy_labels: Vec<String>,     // `xyLabels = "RPM", "Fuel Load: "` (empty when absent)
    pub grid_height: f64,           // `gridHeight` (0.0 when absent)
    pub grid_orient: Vec<f64>,      // `gridOrient = 250, 0, 340` (empty when absent)
    pub up_down_label: Vec<String>, // `upDownLabel = "(RICHER)", "(LEANER)"`
    pub help: String,               // `topicHelp` URL ("" when absent)
}

/// One `xAxis`/`yAxis` curve attribute: `min, max, gridDivisions`. Min/max may
/// be `{ expr }` in real INIs (e.g. under `#if LAMBDA`) → captured as `Number`.
pub struct CurveAxis { pub min: Number, pub max: Number, pub divisions: u32 }

pub struct CurveDef {
    pub name: String,
    pub title: String,               // `curve = name, "title"` ("" when absent)
    pub column_labels: Vec<String>,  // `columnLabel = "Temp", "Duty %"`
    pub x_axis: Option<CurveAxis>,
    pub y_axis: Option<CurveAxis>,
    pub x_bins: String,
    pub x_channel: String,           // live-cursor channel (2nd xBins token; "")
    pub y_bins: String,              // the editable data array
    pub gauge: String,               // referenced gauge name ("" when absent)
    pub size: Vec<f64>,              // `size = 400, 400` (empty when absent)
}

// ── ini: ve_analyze.rs — the parsed [VeAnalyze] binding ─────────────────────
pub struct VeAnalyzeDef {
    pub maps: Vec<VeAnalyzeMapDef>,
    pub filters: Vec<AnalyzeFilterDef>,
}
/// `veAnalyzeMap = veTable1Tbl, afrTable1Tbl, afr, egoCorrection`
pub struct VeAnalyzeMapDef {
    pub table: String,          // [TableEditor] id of the table being corrected
    pub target_table: String,   // [TableEditor] id of the AFR/lambda target table
    pub lambda_channel: String, // measured-AFR/lambda output channel
    pub ego_channel: String,    // EGO-correction output channel
}
pub enum AnalyzeFilterDef {
    /// `filter = std_xAxisMin` etc. — carries the raw std name.
    Std(String),
    /// `filter = minCltFilter, "Minimum CLT", coolant, <, 160, , true`
    Custom {
        id: String,
        label: String,
        channel: String,
        op: FilterOp,
        value: f64,
        /// Trailing INI flag, captured verbatim (TS semantics unconfirmed —
        /// OpenTune applies all parsed filters; params can disable by id).
        default_on: bool,
    },
}
pub enum FilterOp { Lt, Gt, Eq, And }

// ── ini: Definition gains one field + two accessors ─────────────────────────
pub struct Definition {
    // ... all M3 fields unchanged ...
    /// `[VeAnalyze]` binding; `None` when the INI declares none.
    pub ve_analyze: Option<VeAnalyzeDef>,
}
impl Definition {
    pub fn table(&self, name: &str) -> Option<&TableDef>;   // mirrors constant()
    pub fn curve(&self, name: &str) -> Option<&CurveDef>;
}

// ── analysis crate (opentune-analysis @ crates/analysis, ZERO deps) ─────────
/// A physical-value table: ascending axis bins + row-major cells
/// (`z[y * x_bins.len() + x]`). Self-contained — no ini dependency.
pub struct TableGrid { pub x_bins: Vec<f64>, pub y_bins: Vec<f64>, pub z: Vec<f64> }
impl TableGrid {
    /// Bilinear lookup at (x, y); None when outside the bins or shape-invalid.
    pub fn lookup(&self, x: f64, y: f64) -> Option<f64>;
}

/// Column-oriented capture: channel names pinned once, one f64 row per frame
/// (missing channel = NaN). The owner's ring buffer produces this.
pub struct SampleSet {
    pub columns: Vec<String>,
    pub t_ms: Vec<f64>,        // per-row ms since capture start (audit only)
    pub rows: Vec<Vec<f64>>,   // rows[i].len() == columns.len()
}
impl SampleSet {
    pub fn column(&self, name: &str) -> Option<usize>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}

/// Which samples to reject, in declaration order (first match wins).
pub enum FilterSpec {
    XAxisMin, XAxisMax, YAxisMin, YAxisMax,
    /// Measured AFR/lambda ≤ 0 (dead sensor).
    DeadLambda,
    /// `channel <op> value` ⇒ rejected. `And` = (channel as i64 & value as i64) != 0.
    Custom { id: String, label: String, channel: String, op: FilterOp, value: f64 },
}
pub enum FilterOp { Lt, Gt, Eq, And }   // analysis's own copy — zero-dep crate

pub struct AnalyzeBinding {
    pub x_channel: String, pub y_channel: String,
    pub afr_channel: String, pub ego_channel: String,
    pub filters: Vec<FilterSpec>,
}

/// Explicit thresholds — every knob is data, none is hidden (§determinism).
pub struct VeAnalyzeParams {
    pub min_weight: f64,            // cells below this summed weight stay unchanged (1.0)
    pub confidence_sat_weight: f64, // weight where weight-confidence saturates to 1 (20.0)
    pub variance_penalty: f64,      // confidence /= 1 + penalty·variance (4.0)
    pub cell_change_resistance: f64,// 0..1 blend toward the current table (0.2)
    pub max_delta_pct: f64,         // |per-cell change| clamp, % of current (15.0)
    pub lag_records: u32,           // wideband delay: afr[i] pairs with point[i-lag] (6)
    pub ego_center: f64,            // egoCorrection no-trim value; 0 disables (100.0)
    pub disabled_filters: Vec<String>, // Custom-filter ids to skip (empty)
}
impl Default for VeAnalyzeParams { /* the values above */ }

pub struct CellResult {
    pub current: f64, pub proposed: f64,
    pub delta_pct: f64,     // (proposed-current)/current·100; 0 when unchanged
    pub hit_weight: f64,    // summed bilinear weight (MLV "Total Weight")
    pub sample_count: u32,  // samples whose max-weight cell is this (MLV "Hit Count")
    pub confidence: f64,    // 0..1
}
pub struct FilterCount { pub id: String, pub label: String, pub count: u32 }
pub struct VeAnalysisReport {
    pub x_len: u32, pub y_len: u32,
    pub cells: Vec<CellResult>,     // len x_len·y_len, index y·x_len+x (pinned order)
    pub filtered: Vec<FilterCount>, // declaration order + built-ins first
    pub total_samples: u32, pub used_samples: u32,
}
pub enum AnalyzeError { MissingChannel(String), EmptyTable, ShapeMismatch(String) }

/// THE deterministic engine (design doc §3.1). Pure: same input → identical
/// output. No RNG, no HashMap iteration, fixed sample order, fixed cell order.
pub fn ve_analyze(
    samples: &SampleSet, ve: &TableGrid, target: &TableGrid,
    binding: &AnalyzeBinding, params: &VeAnalyzeParams,
) -> Result<VeAnalysisReport, AnalyzeError>;

// ── model: the cell-write seam (signature frozen; body Task 3) ──────────────
impl Tune {
    /// Set flat row-major cells of a named array constant. ONE undo `Edit`
    /// per call (a paste/smooth gesture is one undo step). Validates every
    /// index/value before touching any byte.
    pub fn set_cells(&mut self, name: &str, cells: &[(u32, f64)]) -> Result<(), ModelError>;
}

// ── src-tauri dto.rs (specta-safe: u32/f64 only) ────────────────────────────
pub struct CellEditDto { pub index: u32, pub value: f64 }   // + Deserialize (command input)
pub struct CaptureStatusDto { pub capturing: bool, pub sample_count: u32, pub duration_ms: f64, pub dropped: u32 }
pub struct CellResultDto { pub current: f64, pub proposed: f64, pub delta_pct: f64, pub hit_weight: f64, pub sample_count: u32, pub confidence: f64 }
pub struct FilterCountDto { pub id: String, pub label: String, pub count: u32 }
pub struct VeAnalysisReportDto {
    pub table: String, pub x_len: u32, pub y_len: u32,
    pub cells: Vec<CellResultDto>, pub filtered: Vec<FilterCountDto>,
    pub total_samples: u32, pub used_samples: u32,
}

// ── src-tauri owner.rs: five Command variants (handlers stubbed until their task) ──
SetCells { name: String, cells: Vec<CellEditDto>, reply: Reply<TuneDirtyEvent> }, // Task 3
StartCapture { reply: Reply<()> },                       // Task 8
StopCapture { reply: Reply<CaptureStatusDto> },          // Task 8
CaptureStatus { reply: Reply<CaptureStatusDto> },        // Task 8
RunVeAnalyze { table: String, reply: Reply<VeAnalysisReportDto> }, // Task 11
```

- [ ] **0.0 Bring in the M3 auto-tick fix.** `m4-table-editors` forked before
  `34097f3 fix(simulator): auto-tick engine on realtime requests` (on `m3-realtime-dashboard`).

Run: `git log --oneline --all | grep "auto-tick"` then `git cherry-pick 34097f3`
(or `git rebase m3-realtime-dashboard` if that branch has merged to main — prefer the
cherry-pick if unsure). Then `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-simulator`
Expected: PASS. Commit is the cherry-pick itself (keep its original message).

- [ ] **0.1 Grep every struct-literal site that will break.**

Run: `. "$HOME/.cargo/env" && cd src-tauri && grep -rn "TableDef {\|CurveDef {\|Definition {" --include="*.rs" . | grep -v target`
Expected sites: `crates/ini/src/ui_table_curve_parser.rs` (two literals),
`crates/ini/tests/ui.rs` + `crates/ini/tests/contract.rs` fixtures, and any `Definition {`
literal (`crates/ini/src/definition.rs` build site). Record the list.

- [ ] **0.2 Extend the `ini` types.** In `ui.rs`, add the new `TableDef`/`CurveDef` fields and
  `CurveAxis` (doc comments verbatim from the interface block; keep the existing
  `derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)`). Create `ve_analyze.rs`
  with `VeAnalyzeDef`/`VeAnalyzeMapDef`/`AnalyzeFilterDef`/`FilterOp` (same derives; module
  header: write-fresh port-note citing `speeduino.ini` GPL-3 grammar l.5984, per the
  `gauges_parser.rs` house style). Add `pub ve_analyze: Option<VeAnalyzeDef>` to `Definition`,
  set it to `None` in `parse_definition` (Task 2 fills it), add the `table()`/`curve()`
  accessors (mirror `constant()`), and re-export
  `pub use ve_analyze::{AnalyzeFilterDef, FilterOp, VeAnalyzeDef, VeAnalyzeMapDef};` +
  `pub use ui::CurveAxis;` in `lib.rs`.

- [ ] **0.3 Fix every literal from 0.1.** In `ui_table_curve_parser.rs` fill the new fields
  with defaults at the two construction sites (`map3d_id/title/help/x_channel/y_channel:
  String::new(), page: 0, xy_labels/grid_orient/up_down_label/column_labels/size: Vec::new(),
  grid_height: 0.0, x_axis/y_axis: None, gauge: String::new()`); same for test fixtures; add
  `ve_analyze: None` to every `Definition` literal.

- [ ] **0.4 Seed the `opentune-analysis` crate.** `crates/analysis/Cargo.toml`:

```toml
[package]
name = "opentune-analysis"
version = "0.1.0"
edition = "2021"
license = "GPL-3.0-or-later"

[dependencies]
# Deliberately empty: the deterministic core is pure (design doc §3.1).
```

Check `src-tauri/Cargo.toml` `[workspace] members` — if it lists crates explicitly, add
`"crates/analysis"`. `src/lib.rs`: SPDX header + the seam types verbatim (`SampleSet`,
`AnalyzeBinding`, `FilterSpec`, `FilterOp`, `VeAnalyzeParams` + `Default`, `CellResult`,
`FilterCount`, `VeAnalysisReport`, `AnalyzeError`) with `derive(Debug, Clone, PartialEq)`;
`src/grid.rs`: `TableGrid` with `lookup` stubbed to `None`; `ve_analyze` stubbed to
`Err(AnalyzeError::EmptyTable)` (compiles, pins the signature). Module header: write-fresh
note (no code port — TS/MLV proprietary; math from MS Extra manual + Speeduino
`[VeAnalyze]`, behavioral only).

- [ ] **0.5 Freeze the model seam.** Add `Tune::set_cells` to `tune.rs` with the doc comment
  from the interface block and body `todo!("M4 Task 3")`.

- [ ] **0.6 Freeze DTOs + owner variants.** Add the five DTO structs to `dto.rs`
  (`derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)`; `CellEditDto` also
  `serde::Deserialize` — it is command *input*). Add the five `Command` variants to
  `owner.rs` and stub `serve` arms that reply
  `Err("not implemented (M4)".to_string())` (send exactly one reply per arm, the M3 rule).
  Do **not** register any command yet (registration lands with each working command:
  Tasks 3/8/11).

- [ ] **0.7 Contract tests.** `crates/ini/tests/contract.rs`: extend the hand-built
  `Definition` with a `TableDef` carrying `x_channel: "rpm".into(), page: 2`, assert
  `def.table("veTable1Tbl").unwrap().x_channel == "rpm"` and a `CurveDef` with
  `x_axis: Some(CurveAxis { min: Number::Lit(-40.0), max: Number::Lit(215.0), divisions: 4 })`
  round-trips. `crates/analysis/tests/contract.rs` (new):

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
//! Signature-pinning contract tests for the M4-frozen analysis seams.
use opentune_analysis::*;

#[test]
fn seams_compile_and_default_params_are_pinned() {
    let p = VeAnalyzeParams::default();
    assert_eq!(
        (p.min_weight, p.confidence_sat_weight, p.variance_penalty),
        (1.0, 20.0, 4.0)
    );
    assert_eq!((p.cell_change_resistance, p.max_delta_pct), (0.2, 15.0));
    assert_eq!((p.lag_records, p.ego_center), (6, 100.0));
    // Pin the ve_analyze signature without invoking the stub body.
    let _: fn(&SampleSet, &TableGrid, &TableGrid, &AnalyzeBinding, &VeAnalyzeParams)
        -> Result<VeAnalysisReport, AnalyzeError> = ve_analyze;
    let s = SampleSet { columns: vec!["rpm".into()], t_ms: vec![0.0], rows: vec![vec![1000.0]] };
    assert_eq!(s.column("rpm"), Some(0));
    assert_eq!(s.len(), 1);
}
```

Pin `Tune::set_cells` in the existing model test module the same way:
`let _: fn(&mut Tune, &str, &[(u32, f64)]) -> Result<(), ModelError> = Tune::set_cells;`

- [ ] **0.8 Build + lint the workspace clean.**

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo build && cargo test && cargo clippy --workspace -- -D warnings`
Expected: PASS — every M2/M3 literal compiles with the new fields; the analysis crate
builds; owner stub arms compile.

- [ ] **0.9 Commit.**

```bash
git add -A
git commit -m "feat(ini,analysis,model,app): freeze M4 seams (table/curve/ve-analyze defs, analysis crate, set_cells, capture commands)"
```

---

## Task 1 — Real-INI ingestion: comms scatter + `lastOffset` aliasing + the vendored golden gate *(the M4 gate — before any editor UI)*

**Why here:** table editors that target the real INI can't load it today. Wall #1 fires first
(`parse_comms` → `MissingKey("ochGetCommand")` — 6 of its 9 required keys live in
`[Constants]`/`[OutputChannels]` in the real file); wall #2 corrupts page 5 (`lastOffset`
resolves to the running *end*, but TS semantics is the previous field's *start* — all 5 real
uses are AFR↔Lambda aliases; `afrTable` hard-errors and the scalar aliases land one byte
late *silently*). Both fixes are parser-local. The gate is the full, unmodified vendored file.

**Files:**
- Modify: `src-tauri/crates/ini/src/parser.rs` (scattered-key collection),
  `src-tauri/crates/ini/src/constants_fields.rs` (`lastOffset` = previous start),
  `src-tauri/crates/ini/src/constants_parser.rs` (skip known TS metadata keys)
- Create: `src-tauri/crates/ini/tests/fixtures/speeduino-real-0832dc1d.ini` (vendored, byte-identical),
  `src-tauri/crates/ini/tests/real_ini.rs`
- Test: extend `src-tauri/crates/ini/tests/parse_comms.rs`, `src-tauri/crates/ini/tests/constants.rs`

**Interfaces:**
- Consumes: `parse_comms`/`extract_comms_section`/`extract_och_get_command` (parser.rs),
  `resolve_offset`/`OffsetCounter` (constants_fields.rs), `parse_definition`.
- Produces: `parse_definition(&real_ini)` returns `Ok` with correct comms, aliasing, and
  **zero non-allowlisted diagnostics**. No public type changes.

> **Write fresh** (ADR-0006): hyper-tuner does not handle `lastOffset` or scattered comms
> keys; `speeduino.ini` @ `0832dc1d` (GPL-3) is the truth source. Record both fixes in
> `docs/notes/m4-decisions.md` as they land.

- [ ] **1.1 Failing test `comms_keys_scattered_into_constants_and_output_channels`** in
  `tests/parse_comms.rs` — a trimmed-but-real fragment (keys placed exactly as the real file
  places them, incl. per-page comma lists and quoted templates):

```rust
#[test]
fn comms_keys_scattered_into_constants_and_output_channels() {
    // Layout mirrors reference/speeduino.ini @ 0832dc1d l.4-10 + l.240-274 + l.5352-5353.
    let ini = r#"
[MegaTune]
   queryCommand   = "Q"
   signature      = "speeduino 202504-dev"
   versionInfo    = "S"

[Constants]
    pageSize            = 128,   288
    pageReadCommand     = "p%2i%2o%2c", "p%2i%2o%2c"
    pageValueWrite      = "M%2i%2o%2c%v", "M%2i%2o%2c%v"
    burnCommand         = "b%2i", "b%2i"
    blockingFactor      = 121
    blockReadTimeout    = 2000

page = 1
      reqFuel    = scalar, U16,  0, "ms",   0.1,  0.0,  0.0,  6553.5,  1

[OutputChannels]
  ochGetCommand    = "r\$tsCanId\x30%2o%2c"
  ochBlockSize     =  139
"#;
    let comms = opentune_ini::parse_comms(ini).expect("scattered keys must resolve");
    assert_eq!(comms.signature, "speeduino 202504-dev");
    assert_eq!(comms.page_read_command, "p%2i%2o%2c"); // first list element
    assert_eq!(comms.page_value_write, "M%2i%2o%2c%v");
    assert_eq!(comms.burn_command, "b%2i");
    assert_eq!(comms.blocking_factor, 121);
    assert_eq!(comms.block_read_timeout_ms, 2000);
    assert_eq!(comms.och_get_command, r"r\$tsCanId\x30%2o%2c");
    assert_eq!(comms.och_block_size, 139);
}
```

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-ini --test parse_comms`
Expected: FAIL with `MissingKey("ochGetCommand")` (or `pageReadCommand` — whichever
`require_string` hits first).

- [ ] **1.2 Implement scattered-key collection** in `parser.rs`. Add:

```rust
/// Comms keys the real speeduino.ini scatters into `[Constants]` (l.240-274 @
/// 0832dc1d) instead of `[MegaTune]`/`[TunerStudio]`. Values there may be
/// per-page comma lists (`"p%2i%2o%2c", "p%2i%2o%2c", ...`) — Speeduino uses
/// identical templates for every page, so the first element is taken and a
/// heterogeneous list is fine to ignore (recorded M4 decision).
const SCATTERED_COMMS_KEYS: &[&str] = &[
    "pageReadCommand", "pageValueWrite", "burnCommand", "blockingFactor",
    "blockReadTimeout", "interWriteDelay", "pageActivationDelay",
    "messageEnvelopeFormat",
];

/// First element of a possibly comma-separated value, honoring double quotes
/// (a comma inside `"..."` does not split). Returns the element verbatim
/// (quotes intact) so the existing `require_*` unquoting applies unchanged.
fn first_list_element(value: &str) -> &str {
    let mut in_quotes = false;
    for (i, ch) in value.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => return value[..i].trim(),
            _ => {}
        }
    }
    value.trim()
}

/// Collect allowlisted comms keys from `[Constants]` + `[OutputChannels]`,
/// appended AFTER the primary-section pairs so first-wins keeps the
/// `[MegaTune]`/`[TunerStudio]` value when both declare a key.
fn extract_scattered_comms(ini_text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_section = false;
    for raw in ini_text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_section = matches!(inner.trim(), "Constants" | "OutputChannels");
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        if SCATTERED_COMMS_KEYS.contains(&key) {
            out.push((key.to_string(), first_list_element(value).to_string()));
        }
    }
    out
}
```

In `parse_comms`, append the scattered pairs to the primary-section kv
(`let mut kv = extract_comms_section(ini_text); kv.extend(extract_scattered_comms(ini_text));`)
— check how `require_string` strips inline comments/quotes and mirror it if
`first_list_element`'s output needs pre-trimming — and make `ochGetCommand` fall back to the
existing `[OutputChannels]` scanner when the kv still lacks it:

```rust
    let och_get_command = require_string(&kv, "ochGetCommand")
        .or_else(|e| extract_och_get_command(ini_text).ok_or(e))?;
```

(The `parse_definition`-level override stays — `[OutputChannels]` still *wins* when both
exist; this fallback only stops the hard error when `[MegaTune]` has no `ochGetCommand` at
all.) Run 1.1 → GREEN; run the full `parse_comms` suite → all M1 fixtures still green
(first-wins).

- [ ] **1.3 Silence the metadata-key diagnostics** in `constants_parser.rs`: the real
  `[Constants]` header block also carries TS metadata that is neither a constant nor
  M4-parsed comms. Extend the existing `match key` skip arms with:

```rust
            // TunerStudio metadata / comms keys living in [Constants] (real
            // speeduino.ini l.240-274). Comms keys are consumed by
            // `parse_comms`' scattered scan; the rest are recorded-deferred
            // (m4-decisions) — neither is an unknown *constant*, so no
            // diagnostic and no page-counter poison.
            "pageIdentifier" | "pageReadCommand" | "pageValueWrite" | "burnCommand"
            | "blockingFactor" | "blockReadTimeout" | "interWriteDelay"
            | "pageActivationDelay" | "messageEnvelopeFormat" | "crc32CheckCommand"
            | "tableCrcCommand" | "pageChunkWrite" | "tsWriteBlocks"
            | "delayAfterPortOpen" | "readSdCompressed"
            | "restrictSquirtRelationship" => continue,
```

Add a test in `tests/constants.rs`: a `[Constants]` block containing
`pageReadCommand = "p%2i%2o%2c", "p%2i%2o%2c"` before `page = 1` parses with **zero**
diagnostics and an un-poisoned page counter (a following `lastOffset` constant still
resolves).

- [ ] **1.4 Failing test `last_offset_is_previous_field_start`** in `tests/constants.rs` —
  the page-5 shape verbatim from the real file:

```rust
#[test]
fn last_offset_is_previous_field_start() {
    // reference/speeduino.ini @ 0832dc1d l.640-645 + l.675-679: every real
    // `lastOffset` use ALIASES the immediately-preceding field (AFR↔Lambda
    // views over the same bytes). TS semantics: lastOffset = the previous
    // field's START offset, not the running end.
    let ini = r#"
[MegaTune]
   signature      = "test"
   queryCommand   = "Q"
   versionInfo    = "S"
   ochGetCommand  = "r"
   pageReadCommand = "p%2i%2o%2c"
   pageValueWrite = "M%2i%2o%2c%v"
   burnCommand    = "b%2i"
   blockingFactor = 121
   blockReadTimeout = 2000

[Constants]
    pageSize = 288

page = 1
      lambdaTable = array,  U08,          0, [16x16], "Lambda", 0.006, 0.0, 0.0, 2.0, 3
      afrTable    = array,  U08, lastOffset, [16x16], "AFR",    0.1,   0.0, 7.0, 25.5, 1
      rpmBinsAFR  = array,  U08, 256, [16], "RPM", 100.0, 0.0, 100.0, 25500.0, 0
      ego_min_afr    = scalar, U08, 272, "AFR", 0.1, 0.0, 7.0, 25.0, 1
      ego_min_lambda = scalar, U08, lastOffset, "Lambda", 0.006, 0.0, 0.0, 2.0, 3
"#;
    let def = opentune_ini::parse_definition(ini).expect("aliased page must parse");
    let lambda = def.constant("lambdaTable").unwrap();
    let afr = def.constant("afrTable").unwrap();
    assert_eq!((lambda.offset, afr.offset), (0, 0), "afrTable aliases lambdaTable");
    let e_afr = def.constant("ego_min_afr").unwrap();
    let e_lambda = def.constant("ego_min_lambda").unwrap();
    assert_eq!(e_lambda.offset, e_afr.offset, "scalar alias shares its byte");
    assert!(def.diagnostics.is_empty(), "no diagnostics: {:?}", def.diagnostics);
}
```

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-ini --test constants`
Expected: FAIL — today `afrTable` errors with `offset 256 + size 256 exceeds page 1 size 288`.

- [ ] **1.5 Implement previous-start semantics** in `constants_fields.rs`: at each successful
  per-class parse the counter currently advances to the *end*
  (`*running_offset = OffsetCounter::Known(offset + scalar_width(scalar_type));` at :294,
  and the array/bits/string equivalents at :398/:444/:490). Change all four sites to store
  the field's **start**:

```rust
    *running_offset = OffsetCounter::Known(offset);
```

Update the `OffsetCounter`/`resolve_offset` doc comments: "the running per-page byte
counter" → "the start offset of the previous successfully-parsed field on this page (TS
`lastOffset` semantics — every real use aliases the preceding field)". Poison semantics
unchanged (an unknown class still has an unknowable start). Then
`grep -rn "lastOffset" crates/ini/tests/` — if any M2 fixture/test pinned the old
end-semantics, correct the *expectation* with a comment citing the real-file truth source
(the M2 assumption was wrong; `Definition`'s shape is unchanged). Run 1.4 → GREEN; run the
full ini suite → GREEN.

- [ ] **1.6 Vendor the real INI** (byte-identical; provenance in the test header, not the file):

Run:
```bash
curl -sL "https://raw.githubusercontent.com/noisymime/speeduino/0832dc1d25b108cf33b30167284c44e3edd3d35a/reference/speeduino.ini" \
  -o src-tauri/crates/ini/tests/fixtures/speeduino-real-0832dc1d.ini
wc -l src-tauri/crates/ini/tests/fixtures/speeduino-real-0832dc1d.ini
```
Expected: `6026`.

- [ ] **1.7 The golden gate** — `tests/real_ini.rs`:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
//! M4 golden gate: the FULL, UNMODIFIED real speeduino.ini must parse with
//! zero non-allowlisted diagnostics.
//!
//! Fixture provenance: `reference/speeduino.ini` from noisymime/speeduino @
//! 0832dc1d25b108cf33b30167284c44e3edd3d35a (GPL-3.0, vendored byte-identical
//! — license-compatible with this GPL-3.0-or-later crate).
//!
//! Allowlist rule: every diagnostic must match a RECORDED-DEFERRED construct
//! below. A new, unexplained diagnostic is a parser gap — fix the parser or
//! record the deferral in docs/notes/m4-decisions.md; NEVER widen this list
//! silently.
use opentune_ini::parse_definition;

const REAL_INI: &str = include_str!("fixtures/speeduino-real-0832dc1d.ini");

/// Substrings identifying recorded-deferred constructs (M2/M4 decisions):
/// dialog widgets with no frozen representation, menu grouping, and any
/// further entries added ONLY together with an m4-decisions record.
const ALLOWED_DIAGNOSTICS: &[&str] = &["commandButton", "settingSelector", "groupMenu", "groupChildMenu"];

#[test]
fn real_speeduino_ini_parses_diagnostic_clean() {
    let def = parse_definition(REAL_INI).expect("real INI must parse");

    // Wall #1 closed: scattered comms fully resolved.
    assert_eq!(def.comms.signature, "speeduino 202504-dev");
    assert_eq!(def.comms.och_get_command, r"r\$tsCanId\x30%2o%2c");
    assert_eq!(def.comms.page_read_command, "p%2i%2o%2c");
    assert_eq!(def.comms.page_value_write, "M%2i%2o%2c%v");
    assert_eq!(def.comms.och_block_size, 139);
    assert!([121, 251].contains(&def.comms.blocking_factor));
    assert_eq!(def.pages.len(), 15);
    assert_eq!(def.pages[4].size, 288); // page 5

    // Wall #2 closed: all five lastOffset uses alias their predecessor.
    for (alias, original) in [
        ("afrTable", "lambdaTable"),
        ("ego_min_lambda", "ego_min_afr"),
        ("ego_max_lambda", "ego_max_afr"),
        ("afrProtectDeviation", "afrProtectDeviationLambda"),
        ("n2o_maxLambda", "n2o_maxAFR"),
    ] {
        let a = def.constant(alias).unwrap_or_else(|| panic!("missing {alias}"));
        let o = def.constant(original).unwrap_or_else(|| panic!("missing {original}"));
        assert_eq!((a.page, a.offset), (o.page, o.offset), "{alias} must alias {original}");
    }
    let afr = def.constant("afrTable").unwrap();
    assert_eq!((afr.page, afr.offset), (5, 0));

    // The M4 payload sections exist (parsed fully in Task 2; here just present).
    assert!(def.output_channels.len() > 100, "got {}", def.output_channels.len());
    assert!(!def.tables.is_empty() && !def.curves.is_empty());
    assert!(!def.gauges.is_empty());

    // Diagnostic-clean modulo the recorded allowlist.
    let unexpected: Vec<_> = def
        .diagnostics
        .iter()
        .filter(|d| !ALLOWED_DIAGNOSTICS.iter().any(|p| d.detail.contains(p)))
        .collect();
    assert!(unexpected.is_empty(), "unexplained diagnostics:\n{unexpected:#?}");
}
```

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-ini --test real_ini -- --nocapture`
Expected: likely still RED on first run — this is the *gate*, not a formality. Triage rule
(explicit, per failure class): a hard `Err` from `parse_definition` = a parser bug in a
section we model → fix it in this task (keep each fix small + individually tested); an
unexplained `Diagnostic` = either a grammar gap in a modeled section (fix) or a genuinely
new deferred construct (add the substring to `ALLOWED_DIAGNOSTICS` **and** a matching entry
in `docs/notes/m4-decisions.md` in the same commit). Iterate until GREEN. Do not weaken the
non-diagnostic assertions.

- [ ] **1.8 Full ini suite + workspace.**

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-ini && cargo test && cargo clippy --workspace -- -D warnings`
Expected: PASS.

- [ ] **1.9 Commit (two commits, matching the two walls + the gate).**

```bash
git add src-tauri/crates/ini/src/parser.rs src-tauri/crates/ini/src/constants_parser.rs \
  src-tauri/crates/ini/src/constants_fields.rs src-tauri/crates/ini/tests/parse_comms.rs \
  src-tauri/crates/ini/tests/constants.rs
git commit -m "fix(ini): real-INI walls — scattered comms keys + lastOffset aliases previous field start"
git add -A
git commit -m "test(ini): vendored real speeduino.ini golden gate (0832dc1d, diagnostic-clean)"
```

---

## Task 2 — `ini`: full `[TableEditor]`/`[CurveEditor]` grammar + `[VeAnalyze]` parser; DTO projection *(roadmap: table/curve editors, INI half)*

**Files:**
- Modify: `src-tauri/crates/ini/src/ui_table_curve_parser.rs` (capture everything the M2
  parser drops), `src-tauri/crates/ini/src/definition.rs` (wire `parse_ve_analyze`)
- Create: `src-tauri/crates/ini/src/ve_analyze_parser.rs`
- Modify: `src-tauri/src/dto.rs` (extend `TableDto`, add `CurveDto`/`AxisDto`, `DefinitionDto.curves`), `src-tauri/src/lib.rs` (needles)
- Test: `src-tauri/crates/ini/tests/ui.rs` (extend), `src-tauri/crates/ini/tests/ve_analyze.rs` (new), `src-tauri/crates/ini/tests/real_ini.rs` (extend the golden gate)

**Interfaces:**
- Consumes: the Task 0 `TableDef`/`CurveDef`/`CurveAxis`/`VeAnalyzeDef` types,
  `ui_tokens::split_tokens`/`unquote`, `constants_fields::{split_fields, parse_number}`
  (pub(crate)), the preprocessor (already run by `parse_definition` — `#else` branches win).
- Produces: `parse_definition` fills the richer `tables`/`curves` + `ve_analyze`;
  `DefinitionDto` carries `tables` (extended) + `curves`.

> **Confirmed (research):** `[TableEditor]`/`[CurveEditor]` richer shape **PORTS** from
> `hyper-tuner/ini` + `hyper-tuner/types` (MIT © Piotr Rogowski —
> `parseTables`/`parseCurves`, `config.ts:153-176`; keep the copyright line in the module
> header, M3 license lesson). **First sub-step confirms** the source covers each attribute;
> the axis-display channel (2nd `xBins` token) is captured-but-unused there — capturing it
> into `x_channel` is our extension (record). `[VeAnalyze]` is **WRITE FRESH** (hyper-tuner
> stops at `[Datalog]`; grammar truth source: `speeduino.ini` l.5984-6010 @ 0832dc1d).

- [ ] **2.1 Confirm the port source** (ADR-0006 first sub-step): open hyper-tuner
  `src/ini.ts` `parseTables`/`parseCurves` + `types/src/types/config.ts:153-176`; confirm the
  field set (`map, title, page, help?, xBins[], yBins[], zBins[], xyLabels[], gridHeight,
  gridOrient[], upDownLabel[]`; `Curve { title, labels[], xAxis[], yAxis[], xBins[],
  yBins[], size[], gauge? }`). Update the `ui_table_curve_parser.rs` module header: extend
  the existing port-note with the fuller field set + `© 2021 Piotr Rogowski` + the
  `x_channel`/`y_channel` capture as our recorded extension.

- [ ] **2.2 Failing test `parses_full_table_and_curve_attributes`** in `tests/ui.rs` — real
  grammar verbatim (from `speeduino.ini` l.4935-4948 + l.4621-4630):

```rust
#[test]
fn parses_full_table_and_curve_attributes() {
    let ini = r#"
[Constants]
    pageSize = 288, 288

page = 2
      veTable    = array,  U08,   0, [16x16], "%",   1.0, 0.0, 0.0, 255.0, 0
      rpmBins    = array,  U08, 256, [16],  "RPM", 100.0, 0.0, 100.0, 25500.0, 0
      fuelLoadBins = array, U08, 272, [16], "kPa", 1.0, 0.0, 0.0, 511.0, 0
page = 1
      taeBins    = array,  U08,   0, [4], "%/s", 10.0, 0.0, 0.0, 2550.0, 0
      taeRates   = array,  U08,   4, [4], "%",    1.0, 0.0, 0.0, 255.0, 0

[TableEditor]
   table = veTable1Tbl,  veTable1Map,  "VE Table",   2
      topicHelp   = "http://wiki.speeduino.com/en/configuration/VE_table"
      xBins       = rpmBins,  rpm
      yBins       = fuelLoadBins, fuelLoad
      xyLabels    = "RPM", "Fuel Load: "
      zBins       = veTable
      gridHeight  = 2.0
      gridOrient  = 250,   0, 340
      upDownLabel = "(RICHER)", "(LEANER)"

[CurveEditor]
      curve = time_accel_tpsdot_curve, "TPS based AE"
            columnLabel = "TPSdot", "Added"
            xAxis = 0, 1200, 6
            yAxis = 0, 250, 4
            xBins = taeBins, TPSdot
            yBins = taeRates
            size  = 400, 400
"#;
    let def = opentune_ini::parse_definition(&format!("{COMMS_HEADER}{ini}"))
        .expect("parses");
    let t = def.table("veTable1Tbl").expect("table by id");
    assert_eq!(t.map3d_id, "veTable1Map");
    assert_eq!(t.title, "VE Table");
    assert_eq!(t.page, 2);
    assert_eq!((t.x_bins.as_str(), t.x_channel.as_str()), ("rpmBins", "rpm"));
    assert_eq!((t.y_bins.as_str(), t.y_channel.as_str()), ("fuelLoadBins", "fuelLoad"));
    assert_eq!(t.z, "veTable");
    assert_eq!(t.xy_labels, vec!["RPM", "Fuel Load: "]);
    assert!((t.grid_height - 2.0).abs() < 1e-9);
    assert_eq!(t.grid_orient, vec![250.0, 0.0, 340.0]);
    assert_eq!(t.up_down_label, vec!["(RICHER)", "(LEANER)"]);
    assert_eq!(t.help, "http://wiki.speeduino.com/en/configuration/VE_table");
    let c = def.curve("time_accel_tpsdot_curve").expect("curve by id");
    assert_eq!(c.title, "TPS based AE");
    assert_eq!(c.column_labels, vec!["TPSdot", "Added"]);
    let x = c.x_axis.as_ref().expect("xAxis");
    assert_eq!((x.min.clone(), x.max.clone(), x.divisions),
        (Number::Lit(0.0), Number::Lit(1200.0), 6));
    assert_eq!((c.x_bins.as_str(), c.x_channel.as_str()), ("taeBins", "TPSdot"));
    assert_eq!(c.y_bins, "taeRates");
    assert_eq!(c.size, vec![400.0, 400.0]);
}
```

(`COMMS_HEADER` = the minimal `[MegaTune]` block the existing `tests/ui.rs` fixtures use —
reuse whatever helper/constant that file already has for it, or lift the 10-line `[MegaTune]`
block from Task 1.4's test.)

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-ini --test ui`
Expected: FAIL (new fields empty).

- [ ] **2.3 Implement the richer table/curve capture** in `ui_table_curve_parser.rs`:

```rust
// In parse_table_line's "table" arm — capture the full header:
        "table" => {
            // table = table_id, map3d_id, "title", page
            let tokens = split_tokens(value);
            let Some(name) = tokens.first() else {
                return;
            };
            tables.push(TableDef {
                name: name.clone(),
                map3d_id: tokens.get(1).cloned().unwrap_or_default(),
                title: tokens.get(2).map(|t| unquote(t)).unwrap_or_default(),
                page: tokens
                    .get(3)
                    .and_then(|t| t.trim().parse::<u32>().ok())
                    .unwrap_or(0),
                x_bins: String::new(),
                x_channel: String::new(),
                y_bins: String::new(),
                y_channel: String::new(),
                z: String::new(),
                xy_labels: Vec::new(),
                grid_height: 0.0,
                grid_orient: Vec::new(),
                up_down_label: Vec::new(),
                help: String::new(),
            });
        }
        "xBins" => set_table_bin(value, tables, constants, diagnostics, TableBin::X),
        "yBins" => set_table_bin(value, tables, constants, diagnostics, TableBin::Y),
        "zBins" => set_table_bin(value, tables, constants, diagnostics, TableBin::Z),
        "topicHelp" => set_table_attr(tables, |t| t.help = unquote(value)),
        "xyLabels" => set_table_attr(tables, |t| t.xy_labels = quoted_list(value)),
        "gridHeight" => set_table_attr(tables, |t| {
            t.grid_height = value.trim().parse::<f64>().unwrap_or(0.0);
        }),
        "gridOrient" => set_table_attr(tables, |t| t.grid_orient = float_list(value)),
        "upDownLabel" => set_table_attr(tables, |t| t.up_down_label = quoted_list(value)),
        _ => {}
```

with three small helpers in the same file (keep it <400 lines — if it crosses, split the
helpers into `ui_table_curve_attrs.rs`):

```rust
/// Apply an attribute to the most recently declared table (attributes always
/// follow their `table =` header). No table yet ⇒ silently ignore (the M2
/// graceful rule; the header itself already diagnosed if malformed).
fn set_table_attr(tables: &mut [TableDef], apply: impl FnOnce(&mut TableDef)) {
    if let Some(t) = tables.last_mut() {
        apply(t);
    }
}

/// `"RPM", "Fuel Load: "` → unquoted strings, order preserved.
fn quoted_list(value: &str) -> Vec<String> {
    split_tokens(value).iter().map(|t| unquote(t)).collect()
}

/// `250, 0, 340` → floats; unparseable tokens are skipped (graceful).
fn float_list(value: &str) -> Vec<f64> {
    split_tokens(value)
        .iter()
        .filter_map(|t| t.trim().parse::<f64>().ok())
        .collect()
}
```

Extend `set_table_bin` to also capture the display channel (2nd token) for X/Y:

```rust
    match which {
        TableBin::X => {
            table.x_bins = name.clone();
            table.x_channel = tokens.get(1).cloned().unwrap_or_default();
        }
        TableBin::Y => {
            table.y_bins = name.clone();
            table.y_channel = tokens.get(1).cloned().unwrap_or_default();
        }
        TableBin::Z => table.z = name.clone(),
    }
```

Mirror for curves in `parse_curve_line`: the `"curve"` arm fills
`title: tokens.get(1).map(|t| unquote(t)).unwrap_or_default()` plus defaults; add arms
`"columnLabel" => ... c.column_labels = quoted_list(value)`,
`"xAxis"`/`"yAxis"` parse `min, max, divisions` via `split_tokens` +
`constants_fields::parse_number` for min/max and `u32` for divisions
(`c.x_axis = parse_curve_axis(value)`):

```rust
/// `xAxis = -40, 215, 4` — min/max may be `{ expr }` (Number), divisions u32.
fn parse_curve_axis(value: &str) -> Option<CurveAxis> {
    let tokens = split_tokens(value);
    Some(CurveAxis {
        min: parse_number(tokens.first()?),
        max: parse_number(tokens.get(1)?),
        divisions: tokens.get(2).and_then(|t| t.trim().parse().ok()).unwrap_or(0),
    })
}
```

`"gauge" => set_curve_attr(curves, |c| c.gauge = split_tokens(value).first().cloned().unwrap_or_default())`,
`"size" => set_curve_attr(curves, |c| c.size = float_list(value))` (add a `set_curve_attr`
twin of `set_table_attr`); `set_curve_bin` captures `x_channel` from the 2nd `xBins` token
(curves' `yBins` has no channel). `parse_number` is `pub(crate)` in `constants_fields.rs` —
import it. Run 2.2 → GREEN, then the whole ui suite (the M2 tests assert the old fields —
they must stay green untouched).

- [ ] **2.4 Failing test `parses_ve_analyze_binding`** in new `tests/ve_analyze.rs` — the real
  block verbatim (l.5984-6009), exercising the `#else` branch:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
//! [VeAnalyze] parser tests. Grammar truth source: reference/speeduino.ini
//! @ 0832dc1d l.5984-6010 (GPL-3, quoted verbatim below). WRITE FRESH per
//! ADR-0006 (hyper-tuner does not parse this section).
use opentune_ini::{parse_definition, AnalyzeFilterDef, FilterOp};

#[test]
fn parses_ve_analyze_binding() {
    let ini = r#"
[MegaTune]
   signature      = "test"
   queryCommand   = "Q"
   versionInfo    = "S"
   ochGetCommand  = "r"
   pageReadCommand = "p%2i%2o%2c"
   pageValueWrite = "M%2i%2o%2c%v"
   burnCommand    = "b%2i"
   blockingFactor = 121
   blockReadTimeout = 2000

[VeAnalyze]
#if LAMBDA
     veAnalyzeMap = veTable1Tbl, lambdaTable1Tbl, lambda, egoCorrection
     lambdaTargetTables = lambdaTable1Tbl, afrTSCustom
#else
     veAnalyzeMap = veTable1Tbl, afrTable1Tbl, afr, egoCorrection
     lambdaTargetTables = afrTable1Tbl, afrTSCustom
#endif
         filter = std_xAxisMin ; Auto build
         filter = std_xAxisMax ; Auto build
         filter = std_DeadLambda ; Auto build
#if CELSIUS
         filter = minCltFilter, "Minimum CLT", coolant,       <       , 71,       , true
#else
         filter = minCltFilter, "Minimum CLT", coolant,       <       , 160,      , true
#endif
         filter = accelFilter, "Accel Flag" , engine,         &       , 16,       , false
         filter = overrunFilter, "Overrun"    , pulseWidth,  =       , 0,        , false
         filter = std_Custom ; Standard Custom Expression Filter.
"#;
    let def = parse_definition(ini).expect("parses");
    let va = def.ve_analyze.as_ref().expect("[VeAnalyze] parsed");
    assert_eq!(va.maps.len(), 1, "#else branch only");
    let m = &va.maps[0];
    assert_eq!(
        (m.table.as_str(), m.target_table.as_str(), m.lambda_channel.as_str(), m.ego_channel.as_str()),
        ("veTable1Tbl", "afrTable1Tbl", "afr", "egoCorrection")
    );
    assert_eq!(va.filters.len(), 7);
    assert!(matches!(&va.filters[0], AnalyzeFilterDef::Std(s) if s == "std_xAxisMin"));
    match &va.filters[3] {
        AnalyzeFilterDef::Custom { id, label, channel, op, value, default_on } => {
            assert_eq!(id, "minCltFilter");
            assert_eq!(label, "Minimum CLT");
            assert_eq!(channel, "coolant");
            assert_eq!(*op, FilterOp::Lt);
            assert!((value - 160.0).abs() < 1e-9, "#else branch value");
            assert!(default_on);
        }
        other => panic!("expected Custom, got {other:?}"),
    }
    match &va.filters[4] {
        AnalyzeFilterDef::Custom { op, value, default_on, .. } => {
            assert_eq!(*op, FilterOp::And);
            assert!((value - 16.0).abs() < 1e-9);
            assert!(!default_on);
        }
        other => panic!("expected Custom, got {other:?}"),
    }
    match &va.filters[5] {
        AnalyzeFilterDef::Custom { op, .. } => assert_eq!(*op, FilterOp::Eq),
        other => panic!("expected Custom, got {other:?}"),
    }
}
```

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-ini --test ve_analyze`
Expected: FAIL (`ve_analyze` is `None`).

- [ ] **2.5 Implement `parse_ve_analyze`** in new `ve_analyze_parser.rs` (SPDX header +
  write-fresh port-note):

```rust
pub(crate) struct ParsedVeAnalyze {
    pub def: Option<VeAnalyzeDef>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Parse the `[VeAnalyze]` section from preprocessed INI text. `[WueAnalyze]`
/// and `lambdaTargetTables` are recorded-deferred (silently skipped —
/// m4-decisions). Malformed rows degrade to a `Diagnostic`, never an error.
pub(crate) fn parse_ve_analyze(ini_text: &str) -> ParsedVeAnalyze {
    let mut maps = Vec::new();
    let mut filters = Vec::new();
    let mut diagnostics = Vec::new();
    let mut in_section = false;
    for raw in ini_text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_section = inner.trim() == "VeAnalyze";
            continue;
        }
        if !in_section {
            continue;
        }
        let line = strip_inline_comment(line);
        let Some((key, value)) = line.split_once('=') else { continue };
        match key.trim() {
            "veAnalyzeMap" => {
                let f = split_fields(value.trim());
                if f.len() >= 4 {
                    maps.push(VeAnalyzeMapDef {
                        table: f[0].clone(),
                        target_table: f[1].clone(),
                        lambda_channel: f[2].clone(),
                        ego_channel: f[3].clone(),
                    });
                } else {
                    diagnostics.push(Diagnostic {
                        section: "VeAnalyze".to_string(),
                        detail: format!("malformed veAnalyzeMap: `{value}`"),
                    });
                }
            }
            "lambdaTargetTables" => {} // recorded-deferred (m4-decisions)
            "filter" => match parse_filter(value.trim()) {
                Some(f) => filters.push(f),
                None => diagnostics.push(Diagnostic {
                    section: "VeAnalyze".to_string(),
                    detail: format!("malformed filter: `{value}`"),
                }),
            },
            other => diagnostics.push(Diagnostic {
                section: "VeAnalyze".to_string(),
                detail: format!("unrecognised key `{other}`"),
            }),
        }
    }
    let def = if maps.is_empty() && filters.is_empty() {
        None
    } else {
        Some(VeAnalyzeDef { maps, filters })
    };
    ParsedVeAnalyze { def, diagnostics }
}

/// `std_*` → Std; else `id, "label", channel, op, value, [reserved], [bool]`.
fn parse_filter(value: &str) -> Option<AnalyzeFilterDef> {
    let f = split_fields(value);
    let first = f.first()?;
    if f.len() == 1 && first.starts_with("std_") {
        return Some(AnalyzeFilterDef::Std(first.clone()));
    }
    if f.len() < 5 {
        return None;
    }
    let op = match f[3].trim() {
        "<" => FilterOp::Lt,
        ">" => FilterOp::Gt,
        "=" => FilterOp::Eq,
        "&" => FilterOp::And,
        _ => return None,
    };
    Some(AnalyzeFilterDef::Custom {
        id: f[0].clone(),
        label: unquote(&f[1]),
        channel: f[2].clone(),
        op,
        value: f[4].trim().parse::<f64>().ok()?,
        default_on: f.get(6).map(|b| b.trim() == "true").unwrap_or(true),
    })
}
```

(Reuse `constants_fields::{split_fields, unquote}` and the brace-aware
`output_channels_parser::strip_inline_comment` — all `pub(crate)`. If `split_fields`
already unquotes, drop the extra `unquote` — mirror what `gauges_parser.rs` does with the
same helpers.) Wire in `definition.rs`: `let va = parse_ve_analyze(&preprocessed);`,
`diagnostics.extend(va.diagnostics);`, `ve_analyze: va.def`. Run 2.4 → GREEN.

- [ ] **2.6 Extend the golden gate** (`tests/real_ini.rs`) with the Task 2 payload — append
  to the existing test:

```rust
    // Task 2: full table/curve grammar against the real file.
    let ve = def.table("veTable1Tbl").expect("veTable1Tbl");
    assert_eq!(ve.page, 2);
    assert_eq!((ve.x_channel.as_str(), ve.y_channel.as_str()), ("rpm", "fuelLoad"));
    assert_eq!(ve.title, "VE Table");
    assert_eq!(ve.grid_orient, vec![250.0, 0.0, 340.0]);
    assert_eq!(ve.z, "veTable");
    let dwell = def.curve("dwell_correction_curve").expect("dwell curve");
    assert!(!dwell.title.is_empty());
    assert!(dwell.x_axis.is_some() && dwell.y_axis.is_some());
    // Task 2: [VeAnalyze] binding (the #else / AFR branch wins by default).
    let va = def.ve_analyze.as_ref().expect("[VeAnalyze]");
    assert_eq!(va.maps[0].table, "veTable1Tbl");
    assert_eq!(va.maps[0].lambda_channel, "afr");
    assert!(va.filters.len() >= 9, "got {}", va.filters.len());
```

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-ini`
Expected: PASS (triage any new real-file surprise per the 1.7 rule).

- [ ] **2.7 Project the new shape over IPC** (`dto.rs`): extend `TableDto` and add curves —

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct TableDto {
    pub name: String,
    pub title: String,
    pub page: u32,
    pub x_bins: String,
    /// Output channel driving the live X cursor ("" when the INI names none).
    pub x_channel: String,
    pub y_bins: String,
    pub y_channel: String,
    pub z: String,
    pub xy_labels: Vec<String>,
    pub up_down_label: Vec<String>,
    pub help: String,
}

/// Curve axis bounds when literal (an `{expr}` bound resolves to `None` —
/// the frontend falls back to data extents).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct AxisDto { pub min: Option<f64>, pub max: Option<f64>, pub divisions: u32 }

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct CurveDto {
    pub name: String,
    pub title: String,
    pub column_labels: Vec<String>,
    pub x_axis: Option<AxisDto>,
    pub y_axis: Option<AxisDto>,
    pub x_bins: String,
    pub x_channel: String,
    pub y_bins: String,
    pub gauge: String,
}
```

Update `impl From<&TableDef> for TableDto` (new fields; `grid_height`/`grid_orient`/
`map3d_id` are deliberately **not** projected — unused by the frontend, recorded); add
`From<&CurveDef> for CurveDto` (axis via the `lit()` helper on `Number`); add
`pub curves: Vec<CurveDto>` to `DefinitionDto` + the `From<&Definition>` line
(`curves: def.curves.iter().map(CurveDto::from).collect(),`). Add needles `"CurveDto"`,
`"AxisDto"`, `"x_channel"` to the `binding_gen` needle test in `src-tauri/src/lib.rs`.

- [ ] **2.8 Run everything** (bindings regenerate via the binding_gen test).

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test && cargo clippy --workspace -- -D warnings && cd .. && npm run build`
Expected: PASS — `src/ipc/bindings.ts` now carries `CurveDto`/`x_channel`; `tsc` still
compiles (`TableField.tsx` uses only old fields — additive change).

- [ ] **2.9 Commit.**

```bash
git add -A
git commit -m "feat(ini): full [TableEditor]/[CurveEditor] grammar (port hyper-tuner shape) + [VeAnalyze] parser; project tables/curves over IPC"
```

---

## Task 3 — `model`/`session`/`app`: `set_cells` — per-gesture cell writes *(the editor's wire path)*

**Files:**
- Modify: `src-tauri/crates/model/src/tune.rs` (fill the frozen `set_cells`),
  `src-tauri/src/session.rs` (`Session::set_cells`),
  `src-tauri/src/owner.rs` (fill the `SetCells` arm),
  `src-tauri/src/tune_commands.rs` (`set_cells` command),
  `src-tauri/src/lib.rs` (register + needles)
- Test: `tune.rs` unit tests, `session.rs` test module

**Interfaces:**
- Consumes: `Tune::{get, set}` (whole-array validate/encode), `page_deltas`/`write_deltas`
  (session.rs — already compute the **minimal contiguous changed span**, so a cell edit
  writes only its bytes on the wire), `Command`/`request` (owner pattern), `CellEditDto`
  (Task 0).
- Produces: `commands.setCells(name, cells)` — one gesture = one command = one undo `Edit`
  = one minimal wire span. Emits `TuneDirtyEvent` like `setValue`.

> **Confirmed (research): WRITE FRESH.** The dossier's write-amplification fear is already
> half-solved: `page_deltas` (session.rs:251) diffs whole pages into one contiguous span, so
> the wire cost of a cell edit is minimal *today*; what M4 adds is the **gesture-shaped
> command** (multi-cell edits as one validated call + one undo step) via decode-modify-set
> — reusing `Tune::set`'s validation/dirty/undo verbatim instead of a parallel byte path.

- [ ] **3.1 Failing model tests** in `tune.rs`'s test module (reuse its existing
  `Definition`-fixture helper that the M2 `set`/`undo` tests use — same page/constant setup):

```rust
#[test]
fn set_cells_edits_cells_and_undoes_as_one_step() {
    let mut tune = test_tune(); // the module's existing array-constant fixture helper
    let Value::Array(before) = tune.get("veTable").unwrap() else { panic!() };
    tune.set_cells("veTable", &[(0, 55.0), (17, 60.0)]).unwrap();
    let Value::Array(after) = tune.get("veTable").unwrap() else { panic!() };
    assert_eq!(after[0], 55.0);
    assert_eq!(after[17], 60.0);
    assert_eq!(after[1], before[1], "untouched cells intact");
    assert!(tune.is_dirty());
    assert!(tune.undo(), "one gesture = one undo step");
    assert_eq!(tune.get("veTable").unwrap(), Value::Array(before));
    assert!(!tune.undo() || tune.get("veTable").unwrap() != Value::Array(vec![]));
}

#[test]
fn set_cells_rejects_out_of_bounds_and_non_array_untouched() {
    let mut tune = test_tune();
    let before = tune.get("veTable").unwrap();
    assert!(tune.set_cells("veTable", &[(9999, 1.0)]).is_err());
    assert_eq!(tune.get("veTable").unwrap(), before, "failed call touches nothing");
    assert!(tune.set_cells("reqFuel", &[(0, 1.0)]).is_err(), "scalar is not an array");
    assert!(tune.set_cells("veTable", &[]).is_ok(), "empty gesture is a no-op");
}
```

(If the module's fixture has no 2-D array constant, extend the fixture `Definition` with one
— `veTable = array, U08, [16x16]`-shaped — the same way the M2 array-codec tests build
theirs.) Run → RED (`todo!` panics).

- [ ] **3.2 Implement `Tune::set_cells`** (replacing the Task 0 `todo!`):

```rust
    pub fn set_cells(&mut self, name: &str, cells: &[(u32, f64)]) -> Result<(), ModelError> {
        if cells.is_empty() {
            return Ok(());
        }
        let Value::Array(mut xs) = self.get(name)? else {
            return Err(ModelError::TypeMismatch(format!("`{name}` is not an array")));
        };
        for (index, value) in cells {
            let i = *index as usize;
            if i >= xs.len() {
                return Err(ModelError::TypeMismatch(format!(
                    "`{name}`: cell index {i} out of bounds ({} elements)",
                    xs.len()
                )));
            }
            xs[i] = *value;
        }
        // Re-encode through `set`: shares range-checking (per element, lo/hi),
        // dirty tracking, and records exactly ONE undo Edit for the gesture.
        self.set(name, Value::Array(xs))
    }
```

Run 3.1 → GREEN. (Range violations come back as `ModelError::OutOfRange` from
`encode_scalar` — add one assertion for a value above the constant's `high` if the fixture
has literal bounds.)

- [ ] **3.3 `Session::set_cells`** — mirror `set_value` (session.rs:130-147) exactly:

```rust
    /// Write individual table cells live: validate on a clone, push only the
    /// changed byte span to the ECU, then commit. One call = one undo step.
    pub fn set_cells(&mut self, name: &str, cells: &[(u32, f64)]) -> Result<TuneDirtyEvent, String> {
        let Session { conn, def, tune, .. } = self;
        let tune = tune.as_mut().ok_or_else(|| NO_TUNE.to_string())?;

        let mut probe = tune.clone();
        probe.set_cells(name, cells).map_err(fmt_model_err)?;
        let deltas = page_deltas(tune, &probe, &def.pages);

        write_deltas(conn, &def.comms, &deltas)?;

        tune.set_cells(name, cells).map_err(fmt_model_err)?;
        Ok(dirty_event(tune))
    }
```

Add a test to session.rs's existing simulator-backed test module (mirror its `set_value`
test setup): edit two **adjacent** cells and one far cell of an array constant via
`set_cells`, then assert (a) the session tune reflects them, (b) `page_deltas` between the
pre-call tune clone and the post-call tune spans exactly `first_changed..=last_changed`
bytes (one span — the far cell stretches it; that is the documented contiguous-span
trade-off, bytes between are rewritten with identical values). Run → GREEN.

- [ ] **3.4 Owner arm + command + registration.** Replace the Task 0 stub arm in `owner.rs`
  with the `SetValue` arm's exact shape (with_session + emit_dirty + single reply):

```rust
            Command::SetCells { name, cells, reply } => {
                let r = self
                    .with_session(move |s| {
                        let cells: Vec<(u32, f64)> =
                            cells.iter().map(|c| (c.index, c.value)).collect();
                        s.set_cells(&name, &cells)
                    })
                    .await;
                self.emit_dirty(&r);
                let _ = reply.send(r);
            }
```

(First confirm `with_session`'s exact closure signature against the existing `SetValue` arm
and match it.) New command in `tune_commands.rs`:

```rust
/// Write individual cells of an array constant (a table-editor gesture).
#[tauri::command]
#[specta::specta]
pub async fn set_cells(
    name: String,
    cells: Vec<CellEditDto>,
    owner: State<'_, OwnerHandle>,
) -> Result<(), String> {
    request(&owner, |reply| Command::SetCells { name, cells, reply })
        .await
        .map(|_| ())
}
```

Register `tune_commands::set_cells` in `collect_commands!` and add needles `"setCells"` +
`"CellEditDto"` to the binding_gen needle test.

- [ ] **3.5 Run everything.**

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test && cargo clippy --workspace -- -D warnings`
Expected: PASS; regenerated `bindings.ts` contains `setCells`.

- [ ] **3.6 Commit.**

```bash
git add -A
git commit -m "feat(model,app): set_cells — per-gesture table cell writes (one undo step, minimal wire span)"
```

---

## Task 4 — Frontend table-editing core: selection, ops, TSV, heatmap *(pure functions, vitest-first)*

**Files:**
- Create: `src/components/table-editor/selection.ts`, `src/components/table-editor/tableOps.ts`,
  `src/components/table-editor/tsv.ts`, `src/components/table-editor/heatmap.ts`
- Test: `src/components/table-editor/selection.test.ts`, `tableOps.test.ts`, `tsv.test.ts`, `heatmap.test.ts`

**Interfaces:**
- Consumes: nothing (pure TS — no DOM, no store, no IPC).
- Produces (used verbatim by Tasks 5/6/7/11):

```ts
// selection.ts
export type Cell = { row: number; col: number };
export type Selection = { anchor: Cell; focus: Cell };  // rectangle corners
export type Rect = { r0: number; c0: number; r1: number; c1: number }; // normalized, inclusive
export function rectOf(sel: Selection): Rect;
export function clampCell(cell: Cell, rows: number, cols: number): Cell;
export function move(sel: Selection, dr: number, dc: number, rows: number, cols: number, extend: boolean): Selection;
export function cellIndices(rect: Rect, cols: number): number[]; // row-major flat indices

// tableOps.ts
export type Grid = { rows: number; cols: number; values: number[] }; // row-major [r*cols+c]
export type CellEdit = { index: number; value: number };
export function interpolateRect(g: Grid, rect: Rect): CellEdit[];
export function smoothRect(g: Grid, rect: Rect): CellEdit[];
export function scaleRect(g: Grid, rect: Rect, factor: number): CellEdit[];
export function setEqualRect(g: Grid, rect: Rect, value?: number): CellEdit[];
export function stepRect(g: Grid, rect: Rect, delta: number): CellEdit[];

// tsv.ts
export function toTsv(g: Grid, rect: Rect, digits: number): string;
export function parseTsv(text: string): number[][] | null; // null on any non-numeric cell
export function pasteEdits(g: Grid, at: Cell, data: number[][]): CellEdit[]; // clipped to grid

// heatmap.ts
export function heatT(value: number, lo: number, hi: number): number; // clamped 0..1
export function heatColor(value: number, lo: number, hi: number): string; // css hsl() string
export function heatRgb(value: number, lo: number, hi: number): [number, number, number]; // 0..1 each, for three.js vertex colors
```

> **WRITE FRESH** (ADR-0006): hypertuner-cloud (MIT) is a read-only viewer — no selection,
> ops, or clipboard to lift; LibreTune (GPL-2) is study-only. Interaction semantics follow
> the TS/LibreTune behavioral spec from the dossier §C. Semantics pinned here: *interpolate*
> = corner-anchored bilinear (the four rect corners stay, everything else in the rect is
> recomputed; 1×N/N×1 degenerates to linear; a single-cell rect is a no-op);
> *smooth* = one pass of a 3×3 kernel (center 4, edge 2, corner 1, window clipped at grid
> bounds and renormalized; neighbors are read from the whole grid, writes stay inside the
> rect); *set-equal* default = arithmetic mean of the finite selected values; non-finite
> cells (the backend's NaN→null sentinel) are never edited and never contribute.

- [ ] **4.1 Write the failing tests** — the load-bearing cases per module (every file gets
  the SPDX header):

```ts
// tableOps.test.ts (write all of these)
import { describe, it, expect } from "vitest";
import { interpolateRect, smoothRect, scaleRect, setEqualRect, stepRect, type Grid } from "./tableOps";

const grid = (rows: number, cols: number, values: number[]): Grid => ({ rows, cols, values });

describe("interpolateRect", () => {
  it("linearly fills a 1xN run between its endpoints", () => {
    const g = grid(1, 4, [10, 0, 0, 40]);
    expect(interpolateRect(g, { r0: 0, c0: 0, r1: 0, c1: 3 })).toEqual([
      { index: 1, value: 20 },
      { index: 2, value: 30 },
    ]);
  });
  it("bilinearly fills a 3x3 rect from its corners", () => {
    const v = [0, 0, 20, 0, 99, 0, 40, 0, 60]; // corners 0,20,40,60 → center 30
    const edits = interpolateRect(grid(3, 3, v), { r0: 0, c0: 0, r1: 2, c1: 2 });
    expect(edits.find((e) => e.index === 4)?.value).toBe(30);
    expect(edits.some((e) => [0, 2, 6, 8].includes(e.index))).toBe(false); // corners kept
  });
  it("is a no-op on a single cell", () => {
    expect(interpolateRect(grid(2, 2, [1, 2, 3, 4]), { r0: 0, c0: 0, r1: 0, c1: 0 })).toEqual([]);
  });
});

describe("smoothRect", () => {
  it("pulls a spike toward its neighbors, writing only inside the rect", () => {
    const g = grid(3, 3, [50, 50, 50, 50, 90, 50, 50, 50, 50]);
    const edits = smoothRect(g, { r0: 1, c0: 1, r1: 1, c1: 1 });
    expect(edits).toHaveLength(1);
    expect(edits[0].index).toBe(4);
    expect(edits[0].value).toBeCloseTo((90 * 4 + 50 * 8 + 50 * 4) / 16, 6);
    expect(edits[0].value).toBeLessThan(90);
  });
  it("renormalizes the kernel at the grid corner", () => {
    const g = grid(2, 2, [80, 40, 40, 40]);
    const [e] = smoothRect(g, { r0: 0, c0: 0, r1: 0, c1: 0 });
    expect(e.value).toBeCloseTo((80 * 4 + 40 * 2 + 40 * 2 + 40 * 1) / 9, 6);
  });
});

describe("scaleRect / setEqualRect / stepRect", () => {
  it("scales only the selection", () => {
    expect(scaleRect(grid(1, 3, [10, 20, 30]), { r0: 0, c0: 1, r1: 0, c1: 2 }, 1.1)).toEqual([
      { index: 1, value: 22 },
      { index: 2, value: 33 },
    ]);
  });
  it("set-equal defaults to the selection mean, skipping non-finite cells", () => {
    expect(setEqualRect(grid(1, 3, [10, NaN, 30]), { r0: 0, c0: 0, r1: 0, c1: 2 })).toEqual([
      { index: 0, value: 20 },
      { index: 2, value: 20 },
    ]);
  });
  it("steps every selected cell by delta", () => {
    expect(stepRect(grid(1, 2, [10, 20]), { r0: 0, c0: 0, r1: 0, c1: 1 }, -1)).toEqual([
      { index: 0, value: 9 },
      { index: 1, value: 19 },
    ]);
  });
});
```

```ts
// tsv.test.ts (write all of these)
it("round-trips a rect through TSV", () => {
  const g = { rows: 2, cols: 3, values: [1, 2, 3, 4, 5, 6] };
  const tsv = toTsv(g, { r0: 0, c0: 1, r1: 1, c1: 2 }, 0);
  expect(tsv).toBe("2\t3\n5\t6");
  expect(parseTsv(tsv)).toEqual([[2, 3], [5, 6]]);
});
it("formats with digits and accepts comma decimals (PL-locale paste)", () => {
  expect(toTsv({ rows: 1, cols: 1, values: [12.345] }, { r0: 0, c0: 0, r1: 0, c1: 0 }, 1)).toBe("12.3");
  expect(parseTsv("1,5\t2")).toEqual([[1.5, 2]]);
});
it("rejects non-numeric cells", () => {
  expect(parseTsv("1\tx")).toBeNull();
});
it("clips paste at the grid edge", () => {
  const g = { rows: 2, cols: 2, values: [0, 0, 0, 0] };
  expect(pasteEdits(g, { row: 1, col: 1 }, [[7, 8], [9, 10]])).toEqual([{ index: 3, value: 7 }]);
});
```

```ts
// selection.test.ts — cover: rectOf normalizes reversed anchors; move clamps at
// edges; move with extend=true moves only the focus; cellIndices is row-major.
// heatmap.test.ts — cover:
it("maps low→blue and high→red, clamps, and survives a degenerate range", () => {
  expect(heatColor(0, 0, 100)).toBe("hsl(220 70% 55%)");
  expect(heatColor(100, 0, 100)).toBe("hsl(0 70% 55%)");
  expect(heatT(-5, 0, 100)).toBe(0);
  expect(heatT(50, 100, 100)).toBe(0.5); // degenerate lo >= hi
  const [r, g, b] = heatRgb(50, 0, 100);
  for (const c of [r, g, b]) expect(c).toBeGreaterThanOrEqual(0);
  for (const c of [r, g, b]) expect(c).toBeLessThanOrEqual(1);
});
```

Run: `npm test`
Expected: FAIL — modules don't exist.

- [ ] **4.2 Implement the four modules.** Key bodies (SPDX header + brief module doc each):

```ts
// tableOps.ts — the pinned semantics
const idx = (g: Grid, r: number, c: number) => r * g.cols + c;
const at = (g: Grid, r: number, c: number) => g.values[idx(g, r, c)];
const editable = (v: number) => Number.isFinite(v);

export function interpolateRect(g: Grid, rect: Rect): CellEdit[] {
  const h = rect.r1 - rect.r0;
  const w = rect.c1 - rect.c0;
  if (h === 0 && w === 0) return [];
  const corners = [at(g, rect.r0, rect.c0), at(g, rect.r0, rect.c1),
    at(g, rect.r1, rect.c0), at(g, rect.r1, rect.c1)];
  if (corners.some((v) => !editable(v))) return [];
  const [tl, tr, bl, br] = corners;
  const edits: CellEdit[] = [];
  for (let r = rect.r0; r <= rect.r1; r++) {
    for (let c = rect.c0; c <= rect.c1; c++) {
      const isCorner = (r === rect.r0 || r === rect.r1) && (c === rect.c0 || c === rect.c1);
      if (isCorner || !editable(at(g, r, c))) continue;
      const fr = h === 0 ? 0 : (r - rect.r0) / h;
      const fc = w === 0 ? 0 : (c - rect.c0) / w;
      const top = tl + (tr - tl) * fc;
      const bottom = bl + (br - bl) * fc;
      edits.push({ index: idx(g, r, c), value: top + (bottom - top) * fr });
    }
  }
  return edits;
}

export function smoothRect(g: Grid, rect: Rect): CellEdit[] {
  const KERNEL = [1, 2, 1, 2, 4, 2, 1, 2, 1];
  const edits: CellEdit[] = [];
  for (let r = rect.r0; r <= rect.r1; r++) {
    for (let c = rect.c0; c <= rect.c1; c++) {
      if (!editable(at(g, r, c))) continue;
      let sum = 0;
      let weight = 0;
      for (let dr = -1; dr <= 1; dr++) {
        for (let dc = -1; dc <= 1; dc++) {
          const rr = r + dr;
          const cc = c + dc;
          if (rr < 0 || rr >= g.rows || cc < 0 || cc >= g.cols) continue;
          const v = at(g, rr, cc);
          if (!editable(v)) continue;
          const k = KERNEL[(dr + 1) * 3 + (dc + 1)];
          sum += v * k;
          weight += k;
        }
      }
      if (weight > 0) edits.push({ index: idx(g, r, c), value: sum / weight });
    }
  }
  return edits;
}
```

(`scaleRect`/`setEqualRect`/`stepRect` follow directly from their tests — map over
`cellIndices(rect, g.cols)`, skip non-finite, emit the new value. `tsv.ts`: `toTsv` joins
rows with `\n` and cells with `\t`, `value.toFixed(digits)` on finite values, `""`
otherwise; `parseTsv` splits `\r?\n` (dropping one trailing empty line), then `\t`, parses
`Number(cell.trim().replace(",", "."))`, any `NaN` → `null`; `pasteEdits` clips
row/col overflow. `heatmap.ts`: `heatT` = clamp((v-lo)/(hi-lo), 0, 1) with the degenerate
guard returning 0.5; `hue = Math.round(220 * (1 - t))`; `heatColor` returns
`` `hsl(${hue} 70% 55%)` ``; `heatRgb` converts the same `(hue/360, 0.7, 0.55)` with a
standard 15-line hslToRgb. `selection.ts` is arithmetic per its tests.)

- [ ] **4.3 Run → GREEN, lint clean.**

Run: `npm test && npm run lint && npm run format:check`
Expected: PASS.

- [ ] **4.4 Commit.**

```bash
git add src/components/table-editor
git commit -m "feat(app): table-editing core — selection model, interpolate/smooth/scale/set-equal, TSV, heatmap (pure, tested)"
```

---

## Task 5 — 2D heatmap table editor: DOM grid, keyboard, clipboard, wire-up *(roadmap: 2D table editor)*

**Files:**
- Create: `src/components/table-editor/TableEditor.tsx` (container),
  `src/components/table-editor/TableGrid.tsx` (presentational grid),
  `src/components/table-editor/table-editor.css`
- Modify: `src/stores/tune.ts` (`activeTable`/`activeCurve`/`mergeValues`/`setCells`),
  `src/components/dialogs/TunePanel.tsx` (Tables/Curves nav; drop the render-all block),
  `src/i18n/en.ts` + `src/i18n/pl.ts`
- Delete: `src/components/dialogs/TableField.tsx` (superseded; migrate its `dialogs.css` rules)
- Test: `src/components/table-editor/TableEditor.test.tsx`, extend `src/stores/tune.test.ts`

**Interfaces:**
- Consumes: Task 4 modules; `commands.getValues`/`commands.setCells` +
  `events.tuneDirtyEvent` (bindings); `TableDto` (extended, Task 2); `ConstantDto`
  (`kind.Array.{rows,cols}`, `digits`, `low`/`high`); `useTuneStore`.
- Produces: `useTuneStore` gains
  `activeTable: string | null`, `activeCurve: string | null`, `setActiveTable(name)`,
  `setActiveCurve(name)`, `mergeValues(patch: Record<string, Value>)`,
  `setCells(name: string, edits: CellEdit[]): Promise<void>` (optimistic + rollback —
  Tasks 6/7/11 reuse all of these).

> **DOM grid — the recorded ARCHITECTURE §3 exception** (locked decision 6): semantic
> `<table role="grid">`, roving active cell via `aria-activedescendant`, one `tabIndex=0`
> keyboard surface. Y rows render **top = highest load** (tuning convention) — display row
> `d` maps to data row `rows-1-d`; all indices in store/ops stay row-major data order.
> Cell edits are NOT link-gated (M3 decision: only burn/undo/redo are connected-only;
> `setValue`-family commands queue safely behind a reconnect) — mirror `Field.tsx`.
> Clipboard = `navigator.clipboard` (no dep; WKWebView `readText` may need a user gesture —
> paste is always triggered by Ctrl/Cmd+V, which is one; failures degrade to the error
> line). Keymap (document in the component header): arrows move · Shift+arrows extend ·
> Ctrl/Cmd+A select-all · Enter edit/commit+down · Esc cancel · type-to-edit · `+`/`-` step
> (Shift ×10) · `=` set-equal · `/` interpolate · `s` smooth · Ctrl/Cmd+C/V copy/paste;
> *scale* runs from the toolbar (factor input + Apply) — no keystroke prompt.

- [ ] **5.1 Failing store tests** (extend `src/stores/tune.test.ts`, mirroring its existing
  `setValue` optimistic/rollback tests): `mergeValues` patches without dropping existing
  keys; `setCells` optimistically patches `values[name].Array` at the edit indices, calls
  `commands.setCells(name, edits)` (mock), and restores the previous array when the command
  errors (and rethrows); `setActiveTable("veTable1Tbl")` clears `activeDialog`/`activeCurve`
  (and symmetrically for `setActiveDialog`/`setActiveCurve`). Run → RED.

- [ ] **5.2 Implement the store additions** in `src/stores/tune.ts`:

```ts
  activeTable: null as string | null,
  activeCurve: null as string | null,

  setActiveDialog: (activeDialog) =>
    set({ activeDialog, activeTable: null, activeCurve: null }),
  setActiveTable: (activeTable) =>
    set({ activeTable, activeDialog: null, activeCurve: null }),
  setActiveCurve: (activeCurve) =>
    set({ activeCurve, activeDialog: null, activeTable: null }),

  mergeValues: (patch) => set((s) => ({ values: { ...s.values, ...patch } })),

  setCells: async (name, edits) => {
    const previous = get().values[name];
    if (!previous || !("Array" in previous) || !previous.Array) {
      throw new Error(`no array value loaded for ${name}`);
    }
    const next = [...previous.Array];
    for (const e of edits) {
      next[e.index] = e.value;
    }
    // Optimistic update; the backend stays the source of truth via tune_dirty.
    set((s) => ({ values: { ...s.values, [name]: { Array: next } as Value } }));
    const result = await commands.setCells(
      name,
      edits.map((e) => ({ index: e.index, value: e.value })),
    );
    if (result.status === "error") {
      set((s) => ({ values: { ...s.values, [name]: previous } }));
      throw new Error(result.error);
    }
  },
```

(extend `INITIAL` + `reset` with the two new nulls; type `edits` as
`import type { CellEdit } from "../components/table-editor/tableOps"`). Run 5.1 → GREEN.

- [ ] **5.3 Failing component test** `TableEditor.test.tsx` — `vi.mock("../../ipc/bindings")`
  with `commands.getValues` resolving xBins/yBins/z arrays, `commands.setCells` resolving
  ok, and `events.tuneDirtyEvent.listen` returning `Promise.resolve(() => {})`; seed
  `useTuneStore.setState` with a definition containing one extended `TableDto`
  (`x_bins: "rpmBins", y_bins: "loadBins", z: "veTable"`, 2×2 `ConstantDto` Array shape)
  and `activeTable: "veTable1Tbl"`. Mock the clipboard:

```ts
const writeText = vi.fn().mockResolvedValue(undefined);
const readText = vi.fn().mockResolvedValue("60\t61");
Object.defineProperty(navigator, "clipboard", {
  value: { writeText, readText },
  configurable: true,
});
```

Assert: (a) the grid renders `role="grid"` with axis headers from the bin values and cell
text from z (display rows reversed — highest load first); (b) ArrowRight moves the active
cell (`aria-activedescendant` changes); (c) typing `55` then Enter calls
`commands.setCells("veTable", [{ index: <active>, value: 55 }])`; (d) Ctrl+C writes TSV of
the selection via `writeText`; (e) Ctrl+V calls `setCells` with the parsed clipboard edits;
(f) clicking Interpolate with a multi-cell selection dispatches the Task 4 edits. Run → RED.

- [ ] **5.4 Implement `TableGrid.tsx`** (presentational; ~120 lines): props

```ts
interface TableGridProps {
  gridId: string;                    // aria-activedescendant id prefix
  xLabels: string[];                 // formatted bin labels (column headers)
  yLabels: string[];                 // row headers, DATA order (component reverses)
  values: (number | null)[];         // row-major data order
  rows: number;
  cols: number;
  digits: number;
  heatLo: number;
  heatHi: number;
  selection: Selection | null;
  active: Cell | null;
  draft: { cell: Cell; text: string } | null; // in-progress edit
  readOnly?: boolean;
  cellTitle?: (index: number) => string | undefined; // AutoTune tooltip hook (Task 11)
  onCellMouseDown: (cell: Cell, shift: boolean) => void;
  onCellMouseEnter: (cell: Cell, buttons: number) => void;
  onDraftChange: (text: string) => void;
}
```

Renders `<table role="grid" className="te-grid">`: a header row of `xLabels`
(`<th scope="col">`), then **display-reversed** data rows — each `<th scope="row">` from
`yLabels[dataRow]` + `<td role="gridcell" id={gridId + "-" + index}
aria-selected={inSelection} style={{ background: heatColor(...) }}>`; finite cells show
`value.toFixed(digits)`, `null`/non-finite show `"—"` with no heat background; the draft
cell renders `<input value={draft.text} onChange={(e) => onDraftChange(e.target.value)}
autoFocus>` instead of text; `cellTitle?.(index)` lands on the `<td title>`. Selection ring
via `.te-cell--active`/`.te-cell--selected` classes. No keyboard here — the container owns
it (single surface).

- [ ] **5.5 Implement `TableEditor.tsx`** (container; ~260 lines). Skeleton:

```ts
export function TableEditor({ locale }: { locale: Locale }) {
  const definition = useTuneStore((s) => s.definition);
  const activeTable = useTuneStore((s) => s.activeTable);
  const values = useTuneStore((s) => s.values);
  const table = definition?.tables.find((t) => t.name === activeTable) ?? null;
  // constants lookup + z shape (rows/cols) from ConstantDto.kind.Array
  // local state: selection, active, draft, error, view ("2d" | "3d"), scaleFactor
```

Data loading (fetch-then-merge; refetch on every `tune_dirty` — undo/redo/burn all emit
it, keeping the grid honest even after its own optimistic writes):

```ts
  useEffect(() => {
    if (!table) return;
    let cancelled = false;
    const names = [table.x_bins, table.y_bins, table.z];
    const fetchValues = async () => {
      const res = await commands.getValues(names);
      if (cancelled || res.status !== "ok") return;
      useTuneStore.getState().mergeValues(
        Object.fromEntries(names.map((n, i) => [n, res.data[i]])),
      );
    };
    fetchValues();
    const unlisten = events.tuneDirtyEvent.listen(fetchValues);
    return () => {
      cancelled = true;
      unlisten.then((f) => f());
    };
  }, [table]);
```

Grid assembly: `zVal = values[table.z]` → `gridValues: number[]` (null → NaN);
`Grid = { rows, cols, values: gridValues }`; axis labels from
`values[table.x_bins]`/`values[table.y_bins]` formatted with each bin constant's `digits`;
heat range = z constant's `low`/`high` when both literal, else finite min/max of the data.
One commit path for every gesture:

```ts
  const applyEdits = async (edits: CellEdit[]) => {
    if (!table || edits.length === 0) return;
    setError(null);
    try {
      await useTuneStore.getState().setCells(table.z, edits);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
```

Keyboard on the grid wrapper (`<div className="te-surface" tabIndex={0} onKeyDown={onKey}
aria-activedescendant={activeId}>`): implement the pinned keymap — arrows/Tab via `move(...)`
(Shift extends), Ctrl/Cmd+A full-rect selection, Enter commits the draft
(`Number(draft.text.replace(",", "."))`, empty/NaN → revert-never-write, the `Field.tsx`
rules) then moves down, Escape cancels the draft or collapses the selection, printable
`[0-9.,-]` starts a draft seeded with the typed char, `+`/`-` →
`applyEdits(stepRect(grid, rect, step * (e.shiftKey ? 10 : 1)))` with
`step = 10 ** -digits`, `=` → `setEqualRect`, `/` → `interpolateRect`, `s` → `smoothRect`,
Ctrl/Cmd+C → `navigator.clipboard.writeText(toTsv(grid, rect, digits)).catch(...)`,
Ctrl/Cmd+V → `readText()` → `parseTsv` → `pasteEdits(grid, { row: rect.r0, col: rect.c0 },
data)` → `applyEdits` (clipboard failures → the error line; WKWebView caveat comment).
Toolbar: `table.title` + `up_down_label` hint, Interpolate/Smooth/Set-equal buttons, Scale
factor `<input type="number">` + Apply, a 2D/3D view toggle (`view === "3d"` renders a
`te-3d-placeholder` div this task — Task 7 fills it), and `help` as an external link when
present. Mouse: `onCellMouseDown` sets anchor (Shift extends instead), `onCellMouseEnter`
with `buttons === 1` drags the focus.

- [ ] **5.6 Wire the navigation** in `TunePanel.tsx`: after the existing menu `<nav>`, add
  Tables + Curves navs from the definition (same button classes/`aria-current` pattern):

```tsx
        {definition.tables.length > 0 && (
          <nav className="tune-menu" aria-label={t("table.navLabel", locale)}>
            {definition.tables.map((table) => (
              <button
                key={table.name}
                type="button"
                className="tune-menu-item"
                aria-current={activeTable === table.name}
                onClick={() => useTuneStore.getState().setActiveTable(table.name)}
              >
                {table.title || table.name}
              </button>
            ))}
          </nav>
        )}
```

(curves symmetric with `setActiveCurve`/`curve.title || curve.name` — add the block now,
it renders nothing until Task 6 because the sim INI has no curves yet); in the content area
render `{activeTable ? <TableEditor locale={locale} /> : <DialogEngine …/>}` (Task 6 adds
the `activeCurve` branch). **Delete** the render-all `definition.tables.map(TableField)`
block and `TableField.tsx` itself; migrate any still-referenced `.table-*` rules from
`dialogs.css` into `table-editor.css`.

- [ ] **5.7 i18n + CSS.** `en.ts` additions (pl mirrors, translated):

```ts
  "table.navLabel": "Tables",
  "curve.navLabel": "Curves",
  "table.interpolate": "Interpolate",
  "table.smooth": "Smooth",
  "table.scale": "Scale",
  "table.scaleFactor": "Scale factor",
  "table.apply": "Apply",
  "table.setEqual": "Set equal",
  "table.view2d": "2D",
  "table.view3d": "3D",
  "table.help": "Help",
  "table.clipboardError": "Clipboard unavailable",
  "table.noValues": "Table values not loaded yet",
```

`table-editor.css`: tokens only (`--color-surface/-text/-accent/--space-md`); `.te-grid`
collapsed borders; `.te-cell--selected` outline `var(--color-accent)`; `.te-cell--active`
2px outline; cell text `oklch(15% 0 0)` over heat backgrounds (heat colors are
mid-lightness by construction, dark text reads on all of them — comment this); the grid
wrapper gets `overflow-x: auto` (wide tables scroll inside their container, never the page).

- [ ] **5.8 Run everything.**

Run: `npm test && npm run lint && npm run format:check && npm run build`
Expected: PASS (the build proves the `TableField` removal left no dangling import).

- [ ] **5.9 Commit.**

```bash
git add -A
git commit -m "feat(app): 2D heatmap table editor — DOM grid, keyboard, TSV clipboard, ops via set_cells"
```

---

## Task 6 — Curve editors (1D) *(roadmap: curve editors)*

**Files:**
- Create: `src/components/curve-editor/CurveEditor.tsx`, `src/components/curve-editor/curveMath.ts`,
  `src/components/curve-editor/curve-editor.css`
- Modify: `src/components/dialogs/TunePanel.tsx` (the `activeCurve` content branch), i18n dicts
- Test: `src/components/curve-editor/curveMath.test.ts`, `src/components/curve-editor/CurveEditor.test.tsx`

**Interfaces:**
- Consumes: `CurveDto` (Task 2), the Task 4 core (a curve is a `Grid` with `rows: 1`), the
  Task 5 store actions (`setCells`, `mergeValues`), `TableGrid`, `useRealtimeStore`
  (`getChannel` for the live cursor — the M3 rAF pattern).
- Produces:

```ts
// curveMath.ts (pure)
export type Range = { min: number; max: number };
export function axisRange(axis: { min: number | null; max: number | null } | null | undefined, data: number[]): Range;
export function polylinePoints(xs: number[], ys: number[], xr: Range, yr: Range, w: number, h: number, pad: number): string; // "x1,y1 x2,y2 ..."
export function cursorFraction(xValue: number, xr: Range): number | null; // 0..1, null outside
```

> **WRITE FRESH**; grid-reuse is locked decision 10: y-values edit through the same
> selection/ops/TSV/`setCells` machinery on a 1×N grid; x-bins render as read-only column
> headers (axis editing deferred, recorded). The preview is inline SVG (static redraw per
> data change — cheap, semantic, no dep); only the live cursor line moves at frame rate,
> positioned imperatively through a ref inside a rAF loop reading
> `useRealtimeStore.getState().getChannel(curve.x_channel)` — the M3
> no-React-state-per-frame rule.

- [ ] **6.1 Failing `curveMath` tests:** `axisRange` prefers literal DTO bounds
  (`{min: -40, max: 215}` from an `AxisDto`), falls back to finite-data extents when bounds
  are `null`, and returns `{min: 0, max: 1}` when both sources are empty/degenerate;
  `polylinePoints([0, 100], [0, 50], {min:0,max:100}, {min:0,max:50}, 200, 100, 10)` →
  `"10,90 190,10"` (y inverted, padding applied); `cursorFraction(50, {min:0,max:100})` →
  `0.5`, `cursorFraction(150, …)` → `null`. Run → RED. Implement (≈45 lines) → GREEN.

- [ ] **6.2 Implement `CurveEditor.tsx`** (~180 lines): same skeleton as `TableEditor` —
  find the `CurveDto` by `activeCurve`; fetch/refetch `[curve.x_bins, curve.y_bins]` on
  mount + `tune_dirty` (the 5.5 effect verbatim with two names); build
  `Grid = { rows: 1, cols: n, values: ys }`; render `TableGrid` with `xLabels` from the
  x-bin values (+ `column_labels` as a caption), `yLabels: [""]`, the same keyboard surface
  (the Task 5 keymap — `/` interpolate degenerates naturally to linear on one row); every
  gesture commits via `setCells(curve.y_bins, edits)`. Above the grid, the preview:

```tsx
      <svg className="curve-preview" viewBox="0 0 400 200" role="img"
           aria-label={curve.title || curve.name}>
        <polyline fill="none" stroke="var(--color-accent)" strokeWidth="2"
                  points={polylinePoints(xs, ys, xr, yr, 400, 200, 12)} />
        <line ref={cursorRef} className="curve-cursor" y1="0" y2="200"
              stroke="var(--color-warn)" strokeWidth="1.5" visibility="hidden" />
      </svg>
```

with the imperative rAF cursor (no React state per frame):

```ts
  useEffect(() => {
    if (!curve?.x_channel) return;
    let frame = 0;
    const paint = () => {
      const v = useRealtimeStore.getState().getChannel(curve.x_channel);
      const el = cursorRef.current;
      if (el) {
        const f = v === undefined ? null : cursorFraction(v, xr);
        if (f === null) {
          el.setAttribute("visibility", "hidden");
        } else {
          const x = 12 + f * (400 - 24);
          el.setAttribute("visibility", "visible");
          el.setAttribute("x1", String(x));
          el.setAttribute("x2", String(x));
        }
      }
      frame = requestAnimationFrame(paint);
    };
    frame = requestAnimationFrame(paint);
    return () => cancelAnimationFrame(frame);
  }, [curve?.x_channel, xr]);
```

- [ ] **6.3 Wire the content branch** in `TunePanel.tsx`:
  `{activeTable ? <TableEditor …/> : activeCurve ? <CurveEditor locale={locale} /> : <DialogEngine …/>}`.
  i18n: add `"curve.preview": "Curve preview"` (+ pl mirror).

- [ ] **6.4 Component test:** mock bindings as in 5.3 (`getValues` resolves
  `xs = [0, 50, 100]`, `ys = [10, 20, 30]`); assert the 1×3 grid renders ys with x headers
  from the bins, typing `25` + Enter on the middle cell calls
  `commands.setCells(<y_bins name>, [{ index: 1, value: 25 }])`, and the `polyline` carries
  a `points` attribute with three pairs. Run: `npm test` → PASS.

- [ ] **6.5 Run the gate + commit.**

Run: `npm test && npm run lint && npm run format:check && npm run build`

```bash
git add -A
git commit -m "feat(app): 1D curve editors — grid-machinery reuse + SVG preview with live cursor"
```

---

## Task 7 — 3D surface view: lazy three.js + live operating-point overlay *(roadmap: 3D surface)*

**Files:**
- Create: `src/components/surface/surfaceGeometry.ts` (pure), `src/components/surface/SurfaceView.tsx` (default export — lazy boundary), `src/components/surface/surface.css`
- Modify: `package.json` (`three` dep + `@types/three` dev-dep),
  `src/components/table-editor/TableEditor.tsx` (replace the 5.5 placeholder with the lazy mount)
- Test: `src/components/surface/surfaceGeometry.test.ts`, `src/components/surface/SurfaceView.test.tsx` (fail-open smoke)

**Interfaces:**
- Consumes: the grid data TableEditor already assembles (bins + values + heat range),
  `TableDto.x_channel`/`y_channel`, `useRealtimeStore.getState().getChannel` (M3 rAF
  pattern), `heatRgb` (Task 4).
- Produces:

```ts
// surfaceGeometry.ts (pure — testable without WebGL)
export function normalize(bins: number[]): number[]; // min..max → 0..1 (degenerate → all 0.5)
export function surfacePositions(xBins: number[], yBins: number[], values: number[], heightScale: number): Float32Array; // [x, y(=height), z] per vertex, rows*cols vertices
export function surfaceIndices(rows: number, cols: number): Uint32Array; // 2 triangles per quad
export function surfaceColors(values: number[], lo: number, hi: number): Float32Array; // heatRgb per vertex
export function bilinearHeight(xBins: number[], yBins: number[], values: number[], x: number, y: number): number | null; // physical → interpolated cell value

// SurfaceView.tsx — the ONLY module that imports three (default export for React.lazy)
export interface SurfaceViewProps {
  xBins: number[]; yBins: number[]; values: number[];
  heatLo: number; heatHi: number;
  xChannel: string; yChannel: string; // "" = no live dot
}
export default function SurfaceView(props: SurfaceViewProps): JSX.Element;
```

> **Locked decision 9 (raw three, lazy).** `npm i three` + `npm i -D @types/three` — the
> single sanctioned dep (+ its types). **The chunk boundary is `React.lazy` in
> `TableEditor`**; `SurfaceView.tsx` may use plain static
> `import * as THREE from "three"` / `import { OrbitControls } from
> "three/examples/jsm/controls/OrbitControls.js"` because the *module itself* is only ever
> reached via `import()` — no other file may import three or `SurfaceView` statically
> (LibreTune's exact mistake; the 7.6 measurement catches it). WKWebView pins:
> `renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))`; `webglcontextlost`
> (preventDefault + stop rAF) / `webglcontextrestored` (rebuild) handlers; full dispose on
> unmount; no per-frame allocation (reuse one `Vector3`). Renderer creation is wrapped in
> try/catch → a `surface.unavailable` message (fail-open — also what jsdom tests hit).

- [ ] **7.1 Install the dependency** (the one sanctioned addition):

Run: `npm i three@^0.182.0 && npm i -D @types/three@^0.182.0 && git diff package.json`
Expected: exactly `three` under dependencies, `@types/three` under devDependencies.

- [ ] **7.2 Failing `surfaceGeometry` tests:** `normalize([1000, 2000, 3000])` →
  `[0, 0.5, 1]`, degenerate `normalize([5, 5])` → `[0.5, 0.5]`;
  `surfacePositions([0, 1], [0, 1], [0, 10, 20, 30], 0.5)` has length 12 and vertex 3
  (`x=1, y=30·(0.5/30-range-normalized)…`) — assert concretely: positions[9..12] =
  `[1, 0.5, 1]` (max value → full heightScale) and positions[0..3] = `[0, 0, 0]`;
  `surfaceIndices(2, 2)` → `Uint32Array [0, 2, 1, 1, 2, 3]` (one quad, CCW);
  `surfaceColors([0, 100], 0, 100)` first vertex ≈ heatRgb(0) (blue-ish: b > r), last
  red-ish (r > b); `bilinearHeight([0, 100], [0, 100], [0, 10, 20, 30], 50, 50)` → `15`;
  outside bins → `null`. Run → RED. Implement (pure array math, ~90 lines; height =
  `heightScale * (v - min) / (max - min)` over finite values, non-finite vertices get
  height 0 + gray `[0.5, 0.5, 0.5]`) → GREEN.

- [ ] **7.3 Implement `SurfaceView.tsx`** (~230 lines, imperative core in one
  mount-effect):

```tsx
// SPDX-License-Identifier: GPL-3.0-or-later
// The ONLY module importing three — reached exclusively via React.lazy(() =>
// import(...)), which makes it (and three) a separate Vite chunk. Bundle
// budget: eager main chunk < 125 kB gz; this chunk ≤ 180 kB gz (Task 7.6).
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
```

Mount effect: guard `const canvas = canvasRef.current`; `try { renderer = new
THREE.WebGLRenderer({ canvas, antialias: true }); } catch { setUnavailable(true); return; }`;
`renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))`; scene + `PerspectiveCamera(50,
w/h, 0.01, 10)` positioned `(1.2, 1.0, 1.6)` looking at the surface center `(0.5, 0.25, 0.5)`;
`OrbitControls(camera, canvas)` with `enableDamping = true`, target = center. Mesh:
`BufferGeometry` with `position`/`color` attributes from `surfacePositions(xBins, yBins,
values, 0.5)` / `surfaceColors(values, heatLo, heatHi)` + `setIndex(surfaceIndices(rows,
cols))` + `computeVertexNormals()`; `MeshBasicMaterial({ vertexColors: true, side:
THREE.DoubleSide })`; plus a `LineSegments` wireframe (`WireframeGeometry`, `opacity 0.15`).
Live dot: `Mesh(new THREE.SphereGeometry(0.02, 12, 12), new THREE.MeshBasicMaterial({ color:
0xffffff }))`, `dot.visible = false`. rAF loop (single reused `Vector3`, no allocation):

```ts
    const paint = () => {
      const xv = props.xChannel ? useRealtimeStore.getState().getChannel(props.xChannel) : undefined;
      const yv = props.yChannel ? useRealtimeStore.getState().getChannel(props.yChannel) : undefined;
      if (xv !== undefined && yv !== undefined) {
        const h = bilinearHeight(props.xBins, props.yBins, props.values, xv, yv);
        if (h === null) {
          dot.visible = false;
        } else {
          dot.visible = true;
          dot.position.set(
            fraction(xv, props.xBins),      // normalize against bin extents
            heightOf(h) + 0.03,             // hover just above the surface
            fraction(yv, props.yBins),
          );
        }
      } else {
        dot.visible = false;
      }
      controls.update();
      renderer.render(scene, camera);
      frame = requestAnimationFrame(paint);
    };
```

Context handlers on the canvas: `webglcontextlost` → `e.preventDefault();
cancelAnimationFrame(frame)`; `webglcontextrestored` → restart the loop. Data-change
effect (`[values, heatLo, heatHi]`): rewrite the two attributes in place
(`attr.copyArray(...); attr.needsUpdate = true`) — geometry topology never changes for a
fixed table. Cleanup: cancel rAF, remove listeners, `controls.dispose()`,
`geometry.dispose()`, both `material.dispose()`, `renderer.dispose()`. `unavailable` state
renders `<p className="surface-unavailable">{t("surface.unavailable", locale)}…` — pass
`locale` down or render a plain string + title from props (keep the component
locale-free: accept an `unavailableLabel: string` prop instead; TableEditor passes the
translated string — simpler than threading `t` into a lazy chunk).

- [ ] **7.4 Mount it lazily** in `TableEditor.tsx` — replace the 5.5 placeholder:

```tsx
const LazySurfaceView = lazy(() => import("../surface/SurfaceView"));
// …in render, when view === "3d":
        <Suspense fallback={<p className="te-3d-loading">{t("surface.loading", locale)}</p>}>
          <LazySurfaceView
            xBins={xBinValues}
            yBins={yBinValues}
            values={gridValues}
            heatLo={heatLo}
            heatHi={heatHi}
            xChannel={table.x_channel}
            yChannel={table.y_channel}
            unavailableLabel={t("surface.unavailable", locale)}
          />
        </Suspense>
```

i18n: `"surface.loading": "Loading 3D view…"`, `"surface.unavailable": "3D view unavailable
(WebGL not supported here)"` (+ pl mirrors).

- [ ] **7.5 Fail-open smoke test** `SurfaceView.test.tsx`: jsdom has no WebGL —
  `render(<SurfaceView xBins={[0,1]} yBins={[0,1]} values={[1,2,3,4]} heatLo={0} heatHi={4}
  xChannel="" yChannel="" unavailableLabel="no webgl" />)` (static import in the test is
  fine — tests are not the shipped bundle) must render the `no webgl` fallback, not throw.
  Run: `npm test` → PASS.

- [ ] **7.6 The bundle-budget gate** (Global Constraints numbers):

Run:
```bash
npm run build
for f in dist/assets/*.js; do echo "$f: $(gzip -c "$f" | wc -c) bytes gz"; done
```
Expected: ≥ 2 JS chunks; the eager entry chunk (`index-*.js`) **< 128000 bytes gz**; the
lazy `SurfaceView-*.js` chunk (carrying three) present and **≤ 184320 bytes gz**. If three
appears in the entry chunk, some file static-imports three or `SurfaceView` — find it with
`grep -rn "from \"three\"\|from \"../surface/SurfaceView\"" src/ --include="*.tsx" --include="*.ts"`
(only `SurfaceView.tsx` itself and test files may match) and fix before proceeding. Record
both measured numbers in the commit body.

- [ ] **7.7 Commit.**

```bash
git add -A
git commit -m "feat(app): lazy three.js 3D surface with live operating-point dot (chunk-split, WKWebView-hardened)"
```

---

## Task 8 — Owner-side realtime capture ring *(the ve_analyze data seam — ARCHITECTURE §5.5-compatible)*

**Files:**
- Create: `src-tauri/src/capture.rs`
- Modify: `src-tauri/src/owner.rs` (buffer field + poll-tick tap + the three capture arms),
  `src-tauri/src/analysis_commands.rs` (new — `start_capture`/`stop_capture`/`capture_status`),
  `src-tauri/src/lib.rs` (register + needles), `src-tauri/Cargo.toml` (dep on `opentune-analysis`)
- Test: `capture.rs` unit tests, one owner-level test in `src-tauri/src/owner_tests.rs`

**Interfaces:**
- Consumes: `RealtimeFrame` (M3), the owner `poll_tick` emit path, `SampleSet` (Task 0),
  `CaptureStatusDto` (Task 0).
- Produces:

```rust
// capture.rs
/// Bounded, column-oriented realtime capture ring. Columns are pinned at
/// start (definition channel order); each emitted frame appends one f64 row
/// (missing channel → NaN — fail-open per item). Oldest rows drop first.
///
/// Rate note (recorded): the tap sits on poll_tick's EMITTED frames. The M3
/// poller coalesces to ≤30 Hz, but the owner polls at 25 Hz (40 ms) — slower
/// than the 33 ms gate — so today every acquired frame is emitted and the
/// capture sees the full poll rate. If M5 ever polls faster than 30 Hz, move
/// the tap below the coalescing gate (poll.rs) — test `capture_rate_pins_
/// the_tap_invariant` breaks loudly if the assumption rots.
pub struct CaptureBuffer { /* columns, VecDeque<(f64 t_ms, Vec<f64>)>, start: Instant, capacity, dropped: u32 */ }

/// ~18 min at the 25 Hz emit rate; ~1.1 kB/row on the real INI (139 ch × 8 B).
pub const CAPTURE_CAPACITY: usize = 27_000;

impl CaptureBuffer {
    pub fn new(columns: Vec<String>, capacity: usize) -> Self;
    pub fn push(&mut self, frame: &opentune_realtime::RealtimeFrame); // t = now - start
    pub fn status(&self, capturing: bool) -> CaptureStatusDto;
    pub fn to_sample_set(&self) -> opentune_analysis::SampleSet;      // clones, order preserved
}
```

Commands: `startCapture()`, `stopCapture() -> CaptureStatusDto`,
`captureStatus() -> CaptureStatusDto`.

> **WRITE FRESH** (locked decision 4). Owner state: `capture: Option<CaptureBuffer>` +
> `capturing: bool`. `StartCapture` requires a session (columns = the definition's
> output-channel names in declaration order — pinned, deterministic), replaces any old
> buffer, sets `capturing = true`. `StopCapture` clears only the flag — the rows stay for
> `run_ve_analyze` (re-running with different params is a feature). `Connect`/`Disconnect`
> clear both (a fresh session never inherits — the M3 polling rule). Raw rows never cross
> IPC.

- [ ] **8.1 Failing `capture.rs` unit tests** (plain `#[test]`, hand-built
  `RealtimeFrame`s):

```rust
#[test]
fn push_fills_rows_by_pinned_columns_with_nan_for_missing() {
    let mut buf = CaptureBuffer::new(vec!["rpm".into(), "afr".into()], 4);
    buf.push(&frame(&[("afr", 14.7), ("rpm", 3000.0)])); // order differs from columns
    buf.push(&frame(&[("rpm", 3100.0)]));                // afr missing this frame
    let s = buf.to_sample_set();
    assert_eq!(s.columns, vec!["rpm", "afr"]);
    assert_eq!(s.rows[0], vec![3000.0, 14.7]);
    assert_eq!(s.rows[1][0], 3100.0);
    assert!(s.rows[1][1].is_nan());
    assert!(s.t_ms[1] >= s.t_ms[0]);
}

#[test]
fn ring_drops_oldest_and_counts() {
    let mut buf = CaptureBuffer::new(vec!["rpm".into()], 2);
    for v in [1.0, 2.0, 3.0] {
        buf.push(&frame(&[("rpm", v)]));
    }
    let s = buf.to_sample_set();
    assert_eq!(s.rows, vec![vec![2.0], vec![3.0]]);
    assert_eq!(buf.status(true).dropped, 1);
    assert_eq!(buf.status(true).sample_count, 2);
}

fn frame(pairs: &[(&str, f64)]) -> opentune_realtime::RealtimeFrame {
    opentune_realtime::RealtimeFrame {
        channels: pairs.iter().map(|(n, v)| opentune_realtime::ChannelValue {
            name: (*n).to_string(), value: *v,
        }).collect(),
        diagnostics: vec![],
    }
}
```

Run → RED; implement `CaptureBuffer` (VecDeque; `push` builds the row by
per-column linear lookup over `frame.channels` — no HashMap, deterministic;
`sample_count`/`dropped` saturate into `u32`) → GREEN.

- [ ] **8.2 Wire the owner.** `Owner` gains `capture: Option<CaptureBuffer>` +
  `capturing: bool` (init `None`/`false`). In `poll_tick`, tap the emitted frame **before**
  the event conversion:

```rust
            if let Ok(Some(frame)) = r {
                if self.capturing {
                    if let Some(buf) = self.capture.as_mut() {
                        buf.push(&frame);
                    }
                }
                let channels = frame.channels.into_iter().map(|c| (c.name, c.value)).collect();
                (self.emit)(OwnerEvent::Realtime(RealtimeFrameEvent { channels }));
            }
```

Replace the three Task 0 stub arms:

```rust
            Command::StartCapture { reply } => {
                let r = match &self.session {
                    Some(s) => {
                        let columns: Vec<String> = s.def.output_channels.iter()
                            .map(|c| c.name().to_string()).collect();
                        self.capture = Some(CaptureBuffer::new(columns, CAPTURE_CAPACITY));
                        self.capturing = true;
                        Ok(())
                    }
                    None => Err("not connected".to_string()),
                };
                let _ = reply.send(r);
            }
            Command::StopCapture { reply } => {
                self.capturing = false;
                let r = self.capture.as_ref()
                    .map(|b| b.status(false))
                    .ok_or_else(|| "no capture".to_string());
                let _ = reply.send(r);
            }
            Command::CaptureStatus { reply } => {
                let r = self.capture.as_ref()
                    .map(|b| b.status(self.capturing))
                    .ok_or_else(|| "no capture".to_string());
                let _ = reply.send(r);
            }
```

Clear both fields in the `Connect` and `Disconnect` arms (next to whatever they already
reset for polling). Add `opentune-analysis = { path = "crates/analysis" }` to
`src-tauri/Cargo.toml`.

- [ ] **8.3 Commands + registration.** `analysis_commands.rs` (SPDX header; thin senders on
  the Task 3 pattern):

```rust
#[tauri::command]
#[specta::specta]
pub async fn start_capture(owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::StartCapture { reply }).await
}

#[tauri::command]
#[specta::specta]
pub async fn stop_capture(owner: State<'_, OwnerHandle>) -> Result<CaptureStatusDto, String> {
    request(&owner, |reply| Command::StopCapture { reply }).await
}

#[tauri::command]
#[specta::specta]
pub async fn capture_status(owner: State<'_, OwnerHandle>) -> Result<CaptureStatusDto, String> {
    request(&owner, |reply| Command::CaptureStatus { reply }).await
}
```

Register all three in `collect_commands!`; needles `"startCapture"`, `"stopCapture"`,
`"captureStatus"`, `"CaptureStatusDto"`.

- [ ] **8.4 Owner-level test `capture_rate_pins_the_tap_invariant`** in `owner_tests.rs`
  (mirror the harness the M3 realtime owner tests use — connect via the
  `realtime-owner.ini` fixture, `StartRealtime`, drive time/ticks the way those tests do):
  `StartCapture` → let ~10 poll ticks elapse → `CaptureStatus` shows `sample_count ≥ 8`
  (every acquired frame captured — the 25 Hz < 30 Hz-gate invariant from the `capture.rs`
  doc) and `capturing == true`; `StopCapture` → a further tick window does **not** grow
  `sample_count` (flag off, rows retained). Run → GREEN.

- [ ] **8.5 Run everything.**

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test && cargo clippy --workspace -- -D warnings`
Expected: PASS; bindings gain the three commands.

- [ ] **8.6 Commit.**

```bash
git add -A
git commit -m "feat(app): owner-side realtime capture ring (bounded, column-oriented) + capture commands"
```

---

## Task 9 — Simulator: measured AFR + deliberate VE-error surface; sample-INI upgrade *(the demo's ground truth)*

**Files:**
- Modify: `src-tauri/crates/simulator/src/och_codec.rs` (`afr`/`ego_correction` channels),
  `src-tauri/crates/simulator/src/engine/mod.rs` (`VeContext` + measured-AFR computation),
  `src-tauri/crates/simulator/src/ecu.rs` (decode VE context from memory each engine tick)
- Create: `src-tauri/crates/simulator/src/ve_model.rs` (true-VE surface + array decode + bilinear)
- Modify: `src-tauri/resources/speeduino.sample.ini` (pages 2-4, new channels, `[TableEditor]`/`[CurveEditor]`/`[VeAnalyze]`)
- Test: `src-tauri/crates/simulator/tests/realtime.rs` (extend), `ve_model.rs` unit tests

**Interfaces:**
- Consumes: `ChannelValues`/`encode_channels` (och_codec), `SimEngine`/`Pipe`/`MemoryImage`
  (ecu.rs), `Definition.{tables, ve_analyze, constants}` (Tasks 0-2).
- Produces:

```rust
// ve_model.rs
/// The simulator's hidden "true VE" surface — what the engine actually needs.
/// Deterministic and cell-dependent: lean error grows with load and rpm.
pub(crate) fn true_ve(rpm: f64, load_kpa: f64) -> f64; // 40 + 25·(load/100) + 15·(rpm/6000), clamped 20..110

/// Decoded veTable context, refreshed from the memory image each engine tick.
pub(crate) struct VeContext { pub rpm_bins: Vec<f64>, pub load_bins: Vec<f64>, pub ve: Vec<f64> }
impl VeContext {
    /// Bilinear current-VE lookup, clamped to the bin range; None on shape mismatch.
    pub(crate) fn current_ve(&self, rpm: f64, load_kpa: f64) -> Option<f64>;
}
/// Resolve + decode the VE table the INI's [VeAnalyze] map points at.
/// Data-driven: def.ve_analyze.maps[0].table → TableDef → x/y/z constants →
/// raw page bytes → physical values. None when the INI has no [VeAnalyze].
pub(crate) fn ve_context(def: &Definition, memory: &MemoryImage) -> Option<VeContext>;
```

`ChannelValues` gains `afr: f64` + `ego_correction: f64`; `SimEngine` gains
`set_ve_context(Option<VeContext>)`.

> **WRITE FRESH** (extends the askrejans MIT port — keep both port-note headers intact).
> **The loop is closed** (locked decision 11): `afr = afr_target × true_ve / current_ve`
> (table VE too low ⇒ too little fuel ⇒ lean ⇒ measured AFR above target — the correction
> `VE_new = VE_old × afr/target = VE_old × true/current` converges to `true_ve` in one
> step). `current_ve` below 1.0 (e.g. a zeroed page) clamps to 1.0 — graceful, never
> divides by ~0. `ego_correction` is a constant 100.0 (Speeduino's channel is 100-centered;
> no trim in the sim — EGO math is unit-tested in `analysis` instead). Without a
> `[VeAnalyze]`/veTable in the loaded INI, `afr == afr_target` (old INIs behave as before).
> First sub-step: confirm the `MemoryImage` page-read accessor name in `memory.rs` (the
> same one the `'p'` read arm uses) and use it in `ve_context`.

- [ ] **9.1 Upgrade `resources/speeduino.sample.ini`** — extend `[Constants]` to four pages
  (page 1 unchanged) and add the M4 sections + channels. Exact additions:

```ini
[Constants]
    endianness      = little
    nPages          = 4
    pageSize        = 8, 288, 288, 16

page = 2
      veTable      = array,  U08,   0, [16x16], "%",   1.0,   0.0,   0.0,   255.0, 0
      rpmBins      = array,  U08, 256, [16],   "RPM", 100.0,  0.0, 100.0, 25500.0, 0
      fuelLoadBins = array,  U08, 272, [16],   "kPa",   1.0,  0.0,   0.0,   255.0, 0

page = 3
      afrTable    = array,  U08,   0, [16x16], "AFR",   0.1,  0.0,   7.0,    25.5, 1
      rpmBinsAFR  = array,  U08, 256, [16],   "RPM", 100.0,  0.0, 100.0, 25500.0, 0
      loadBinsAFR = array,  U08, 272, [16],   "kPa",   1.0,  0.0,   0.0,   255.0, 0

page = 4
      warmupBins   = array, U08,  0, [6], "C", 1.0, 0.0, 0.0, 255.0, 0
      warmupValues = array, U08,  6, [6], "%", 1.0, 0.0, 0.0, 255.0, 0
```

`[OutputChannels]` additions (block size stays 16; bytes 8-11 were free):

```ini
map           = scalar, U08,  8, "kPa",  1.000, 0.000
afr           = scalar, U08,  9, "O2",   0.100, 0.000
egoCorrection = scalar, U08, 10, "%",    1.000, 0.000
afrTarget     = scalar, U08, 11, "O2",   0.100, 0.000
fuelLoad      = { map }, "kPa"
```

New sections (after `[FrontPage]`):

```ini
[TableEditor]
   table = veTable1Tbl, veTable1Map, "VE Table", 2
      xBins       = rpmBins, rpm
      yBins       = fuelLoadBins, fuelLoad
      zBins       = veTable
      upDownLabel = "(RICHER)", "(LEANER)"
   table = afrTable1Tbl, afrTable1Map, "AFR Target Table", 3
      xBins       = rpmBinsAFR, rpm
      yBins       = loadBinsAFR, fuelLoad
      zBins       = afrTable

[CurveEditor]
   curve = warmup_curve, "Warmup Enrichment"
      columnLabel = "Coolant", "Added"
      xAxis = -40, 120, 4
      yAxis = 0, 120, 4
      xBins = warmupBins, coolant
      yBins = warmupValues

[VeAnalyze]
   veAnalyzeMap = veTable1Tbl, afrTable1Tbl, afr, egoCorrection
   filter = std_xAxisMin
   filter = std_xAxisMax
   filter = std_yAxisMin
   filter = std_yAxisMax
   filter = std_DeadLambda
   filter = minCltFilter, "Minimum CLT", coolant, <, 60, , true
```

Add a test asserting the upgraded sample INI parses with zero diagnostics and
`def.ve_analyze.is_some()` (put it next to whatever test already parses the sample INI —
grep `speeduino.sample.ini` under `src-tauri/`; if none exists, add
`crates/ini/tests/sample_ini.rs` with `include_str!("../../../resources/speeduino.sample.ini")`).

- [ ] **9.2 Failing `ve_model` tests** (in-module `#[cfg(test)]`):
  `true_ve(6000.0, 100.0)` = 80.0, `true_ve(800.0, 30.0)` = 49.5 exactly (pin the formula:
  `40 + 25·load/100 + 15·rpm/6000` before clamping);
  a hand-built `VeContext { rpm_bins: vec![1000.0, 2000.0], load_bins: vec![20.0, 40.0],
  ve: vec![50.0, 60.0, 70.0, 80.0] }` → `current_ve(1500.0, 30.0)` = `Some(65.0)`,
  out-of-range clamps to the edge (`current_ve(500.0, 10.0)` = `Some(50.0)`), a
  wrong-length `ve` → `None`. Run → RED; implement (`current_ve` reuses the same
  segment+fraction bilinear the analysis crate pins — ~30 deliberately duplicated lines,
  the M3 "30 trivial lines over a dependency" precedent, note it in the header) → GREEN.

- [ ] **9.3 Implement `ve_context`:** find `def.ve_analyze.as_ref()?.maps.first()?`, resolve
  `def.table(&map.table)?`, then its `x_bins`/`y_bins`/`z` constants via `def.constant`;
  read each constant's raw bytes from the `MemoryImage` page (the accessor confirmed in the
  first sub-step) and decode per `ScalarType`/endianness/scale/translate into physical
  `Vec<f64>` (U08-array decode is `byte as f64 * scale + translate` — support the scalar
  types `och_codec` already writes; `Number::Expr` scale → return `None`, fail-open).
  Unit-test with `EcuSimulator::from_definition(&sample_def)` + a `set`-written veTable —
  or directly against `MemoryImage` if construction is simpler there.

- [ ] **9.4 Failing test `sim_measured_afr_reflects_ve_error`** in `tests/realtime.rs`:
  parse the upgraded sample INI; build `EcuSimulator::from_definition`; write a **flat-50
  veTable + real bins + flat-14.7 afrTable** into the sim's memory through the existing
  M2 write path (`MsProtocol` page writes, as the memory tests do — rpmBins raw
  `5,10,…,80`, loadBins raw `20,25,…,95`); `tick_engine` until the engine leaves startup
  (mirror the existing 500 ms warm-up); read the och block; decode `afr` and `afrTarget`.
  Assert `afr > afrTarget` (true VE above 50 at running load ⇒ lean) and that pushing the
  veTable cells to the true-VE values (write `true_ve(rpm_bin, load_bin)` per cell, re-tick,
  re-read) brings `|afr - afrTarget| < 0.3`. Run → RED.

- [ ] **9.5 Implement the loop closure:** `ChannelValues` gains `afr: f64, ego_correction:
  f64`; `och_codec::scalar` gains arms `"afr" => self.afr,` and
  `"egoCorrection" => self.ego_correction,`. `SimEngine` gains `ve_ctx:
  Option<VeContext>` + `pub fn set_ve_context(&mut self, ctx: Option<VeContext>)`;
  `snapshot()` computes:

```rust
        let afr_target = f64::from(AFR_STOICH) / 10.0;
        let afr = match &self.ve_ctx {
            Some(ctx) => {
                let current = ctx
                    .current_ve(f64::from(self.rpm), f64::from(self.map_kpa))
                    .unwrap_or(1.0)
                    .max(1.0); // zeroed page must not explode the ratio
                let wanted = crate::ve_model::true_ve(f64::from(self.rpm), f64::from(self.map_kpa));
                afr_target * wanted / current
            }
            None => afr_target, // no VE binding in this INI — behave as before M4
        };
        // …
            afr,
            ego_correction: 100.0, // Speeduino egoCorrection is 100-centered; no trim in the sim
```

In `ecu.rs`, wherever `tick_engine` ticks the engine under the pipe lock, refresh first:
`let ctx = ve_model::ve_context(&definition…, &p.memory); engine.set_ve_context(ctx);` —
the `Definition` must be reachable there; if `Pipe`/`EcuSimulator` doesn't retain it beyond
construction, store the three resolved `ConstantDef`s (+ endianness) on the `Pipe` at
`from_definition` time instead and pass them to `ve_context` (smaller retained state —
prefer this if `Definition` isn't already kept). Run 9.4 → GREEN; whole simulator suite +
M1/M3 sim tests stay green (INIs without `[VeAnalyze]` take the `None` path).

- [ ] **9.6 Run everything + commit.**

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test && cargo clippy --workspace -- -D warnings`

```bash
git add -A
git commit -m "feat(simulator): measured AFR closes the VE loop (true-VE surface vs table); sample INI gains tables/curves/[VeAnalyze]"
```

---

## Task 10 — `opentune-analysis::ve_analyze` — the deterministic engine *(roadmap: first auto-tune; design doc §3.1)*

**Files:**
- Modify: `src-tauri/crates/analysis/src/grid.rs` (fill `TableGrid::lookup` + the shared `segment`),
  `src-tauri/crates/analysis/src/lib.rs` (re-export; move the `ve_analyze` stub into `src/ve_analyze.rs`)
- Create: `src-tauri/crates/analysis/src/ve_analyze.rs`
- Test: `src-tauri/crates/analysis/tests/ve_analyze.rs`, `src-tauri/crates/analysis/tests/grid.rs`

**Interfaces:**
- Consumes/Produces: exactly the Task 0 seam — no signature changes. Everything below is
  *algorithm*, pinned so two implementers would write the same function.

**The pinned algorithm** (every rule is normative; deviations are plan bugs):

1. **Validation.** `x_bins.len() ≥ 2 && y_bins.len() ≥ 2`, `z.len() == x_len·y_len` for
   both grids, else `Err(ShapeMismatch(msg))`. Resolve `x/y/afr` columns via
   `samples.column(...)` — missing → `Err(MissingChannel(name))`; the `ego` column is
   required only when `params.ego_center > 0`. Custom-filter channels resolve up front; a
   filter whose channel is absent is **not evaluated** but still appears in `filtered`
   with `count: 0` (auditability: the user sees it existed and did nothing).
2. **Lag pairing** (wideband transport delay): pair index `i` in
   `lag..rows.len()` — operating point (`x`, `y`, `ego`, custom-filter channels) from
   `rows[i - lag]`, measured `afr` from `rows[i]`. `lag = params.lag_records as usize`.
   `total_samples = rows.len()`; pair count = `rows.len().saturating_sub(lag)`.
3. **Filters, first match wins, fixed order.** Built-ins first: `nonFinite` (any of
   x/y/afr — and ego when used — non-finite), then `targetMissing`
   (`target.lookup(x, y)` is `None` or `≤ 0.0`); then `binding.filters` in declaration
   order, skipping `Custom` ids listed in `params.disabled_filters`:
   `XAxisMin: x < x_bins[0]` · `XAxisMax: x > *x_bins.last()` · `YAxisMin/YAxisMax`
   likewise · `DeadLambda: afr ≤ 0.0` · `Custom { op, value }` rejects when
   `Lt: ch < value` / `Gt: ch > value` / `Eq: ch == value` /
   `And: (ch as i64) & (value as i64) != 0` (a non-finite custom-filter channel value
   never matches). Every filter (built-ins included) gets a `FilterCount` row even at 0;
   `filtered` order = `[nonFinite, targetMissing, …binding.filters]`.
4. **Per-sample factor.** `ego_factor = if params.ego_center > 0.0 { ego /
   params.ego_center } else { 1.0 }`; `factor = ego_factor · afr / target_val`
   (measured lean ⇒ factor > 1 ⇒ raise VE — the MS Extra form; EGO neutralization folds
   the trim the ECU was already applying back into the table).
5. **Bilinear accumulation** (MLV-style fractional hits): `segment(bins, v) → (i, t)`
   where `bins[i] ≤ v ≤ bins[i+1]`, `t = (v - bins[i]) / (bins[i+1] - bins[i])`
   (equal-neighbor segment ⇒ `t = 0`). Four cells `(ix+dx, iy+dy)` get
   `w = fx·fy` with `fx ∈ {1-tx, tx}`, `fy ∈ {1-ty, ty}`. Per-cell accumulators are four
   flat `Vec<f64>`/`Vec<u32>` indexed `y·x_len+x` — **no HashMap anywhere**:
   `sum_w += w`, `sum_wf += w·factor`, `sum_wf2 += w·factor²`; `sample_count += 1` on the
   max-weight cell only (tie → lowest flat index). Samples accumulate in row order —
   the deterministic reduction order.
6. **Per-cell finalize, flat index order.** If `current == 0.0` or
   `sum_w < params.min_weight`: `proposed = current`, `delta_pct = 0`, `confidence = 0`.
   Else: `mean = sum_wf / sum_w`; `var = max(0, sum_wf2/sum_w − mean²)`;
   `w_conf = min(1, sum_w / params.confidence_sat_weight)`;
   `v_conf = 1 / (1 + params.variance_penalty · var)`; `confidence = w_conf · v_conf`;
   `raw = current · mean`;
   `blended = current + (raw − current) · confidence · (1 − params.cell_change_resistance)`;
   `max_delta = |current| · params.max_delta_pct / 100`;
   `proposed = blended.clamp(current − max_delta, current + max_delta)`;
   `delta_pct = (proposed − current) / current · 100`. `hit_weight = sum_w`.
7. `used_samples = pairs − Σ filter counts`. No RNG; no parallelism; float accumulation
   order fully pinned by 5-6.

> **WRITE FRESH** (locked decision 3 + port ledger): no code source exists — TS/MLV are
> proprietary behavioral references; the correction form is MS Extra manual + Speeduino
> `[VeAnalyze]` semantics. This function is the design-doc §3.1 contract: the same engine
> later feeds the AutoTune UI, the AI layer, and MCP — numbers only ever come from here.

- [ ] **10.1 Failing `grid.rs` tests:** `lookup` on
  `TableGrid { x_bins: [1000, 2000], y_bins: [20, 40], z: [10, 20, 30, 40] }` at
  `(1500, 30)` → `Some(25.0)`; at the corner `(1000, 20)` → `Some(10.0)`; outside
  (`(999, 30)`, `(1500, 41)`) → `None`; duplicate bins `[1000, 1000]` don't divide by zero
  (`t = 0` path — `lookup(1000, …)` still `Some`). Run → RED; implement `segment` +
  `lookup` (~40 lines, shared with `ve_analyze`'s accumulation) → GREEN.

- [ ] **10.2 Failing `ve_analyze` tests** — write ALL of these
  (`tests/ve_analyze.rs`; shared helpers at the top):

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
//! Determinism + semantics tests for the ve_analyze engine (M4 Task 10).
use opentune_analysis::*;

fn flat_grid(v: f64) -> TableGrid {
    TableGrid { x_bins: vec![1000.0, 2000.0], y_bins: vec![20.0, 40.0], z: vec![v; 4] }
}
fn binding() -> AnalyzeBinding {
    AnalyzeBinding {
        x_channel: "rpm".into(), y_channel: "map".into(),
        afr_channel: "afr".into(), ego_channel: "egoCorrection".into(),
        filters: vec![
            FilterSpec::XAxisMin, FilterSpec::XAxisMax,
            FilterSpec::YAxisMin, FilterSpec::YAxisMax,
            FilterSpec::DeadLambda,
        ],
    }
}
fn params(lag: u32) -> VeAnalyzeParams {
    VeAnalyzeParams { lag_records: lag, ..VeAnalyzeParams::default() }
}
fn samples(rows: Vec<Vec<f64>>) -> SampleSet {
    SampleSet {
        columns: vec!["rpm".into(), "map".into(), "afr".into(), "egoCorrection".into()],
        t_ms: (0..rows.len()).map(|i| i as f64 * 40.0).collect(),
        rows,
    }
}

#[test]
fn lean_sample_raises_ve_with_pinned_numbers() {
    // One sample exactly on cell 0: factor = 29.4/14.7 = 2.0 exactly.
    let s = samples(vec![vec![1000.0, 20.0, 29.4, 100.0]]);
    let r = ve_analyze(&s, &flat_grid(50.0), &flat_grid(14.7), &binding(), &params(0)).unwrap();
    let c = &r.cells[0];
    assert!((c.hit_weight - 1.0).abs() < 1e-12);
    assert_eq!(c.sample_count, 1);
    // w_conf = 1/20, v_conf = 1 (var 0) → confidence 0.05;
    // blended = 50 + (100-50)·0.05·0.8 = 52.0 (inside the ±15% clamp).
    assert!((c.confidence - 0.05).abs() < 1e-9);
    assert!((c.proposed - 52.0).abs() < 1e-9);
    assert!((c.delta_pct - 4.0).abs() < 1e-9);
    assert_eq!(r.used_samples, 1);
    // Untouched cells: below min_weight ⇒ unchanged, confidence 0.
    assert_eq!(r.cells[3].proposed, 50.0);
    assert_eq!(r.cells[3].confidence, 0.0);
}

#[test]
fn max_delta_clamp_engages() {
    // 10 identical strong-lean samples: confidence 0.5 → blended 70 → clamped to 57.5.
    let s = samples(vec![vec![1000.0, 20.0, 29.4, 100.0]; 10]);
    let r = ve_analyze(&s, &flat_grid(50.0), &flat_grid(14.7), &binding(), &params(0)).unwrap();
    assert!((r.cells[0].proposed - 57.5).abs() < 1e-9); // 50 ± 15%
}

#[test]
fn mid_cell_sample_splits_weight_and_stays_below_min_weight() {
    let s = samples(vec![vec![1500.0, 30.0, 29.4, 100.0]]); // 4 × w=0.25 < min_weight 1.0
    let r = ve_analyze(&s, &flat_grid(50.0), &flat_grid(14.7), &binding(), &params(0)).unwrap();
    assert!(r.cells.iter().all(|c| c.proposed == 50.0 && c.confidence == 0.0));
    assert!((r.cells[0].hit_weight - 0.25).abs() < 1e-12);
    assert_eq!(r.cells[0].sample_count, 1, "max-weight tie breaks to lowest index");
}

#[test]
fn filters_reject_in_declared_order_and_are_all_reported() {
    let mut b = binding();
    b.filters.push(FilterSpec::Custom {
        id: "minCltFilter".into(), label: "Minimum CLT".into(),
        channel: "coolant".into(), op: FilterOp::Lt, value: 60.0,
    });
    let mut s = samples(vec![
        vec![500.0, 30.0, 14.7, 100.0],  // rpm below x-axis → std_xAxisMin
        vec![1500.0, 30.0, 0.0, 100.0],  // dead lambda
        vec![1000.0, 20.0, 14.7, 100.0], // survives
    ]);
    s.columns.push("coolant".into());
    for row in &mut s.rows {
        row.push(90.0); // warm coolant — the CLT filter never fires
    }
    let r = ve_analyze(&s, &flat_grid(50.0), &flat_grid(14.7), &b, &params(0)).unwrap();
    assert_eq!(r.used_samples, 1);
    let count = |id: &str| r.filtered.iter().find(|f| f.id == id).unwrap().count;
    assert_eq!(count("std_xAxisMin"), 1);
    assert_eq!(count("std_DeadLambda"), 1);
    assert_eq!(count("minCltFilter"), 0, "reported even at zero");
    assert_eq!(count("nonFinite"), 0);
}

#[test]
fn lag_pairs_afr_with_the_earlier_operating_point() {
    // Row 0 sits on cell 0; row 1 has moved to cell 3 but carries the lean afr.
    // With lag=1 the lean reading must credit CELL 0, not cell 3.
    let s = samples(vec![
        vec![1000.0, 20.0, 14.7, 100.0],
        vec![2000.0, 40.0, 29.4, 100.0],
    ]);
    let r = ve_analyze(&s, &flat_grid(50.0), &flat_grid(14.7), &binding(), &params(1)).unwrap();
    assert!(r.cells[0].proposed > 50.0, "correction lands on the lagged point");
    assert_eq!(r.cells[3].proposed, 50.0);
    assert_eq!(r.total_samples, 2);
    assert_eq!(r.used_samples, 1, "lag consumes one pairing");
}

#[test]
fn ego_neutralization_folds_the_trim_in_and_center_zero_disables() {
    // The ECU already trimmed +10% (ego 110): even at afr == target the table must rise.
    let s = samples(vec![vec![1000.0, 20.0, 14.7, 110.0]]);
    let r = ve_analyze(&s, &flat_grid(50.0), &flat_grid(14.7), &binding(), &params(0)).unwrap();
    assert!(r.cells[0].proposed > 50.0);
    let mut p = params(0);
    p.ego_center = 0.0; // disabled → afr == target → factor 1 → no change
    let r2 = ve_analyze(&s, &flat_grid(50.0), &flat_grid(14.7), &binding(), &p).unwrap();
    assert_eq!(r2.cells[0].proposed, 50.0);
}

#[test]
fn same_input_is_bitwise_identical() {
    // 200 pseudo-varied samples built DETERMINISTICALLY (no RNG in tests either).
    let rows: Vec<Vec<f64>> = (0..200)
        .map(|i| {
            let f = i as f64;
            vec![1000.0 + (f * 37.0) % 1000.0, 20.0 + (f * 13.0) % 20.0,
                 13.0 + (f * 7.0) % 4.0, 98.0 + (f % 5.0)]
        })
        .collect();
    let s = samples(rows);
    let a = ve_analyze(&s, &flat_grid(50.0), &flat_grid(14.7), &binding(), &params(3)).unwrap();
    let b = ve_analyze(&s, &flat_grid(50.0), &flat_grid(14.7), &binding(), &params(3)).unwrap();
    for (ca, cb) in a.cells.iter().zip(&b.cells) {
        assert_eq!(ca.proposed.to_bits(), cb.proposed.to_bits());
        assert_eq!(ca.confidence.to_bits(), cb.confidence.to_bits());
        assert_eq!(ca.hit_weight.to_bits(), cb.hit_weight.to_bits());
    }
    assert_eq!(a.filtered, b.filtered);
}

#[test]
fn missing_binding_channel_is_a_hard_error() {
    let s = SampleSet { columns: vec!["rpm".into()], t_ms: vec![0.0], rows: vec![vec![1.0]] };
    assert!(matches!(
        ve_analyze(&s, &flat_grid(50.0), &flat_grid(14.7), &binding(), &params(0)),
        Err(AnalyzeError::MissingChannel(n)) if n == "map"
    ));
}
```

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-analysis`
Expected: FAIL (stub returns `Err(EmptyTable)`).

- [ ] **10.3 Implement `ve_analyze`** in `src/ve_analyze.rs` exactly per the pinned
  algorithm (move the stub out of `lib.rs`; `pub use ve_analyze::ve_analyze;`). Keep the
  file <400 lines with private fns `resolve_columns`, `filter_reason` (returns
  `Option<usize>` into the pre-built `FilterCount` table), `accumulate`, `finalize`.
  Run 10.1 + 10.2 → GREEN.

- [ ] **10.4 Clippy + suite; commit.**

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-analysis && cargo clippy -p opentune-analysis -- -D warnings`

```bash
git add -A
git commit -m "feat(analysis): deterministic ve_analyze — bilinear hits, EGO neutralization, lag, filters, per-cell confidence"
```

---

## Task 11 — `run_ve_analyze` command + AutoTune UI *(roadmap: auto-tune with visible filtering + confidence)*

**Files:**
- Create: `src-tauri/src/analysis_bridge.rs` (Definition/Tune/SampleSet → report),
  `src/components/autotune/AutoTunePanel.tsx`, `src/components/autotune/autotune.css`
- Modify: `src-tauri/src/owner.rs` (fill the `RunVeAnalyze` arm),
  `src-tauri/src/analysis_commands.rs` (+`run_ve_analyze`),
  `src-tauri/src/dto.rs` (`DefinitionDto.analyze_tables`), `src-tauri/src/lib.rs`
  (register + needles), `src/components/table-editor/TableEditor.tsx` (mount the panel),
  i18n dicts
- Test: `analysis_bridge.rs` unit tests, `src/components/autotune/AutoTunePanel.test.tsx`

**Interfaces:**
- Consumes: `VeAnalyzeDef`/`TableDef` (Tasks 0/2), `Tune::get` (grids from the live tune),
  `CaptureBuffer::to_sample_set` (Task 8), `ve_analyze` (Task 10), `TableGrid`'s
  `readOnly` + `cellTitle` props (Task 5), `useTuneStore.setCells` (Task 5).
- Produces:

```rust
// analysis_bridge.rs — pure wiring, unit-testable without an owner/connection
/// Resolve everything the engine needs from the parsed definition + live tune
/// and run it. `table` is a [TableEditor] id with a [VeAnalyze] map.
pub fn run_ve_analyze(
    def: &opentune_ini::Definition,
    tune: &opentune_model::Tune,
    samples: &opentune_analysis::SampleSet,
    table: &str,
) -> Result<VeAnalysisReportDto, String>;
```

Command: `runVeAnalyze(table: string) -> VeAnalysisReportDto`. DTO addition:
`DefinitionDto.analyze_tables: Vec<String>` — the `[TableEditor]` ids that carry a
`[VeAnalyze]` map (the frontend's "show AutoTune here" signal).

> Bridge resolution rules (each miss = a clear `Err(String)`): map =
> `def.ve_analyze.as_ref().and_then(|v| v.maps.iter().find(|m| m.table == table))`
> (miss → `"no [VeAnalyze] map for table …"`); VE `TableDef` = `def.table(&map.table)`,
> target `TableDef` = `def.table(&map.target_table)`; each grid via `tune.get` on that
> table's `x_bins`/`y_bins`/`z` (each must decode to `Value::Array`); binding channels =
> the VE table's `x_channel`/`y_channel` (must be non-empty — the INI names them) + the
> map's `lambda_channel`/`ego_channel`; filters compile `AnalyzeFilterDef` → `FilterSpec`
> (`std_xAxisMin/Max`, `std_yAxisMin/Max` → the axis variants; `std_DeadLambda` →
> `DeadLambda`; any other `std_*` incl. `std_Custom` is skipped — recorded-deferred;
> `Custom` maps field-for-field, `ini::FilterOp` → `analysis::FilterOp`). Params =
> `VeAnalyzeParams::default()` (parameterizing over IPC is M5+; the report is already
> fully auditable). The owner arm runs the bridge **inline** — pure compute over ≤27k
> rows, a few ms; the spawn_blocking rule exists for wire/disk I/O.

- [ ] **11.1 Failing bridge test** (`#[cfg(test)]` in `analysis_bridge.rs`): parse the
  upgraded sample INI (`include_str!("../resources/speeduino.sample.ini")`), build a `Tune`
  the way the model tests construct theirs (mirror their `Tune` fixture constructor), seed
  `rpmBins` `[500, 1000, …, 8000]` (as `Value::Array`), `fuelLoadBins` `[20, 25, …, 95]`,
  flat-50 `veTable`, flat-14.7 `afrTable` + the AFR-side bins; hand-build a `SampleSet`
  with columns `["rpm", "map", "afr", "egoCorrection", "coolant"]` and 30 rows sitting on
  one cell with `afr = 16.17` (10 % lean), `ego = 100`, `coolant = 90`; call
  `run_ve_analyze(&def, &tune, &samples, "veTable1Tbl")`. Assert `Ok`, `x_len == 16`,
  the hit cell's `proposed > current`, `filtered` contains `minCltFilter` with `count 0`,
  and an unknown id errors with a message containing `no [VeAnalyze] map`. Run → RED;
  implement the bridge per the resolution rules (+ the `VeAnalysisReport` → DTO mapping
  with the `table` echo) → GREEN.

- [ ] **11.2 Owner arm + command + DTO.** Replace the Task 0 stub:

```rust
            Command::RunVeAnalyze { table, reply } => {
                let r = match (&self.session, &self.capture) {
                    (Some(s), Some(buf)) => {
                        let samples = buf.to_sample_set();
                        s.tune.as_ref()
                            .ok_or_else(|| "no tune loaded".to_string())
                            .and_then(|t| crate::analysis_bridge::run_ve_analyze(
                                &s.def, t, &samples, &table,
                            ))
                    }
                    (None, _) => Err("not connected".to_string()),
                    (_, None) => Err("no capture — start a capture first".to_string()),
                };
                let _ = reply.send(r);
            }
```

`analysis_commands.rs` gains:

```rust
#[tauri::command]
#[specta::specta]
pub async fn run_ve_analyze(
    table: String,
    owner: State<'_, OwnerHandle>,
) -> Result<VeAnalysisReportDto, String> {
    request(&owner, |reply| Command::RunVeAnalyze { table, reply }).await
}
```

`dto.rs`: add `pub analyze_tables: Vec<String>` to `DefinitionDto`; in
`From<&Definition>`:
`analyze_tables: def.ve_analyze.iter().flat_map(|v| v.maps.iter().map(|m| m.table.clone())).collect(),`.
Register `run_ve_analyze` in `collect_commands!`; needles `"runVeAnalyze"`,
`"VeAnalysisReportDto"`, and the generated `analyze_tables` field name (check
`bindings.ts` for the exact casing specta emits and pin that string).

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test && cargo clippy --workspace -- -D warnings` → PASS.

- [ ] **11.3 Failing `AutoTunePanel` test:** mock bindings (`startCapture`/`stopCapture`/
  `captureStatus`/`runVeAnalyze` + `setCells`); seed the store with a sample-INI-shaped
  definition (incl. `analyze_tables: ["veTable1Tbl"]`) and 2×2 z values; the
  `runVeAnalyze` mock resolves a 2×2 report with cells
  `[{current:50, proposed:55, delta_pct:10, hit_weight:8, sample_count:6, confidence:0.8},
  {current:50, proposed:58, delta_pct:16, hit_weight:2, sample_count:1, confidence:0.2}, …]`
  and `filtered: [{id:"std_DeadLambda", label:"std_DeadLambda", count:3}]`. Assert:
  Analyze click renders the delta grid (a second `role="grid"` showing `10.0`-style
  deltas) + the filtered list (`std_DeadLambda — 3`); with the default threshold 0.5,
  Apply calls `commands.setCells("veTable", [{ index: 0, value: 55 }])` — the
  0.2-confidence cell is **excluded**; capture Start/Stop call their commands and render
  `sample_count` from the returned status. Run → RED.

- [ ] **11.4 Implement `AutoTunePanel.tsx`** (~200 lines) and mount it at the bottom of
  `TableEditor` when `definition.analyze_tables.includes(table.name)`:
  props `{ locale, table: TableDto, zName: string, rows: number, cols: number }`. State:
  `status`, `report`, `threshold` (default `0.5`), `busy`, `error`. Controls row:
  Start/Stop capture buttons (each refreshes `status` from its command result), a 1 s
  `setInterval` polling `commands.captureStatus()` **only while** `status?.capturing`
  (cleared on unmount — this is a 1 Hz status poll, not a hot path), the sample counter,
  and Analyze (`runVeAnalyze(table.name)` → `setReport`). Report block: a read-only
  `TableGrid` (`gridId="autotune"`) with `values = report.cells.map((c) => c.delta_pct)`,
  `digits: 1`, symmetric heat range `[-maxAbs, +maxAbs]` where
  `maxAbs = Math.max(1, ...report.cells.map((c) => Math.abs(c.delta_pct)))` (0 sits
  mid-scale), and
  `cellTitle = (i) => { const c = report.cells[i]; return
  `${c.current.toFixed(1)} → ${c.proposed.toFixed(1)} · conf ${c.confidence.toFixed(2)} · w ${c.hit_weight.toFixed(1)} · n ${c.sample_count}`; }`
  — "which logged data drove each cell", per cell; the filtered `<ul>` (every row,
  zeros included — the ROADMAP's visible-filtering requirement); the threshold
  `<input type="number" min="0" max="1" step="0.05">`; Apply →
  `edits = report.cells.flatMap((c, i) => c.confidence >= threshold && c.proposed !== c.current ? [{ index: i, value: c.proposed }] : [])`
  → `useTuneStore.getState().setCells(zName, edits)` (one gesture — one undo step reverts
  the whole apply). i18n additions (pl mirrors, translated):

```ts
  "autotune.title": "AutoTune (VE analyze)",
  "autotune.startCapture": "Start capture",
  "autotune.stopCapture": "Stop capture",
  "autotune.samples": "Samples",
  "autotune.analyze": "Analyze",
  "autotune.apply": "Apply proposed",
  "autotune.threshold": "Min confidence",
  "autotune.filtered": "Filtered samples",
  "autotune.noReport": "Run an analysis to see proposed corrections",
```

Run 11.3 → GREEN.

- [ ] **11.5 Full gates + commit.**

Run: `npm test && npm run lint && npm run format:check && npm run build && . "$HOME/.cargo/env" && cd src-tauri && cargo test`

```bash
git add -A
git commit -m "feat(app): AutoTune UI — capture controls, ve_analyze report (delta+confidence heatmap, visible filtering), apply via set_cells"
```

---

## Task 12 — End-to-end demo: tune the VE table and flatten the sim's error *(the M4 demo)*

**Files:**
- Modify: `src-tauri/src/owner_tests.rs` (the demo E2E), `docs/ROADMAP.md` (tick M4),
  `docs/notes/m4-decisions.md` (final entries)
- No new production files.

**Interfaces:** consumes everything from Tasks 0-11 wired together.

- [ ] **12.1 Read the owner-test harness first** (`owner_tests.rs`, ~680 lines): note its
  emitter shim, the `Connect { Simulator }` setup, the test-only `DebugSimulator` command
  (handle to `EcuSimulator`), and how the M3 realtime E2E drives ticks/sleeps — the new
  test reuses those helpers verbatim. (The poller gates on `std::time::Instant`, so
  tokio's paused clock does **not** apply — this test uses real sleeps and runs ~8-12 s;
  keep it in the normal suite like the M3 E2E and note the duration in a comment.)

- [ ] **12.2 The demo E2E `ve_analyze_flattens_the_sim_ve_error`** (`#[tokio::test]`) —
  write it with the harness's real helper names, implementing exactly this script:

```rust
// Phase 0 — connect + seed a deliberately-wrong tune.
//   Connect { Simulator (bundled sample INI) } → LoadTune.
//   SetValue "rpmBins"      = Array[500, 1000, 1500, …, 8000]           (16)
//   SetValue "fuelLoadBins" = Array[20, 25, …, 95]                      (16)
//   SetValue "veTable"      = Array[50.0; 256]   // flat — the "wrong" table
//   SetValue "afrTable"     = Array[14.7; 256]
//   SetValue "rpmBinsAFR" / "loadBinsAFR" = the same bins
//   Burn.
// Phase 1 — capture a live session.
//   StartRealtime → StartCapture.
//   Drive ~120 windows: sim.tick_engine(Duration::from_millis(50)) then
//   tokio::time::sleep(Duration::from_millis(40)) each (the sim also
//   auto-ticks on every 'r' — the step 0.0 fix).
//   StopCapture → assert sample_count >= 80.
// Phase 2 — analyze: RunVeAnalyze("veTable1Tbl") → report1.
//   assert report1.used_samples > 0;
//   assert at least one cell with confidence >= 0.3;
//   for every such cell (flat index i, x = i % 16, y = i / 16):
//     let want = (40.0 + 25.0 * (load_bins[y] / 100.0)
//                 + 15.0 * (rpm_bins[x] / 6000.0)).clamp(20.0, 110.0);
//     if (want - 50.0).abs() > 1.0 {
//         assert_eq!(report1.cells[i].delta_pct > 0.0, want > 50.0,
//             "correction must point toward the sim's true-VE surface");
//     }
// Phase 3 — apply + re-measure.
//   edits = confident cells (>= 0.3, proposed != current) → SetCells("veTable", …) → Burn.
//   StartCapture (fresh ring) → drive ~120 windows again → RunVeAnalyze → report2.
//   fn mean_abs(r: &VeAnalysisReportDto) -> f64 {
//       let confident: Vec<f64> = r.cells.iter()
//           .filter(|c| c.confidence >= 0.3)
//           .map(|c| c.delta_pct.abs()).collect();
//       if confident.is_empty() { 0.0 }
//       else { confident.iter().sum::<f64>() / confident.len() as f64 }
//   }
//   assert!(mean_abs(&report2) < 0.5 * mean_abs(&report1),
//       "applying the analysis must flatten the seeded VE error");
```

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test ve_analyze_flattens -- --nocapture`
Expected: GREEN. **This is the M4 demo:** "tune a VE table in 2D/3D and apply a
data-driven correction, with a clear view of which logged data drove each cell" — the
report's per-cell hits/confidence + the filtered counts are that view.

- [ ] **12.3 Manual `tauri dev` GUI smoke run** (step, not a task): `npm run tauri dev` →
  connect to the simulator → open **VE Table** from the Tables nav → edit cells (type,
  `+`/`-`, drag-select + interpolate/smooth, copy into a spreadsheet, paste back) →
  undo/redo → toggle **3D**, orbit the surface, click **Start live** on the dashboard and
  confirm the white dot rides the surface → open **Warmup Enrichment** from Curves, edit a
  value, watch the polyline follow → in **AutoTune**: Start capture, wait ~30 s, Stop,
  Analyze — confirm the delta heatmap + filtered list render, Apply at the default
  threshold, re-capture + re-analyze and watch the deltas collapse → Burn. Record the
  observed behavior (and the two 7.6 bundle sizes) in the commit body.

- [ ] **12.4 Docs closeout.** `docs/ROADMAP.md`: tick the four M4 bullets (⬜→✅, M3-style
  one-line summaries — DOM-grid editor; lazy-three surface + live dot; curve editors;
  deterministic `ve_analyze` in the new `opentune-analysis` crate fed by the owner capture
  ring) + the demo line. `docs/notes/m4-decisions.md`: ensure every locked decision that
  shifted during implementation has an entry (minimum: the golden-gate allowlist's final
  contents; any `lastOffset` fixture corrections; the tap-above-the-gate capture
  invariant; the `default_on` filter-flag semantics note; deferred: `lambdaTargetTables`,
  `[WueAnalyze]`, axis-bin editing, paste-special, `std_Custom`, DTO-level analyze params;
  M3-serial follow-ups remain out).

- [ ] **12.5 The full gate.**

Run: `npm test && npm run lint && npm run format:check && npm run build && . "$HOME/.cargo/env" && cd src-tauri && cargo test && cargo clippy --workspace -- -D warnings && cargo fmt --check`
Expected: PASS.

- [ ] **12.6 Commit.**

```bash
git add -A
git commit -m "test(app): E2E VE auto-tune demo — capture, analyze, apply, error flattens (M4)"
```

---

## Self-review

**Spec coverage (ROADMAP M4 bullets + dossier decomposition + M3 follow-ups):**
- 2D heatmap table editor (interpolate, smooth, scale, copy/paste, keyboard) → Tasks 4+5
  (pure ops/TSV/selection core + DOM grid with the full pinned keymap). ✅
- 3D surface view (three.js) with live operating-point overlay → Task 7 (lazy chunk, rAF
  dot off the M3 realtime store, WKWebView pins, measured budget gate). ✅
- Curve editors (1D) → Task 6 (grid-machinery reuse + SVG preview + live cursor). ✅
- First auto-tune, deterministic + auditable, per-cell confidence, visible filtering →
  Task 0 (seams) + 10 (engine + determinism tests) + 11 (report UI). The crate lands in M4
  as `opentune-analysis` (dispatch-locked reading of "the `analysis` crate lands in M5":
  M4 seeds it with `ve_analyze` only; M5 grows it — recorded in decision 3). ✅
- Demo ("tune a VE table in 2D/3D and apply a data-driven correction, with a clear view
  of which logged data drove each cell") → Task 12 (E2E proves the error flattens;
  hits/confidence/filtered are the view) + 12.3 manual smoke. ✅
- Dossier sketch items map: parser prerequisites → Task 1; richer table/curve model →
  Task 2; cell-write seam → Task 3; table ops → Task 4 (frontend — deliberate dispatch
  override of the dossier's Rust-core recommendation, recorded in decision 7); 2D editor
  → 5; curves → 6; 3D → 7; capture ring → 8; simulator → 9; `ve_analyze` → 10; AutoTune
  UI → 11. ✅
- M3 follow-ups: auto-tick fix folded (0.0); `get_values` unknown-name NaN already closed
  in M3 (recorded); `pages.rs`/`constants_fields.rs` splits not fired (no protocol
  addition; Task 1 is a localized fix) — recorded in decision 12; serial-only follow-ups
  excluded as M3-serial. ✅

**Placeholder scan:** no TBD/TODO/"add error handling"/"similar to Task N". Every step
carries code, exact commands with expected output, or a named-mirror instruction pointing
at a specific artifact that verifiably exists at HEAD (the model test fixture, the
`SetValue` owner arm, the owner-test harness, the session `set_value` tests) — the M3
cross-check pattern. `todo!()` appears only in Task 0 stub bodies (frozen-contract
pattern, M2/M3 precedent). Task 1.7's iterate-until-green is a gate with an explicit
per-failure-class triage rule and hard, non-weakenable acceptance — not an open end.

**Type consistency (checked across tasks):** `TableDef`/`CurveDef`/`CurveAxis` fields (0.2
↔ 2.3 ↔ 2.7 DTOs); `Tune::set_cells(&mut self, &str, &[(u32, f64)])` (0.5 ↔ 3.2 ↔ 3.3 ↔
3.4); `CellEditDto { index: u32, value: f64 }` (0.6 ↔ 3.4 ↔ 5.2 ↔ 11.4);
`SampleSet { columns, t_ms, rows }` (0.4 ↔ 8.1 ↔ 10.2 ↔ 11.1); `FilterSpec`/`FilterOp`
duplicated ini↔analysis **by design** (zero-dep crate; mapped only in the 11.1 bridge);
`CaptureStatusDto { capturing, sample_count, duration_ms, dropped }` (0.6 ↔ 8.1-8.4 ↔
11.4); `heatRgb` (4) → `surfaceColors` (7); frontend `Grid`/`CellEdit`/`Rect`/`Selection`
shared by 4/5/6/11; `VeAnalysisReport{,Dto}` field-for-field + the `table` echo (0 ↔ 10 ↔
11); `TableGrid` props `readOnly`/`cellTitle` declared in 5.4 and consumed in 11.4.
`DefinitionDto.analyze_tables` is additive in Task 11 (same additive class as Task 2's
`curves` — not a frozen-seam break).

**Ported-source ledger (ADR-0006):**
- Task 2 richer `[TableEditor]`/`[CurveEditor]` → **PORT** `hyper-tuner/ini` +
  `hyper-tuner/types` (MIT © 2021 Piotr Rogowski; copyright line kept in the module
  header — the M3 license lesson). `x_channel`/`y_channel` capture = recorded extension.
- Task 1 comms-scatter + `lastOffset`; Task 2 `[VeAnalyze]`; Task 3 `set_cells`; Tasks
  4-7 editors/3D; Task 8 capture; Task 9 VE-error surface; Task 10 `ve_analyze` → **WRITE
  FRESH** (`speeduino.ini`/firmware GPL-3 as semantics truth source; TS/MLV/MS-Extra
  behavioral references only — no code; LibreTune GPL-2 + hypertuner GPL study-only;
  hypertuner-cloud MIT read-only — nothing liftable for editing).
- Vendored fixture: real `speeduino.ini` @ `0832dc1d…d35a` (GPL-3 → this GPL-3-or-later
  repo, byte-identical, provenance in the test module header).
- `three` (MIT) — the ROADMAP-sanctioned dependency; `@types/three` dev-dep rides the
  same allowance (recorded in Global Constraints).

**Top-5 risks & mitigations:**
1. **The golden gate uncovers wall #3+** (PcVariables classes, grammar corners across
   6 026 real lines). Mitigation: Task 1.7's per-failure-class triage loop sits *before*
   every UI task; the allowlist only grows together with an m4-decisions entry; the
   non-diagnostic assertions may never be weakened.
2. **`lastOffset` semantics change regresses an M2 fixture** that pinned end-semantics.
   Mitigation: 1.5 greps all fixtures and corrects expectations with a truth-source note;
   the real-file test (all five aliases) is the arbiter; `Definition`'s shape is untouched.
3. **three.js defeats code-splitting** (one stray static import → +175 kB gz eager).
   Mitigation: a single `React.lazy` boundary; only `SurfaceView.tsx` may import three;
   7.6 measures both chunks against pinned byte budgets and greps for illegal imports.
4. **`ve_analyze` correctness drift** (EGO double-count, lag misattribution, float-order
   nondeterminism). Mitigation: the pinned normative algorithm + exact-number unit tests
   (52.0 / 57.5), a lag-misattribution test, ego on/off tests, a bitwise-identity
   determinism test, no HashMap/parallelism by construction — and the 12.2 E2E closes the
   loop against an independent ground truth (the sim's `true_ve`).
5. **Owner/capture coupling rot** (the ≤30 Hz coalescing gate silently starving the ring
   if poll rates ever change). Mitigation: the tap-above-the-gate invariant is documented
   in `capture.rs` and pinned by `capture_rate_pins_the_tap_invariant` (8.4) — a future
   rate change breaks a named test, not the feature.

**Parallelism map** (informational — execution is **sequential in the main checkout**,
single implementer, the M2/M3 precedent): Task 0 gates everything (0.0 cherry-pick first);
Task 1 gates 2 (golden gate) and is the declared prerequisite for all editor UI; 2 gates
{5, 6, 9, 11}; 3 gates {5, 11}; 4 gates {5, 6, 7, 11}; 5 gates {6, 7, 11} (grid/store
reuse); 8 gates {11, 12}; 9 gates 12 (and makes 11 demo-able by hand); 10 gates {11, 12};
12 integrates. Independent groups, were parallelism ever wanted: {2, 3, 4}, {7, 8},
{9, 10}.

**Estimated commits:** 16 — the 0.0 cherry-pick, T0, T1×2, T2, T3, T4, T5, T6, T7, T8,
T9, T10, T11, T12, plus this plan document.

