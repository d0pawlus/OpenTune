# M4 — Table editors & auto-tune: research dossier

> **Research artifact** (2026-07-04, branch `m4-table-editors`), not a design doc.
> Input for the M4 implementation plan. Primary sources: this repo at HEAD;
> `noisymime/speeduino@0832dc1d` (current `master` tip — M2/M3 pinned the older
> `63fd68e9`; the findings below are structural and SHA-independent, but M4
> golden fixtures should re-pin a fixed SHA), GPL-3 truth source for INI grammar
> and VE-analyze semantics; `hyper-tuner/ini` + `hyper-tuner/types` (MIT, our
> licensed porting source) and `hypertuner-cloud` (MIT, read-only viewer);
> `hyper-tuner/hypertuner` and `RallyPat/LibreTune` (GPL-2/GPL-3 — **study only**,
> ADR-0007); `askrejans/speeduino-serial-sim` (MIT, the M3 sim port); EFI
> Analytics MegaLogViewer VE-Analysis help, MS Extra tuning manual (behavioral
> references — proprietary, no code). Every claim carries a `file:line` or URL.

## A. INI `[TableEditor]` / `[CurveEditor]` grammar and the parser gap

Reference: `speeduino.ini` @ 0832dc1d (6 026 lines; `signature "speeduino 202504-dev"`,
iniSpecVersion 3.64). `[TableEditor]` starts l. 4935, `[CurveEditor]` l. 4621,
`[VeAnalyze]` l. 5984.

### `[TableEditor]` grammar (VE table, l. 4935-4948 verbatim)

```ini
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
```

`table = <editorId>, <map3dId>, "<title>", <page>`. The **second token of
`xBins`/`yBins`** (`rpm`, `fuelLoad`) is a live `[OutputChannels]` variable — it
positions the moving cursor drawn on the map from realtime data (the whole point
of the M4 operating-point overlay, §D). `zBins` is the cell array. `gridOrient`
= 3D view rotation; `upDownLabel` = drag hint; `topicHelp` = wiki URL. **`#if`
directives appear mid-block** around individual attributes (e.g. `#if CELSIUS`
around one curve's `xAxis`, `#if LAMBDA` around a `yAxis`) — the parser must run
the preprocessor, not line-scan.

### Backing data lives in `[Constants]` — axis bins are *separate* arrays (l. 493-495)

```ini
page = 2
  veTable      = array, U08,   0, [16x16], "%",   1.0, 0.0, 0.0, 255.0, 0   ; bytes 0..255
  rpmBins      = array, U08, 256, [16],  "RPM", 100.0, 0.0, 100.0, 25500.0, 0 ; bytes 256..271
  fuelLoadBins = array, U08, 272, [16],  { bitStringValue(algorithmUnits, algorithm) }, {fuelLoadRes}, 0.0, 0.0, {fuelLoadMax}, {fuelDecimalRes}
```

A "table" is **three constants**: a `[RxC]` z-array plus two `[N]` axis arrays,
coupled only by name in `[TableEditor]`. Note **expression-valued attributes**
(`{fuelLoadRes}`, `{ 0.1 / stoich }`, `{ bitStringValue(...) }`) in the
scale/units/min/max fields — already captured as `Number::Expr` by
`constants_fields.rs:117` (`parse_number`).

### `[CurveEditor]` grammar (l. 4718-4726 verbatim)

```ini
curve = pwm_fan_curve, "Fan PWM Duty"
    columnLabel = "Temp", "Duty %"
    xAxis = -40, 215, 4          ; min, max, gridDivisions
    yAxis = 0, 100, 4
    xBins = fanPWMBins, coolant  ; array + live cursor channel
    yBins = PWMFanDuty           ; the editable data array
    gauge = cltGauge             ; a [GaugeConfigurations] gauge shown alongside
    size  = 400, 400             ; widget px
```

### What M2/M3 already ingests, and the precise gap

`ui_table_curve_parser.rs` (M3) parses the `table`/`curve` headers into
`TableDef { name, x_bins, y_bins, z }` and `CurveDef { name, x_bins, y_bins }`
(ui.rs). It **keeps only the first bin token and the z name**; it explicitly
drops (`_ => {}`, l. 48/115) the axis-display channel (2nd token), `xyLabels`,
`gridHeight`, `gridOrient`, `upDownLabel`, `topicHelp`, and the curve's
`columnLabel`/`xAxis`/`yAxis`/`gauge`/`size`. Unknown bin names degrade to a
`Diagnostic` and keep the raw name (l. 77-85). **Gap for M4:**

1. `TableDef`/`CurveDef` carry no `page`, no **axis-display channel** (needed for
   the live overlay), no labels, no grid geometry, no help, no curve axis ranges.
   The editor also needs the z array's *shape* — resolvable from the referenced
   `ConstantDef` (`ConstantKind::Array { shape }`), not stored on the table.
2. `[VeAnalyze]`/`[WueAnalyze]` (§E) are unparsed — the data-driven analyze binding.
3. `updateOnBurn` is **absent** in speeduino.ini (0 occurrences) — *not* a gap;
   record "do not build logic assuming it."
4. `groupMenu` (1 occurrence, l. 2014, carried M2 blocker) still unmodeled.

**Port target (ADR-0006):** `hyper-tuner/ini` `parseTables`/`parseCurves` (MIT,
Piotr Rogowski 2021 — LICENSE confirmed) produce the *richer* shape we're
missing: `Table { map, title, page, help?, xBins[], yBins[], zBins[], xyLabels[],
gridHeight, gridOrient[], upDownLabel[] }`, `Curve { title, labels[], xAxis[],
yAxis[], xBins[], yBins[], size[], gauge? }` (`hyper-tuner/types`
`src/types/config.ts:153-176`). Bins are stored as **raw string names** (lazy —
resolved by the consumer against `[Constants]`), the id is the object key, and
the axis-channel `[1]` token is captured-but-unused there. Our `TableDef` is a
strict subset; porting the fuller field set closes gap #1. MIT → portable.

## B. Current model-crate table support; the aliasing blocker

### What exists (model @ HEAD)

`Tune` (`model/tune.rs`) holds one byte buffer per page; `get(name)`/`set(name,
Value)` decode/encode a **whole named constant**. A table's z-array reads/writes
as `Value::Array(Vec<f64>)` (`codec.rs:193/251`, row-major, `raw*scale+translate`,
per-element range check). **Dirty is per page** (`Vec<u16>`, tune.rs:56); **undo
is one `Edit` per `set`** (page+offset+before+after bytes, tune.rs:163). Aliased
constants — two `ConstantDef`s at the same page+offset with different scale/units
— **already round-trip**: `get("afrTable")` and `get("lambdaTable")` read the
same bytes at different scale; `set` on either writes the shared bytes; dirty is
per-page; undo is by page+offset. **So aliasing needs no model reshape.**

### The aliasing blocker is parser-local, and it is *not the first wall*

Running `parse_definition` against the unmodified real INI (throwaway probe,
removed) fails **first** at `parse_comms`:

```
PROBE_ERR MissingKey("ochGetCommand")
```

**Wall #1 — comms-key scatter.** `parse_comms` (parser.rs:25-62) requires
`signature`, `queryCommand`, `versionInfo`, `ochGetCommand`, `pageReadCommand`,
`pageValueWrite`, `burnCommand`, `blockingFactor`, `blockReadTimeout` — all from
the first `[MegaTune]`/`[TunerStudio]` section. But the real INI puts only
`signature`/`queryCommand`/`versionInfo` in `[MegaTune]` (l. 4-10);
`pageReadCommand`/`pageValueWrite`/`burnCommand`/`blockingFactor`/
`messageEnvelopeFormat` live inside **`[Constants]`** (l. 244-274); and
`ochGetCommand` lives in **`[OutputChannels]`** (l. 5352). The M3
`extract_och_get_command` override (parser.rs:84) never runs because
`parse_comms` fails before `parse_definition` reaches it. The parse dies here,
never touching tables.

**Wall #2 — `lastOffset` overflow (the aliasing overflow).** With comms patched,
a minimal page-5 fixture reproduces the second wall:

```
ALIAS_ERR InvalidValue { key: "afrTable", detail: "offset 256 + size 256 exceeds page 5 size 288" }
```

Page 5 (l. 640-645) is `lambdaTable @0 [16x16]` (256 B), `afrTable @lastOffset`,
`rpmBinsAFR @256`, `loadBinsAFR @272`; page size 288. The page holds **one**
256-byte table (viewed as Lambda *or* AFR) + two 16-byte axes = 288. So `afrTable`
must **alias** `lambdaTable` at offset 0. All **5** `lastOffset` uses in the file
are AFR↔Lambda aliases (`afrTable`/`ego_min_lambda`/`ego_max_lambda`/
`afrProtectDeviation`/`n2o_maxLambda`), each overlaying the immediately-preceding
field — i.e. **TS `lastOffset` = the *start* offset of the previous variable**,
unguarded by any `#if`. Our `resolve_offset` (constants_fields.rs:183-196) returns
the running *end* counter, so `afrTable` lands at 256 → `validate_offset_within_page`
(constants_parser.rs:208) hard-errors; and scalar aliases (`ego_min_lambda`)
silently land one byte late (no overflow to catch — a *silent* correctness bug).

**Fix:** track "start of previous field" per page; `lastOffset` → that. Localized
to `constants_fields.rs`; no other regression (all 5 uses are aliases). Downgrades
the feared "aliased-table model reshape" — the model already supports it.

### The genuine model gap for M4: cell-level writes

`Tune::set` encodes the **whole** array (a 256-byte write per single-cell edit)
and records one coarse page-level `Edit`. Over serial that's heavy write
amplification, and undo/redo granularity is a whole table. **Reshape needed:** a
`set_cell` / array-sub-range write seam (or a command-layer element-diff) so a
single-cell edit writes minimal bytes and undo is per-cell — reusing the existing
validate-on-clone → protocol write → commit path. `ConstantDef`/`Value`/aliasing
stay **frozen**; the write/undo path is what reshapes.

## C. Frontend 2D heatmap editor

**Grid substrate — recommend DOM, not canvas (open decision vs ARCHITECTURE §3).**
16×16 = 256 cells is trivial for a semantic `<table role="grid">`, which gives
keyboard nav, TSV clipboard, and ARIA/a11y *for free* (the web ruleset mandates
keyboard-first + semantic HTML). M3's canvas verdict was for 30 Hz *gauge*
redraw — a different problem. ARCHITECTURE §3 records "canvas for 2D grids";
surface this as an explicit exception (small static grid → DOM) or a decision for
the planner. Reserve canvas/WebGL for the **live operating-point overlay** and the
**3D surface**. The M2 `TableField.tsx` (52 lines, read-only DOM grid sized from
the z-array shape) is the seed.

**Where ops run — Rust core.** LibreTune (GPL-2, study-only, *same* Tauri v2 +
Rust + React + Vite stack) puts interpolate/smooth/scale/rebin in its Rust core
and calls them via Tauri commands; the React grid is a thin interaction+render
layer. Match that (and ARCHITECTURE §5.9's determinism culture): bilinear
interpolate, Gaussian smooth (`w = exp(-d²/2σ²)`, kernel radius param), scale
(× factor), set-equal (= selection average) live in `opentune-analysis` (§E),
unit-tested, shared by 2D and 3D. Frontend edits reuse the M2 wire path
(`tune.ts:setValue` optimistic → `commands.setValue` → protocol write →
`tune_dirty` event), extended with the per-cell command from §B.

**Interaction model** (LibreTune manual, study-only; it deliberately tracks
TunerStudio, so it doubles as a behavioral spec): selection = click / click-drag
rectangle / Ctrl-toggle / Shift-extend; keyboard = arrows, Shift+arrows extend,
Ctrl+A, Tab, Home/End, Esc; edit = type-then-Enter, `+`/`-` by step (Shift =
10×); bulk hotkeys `=` set-equal, `*` scale, `/` interpolate, `S` smooth;
**clipboard** Ctrl+C/V/X + a **Paste Special** (Replace/Add/Multiply/Average) with
**TSV** rows for Excel/TunerStudio interop.

**Clipboard path — no new dep.** Current deps are minimal (`@tauri-apps/api`,
`plugin-opener`, `react`, `react-dom`, `zustand`; one `capabilities/default.json`).
Use `navigator.clipboard.readText/writeText` for TSV — no dependency, no capability
entry — vs `@tauri-apps/plugin-clipboard-manager` (dep + capability). Caveat:
WKWebView `readText` may require a user gesture; verify in a real build.

**Nothing to port for editing.** `hypertuner-cloud` (MIT) is a *read-only* viewer
(DOM `<table>` + `colorHsl` heatmap + uPlot curves; no cell inputs, no clipboard,
no interpolate/smooth/scale). Write the interactive layer fresh.

## D. three.js (ROADMAP pre-approved)

**Bundle.** `three@0.182` = 175 kB gz for a full `import * as THREE`
(Bundlephobia); the ROADMAP's "~150 kB gz" is optimistic (that's a tree-shaking
*floor*, ~140-175 realistically). The decisive lever is **lazy-load**, not
tree-shaking: `const THREE = await import('three')` / `React.lazy(() =>
import('./SurfaceView'))` yields a separate Vite chunk (Vite docs). **Trap:** any
static `import … from 'three'` in the eagerly-loaded graph pulls the whole library
back into the main chunk — exactly LibreTune's mistake (it statically imports its
3D view, so three/r3f/drei are *not* code-split). `OrbitControls` ships inside the
package (`three/examples/jsm/controls/OrbitControls`) → a surface mesh + orbit +
RAF loop adds **zero new deps beyond `three`**. **Recommend raw three.js in one
imperative lazy `<SurfaceView>`** over `@react-three/fiber` (no-new-deps bias +
our imperative-canvas fluency; LibreTune chose r3f+drei, but that pays off only
with many 3D scenes).

**WKWebView (macOS Apple-Silicon primary):** #1 pitfall is **Retina blur** —
`devicePixelRatio` can report 1 under a custom URL scheme, halving resolution;
fix with `renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))` (also caps
fill-rate), re-applied on display change. WKWebView shows micro-stutter vs native
Safari (keep the mesh modest, no per-frame allocation). `powerPreference` is a
no-op on Apple Silicon; WebGL2 works (the cited "unavailable" issue is stale 2021).
Register `webglcontextlost`/`webglcontextrestored` handlers. Linux WebKitGTK is
where the real WebGL horror stories live (`WEBKIT_DISABLE_DMABUF_RENDERER=1` etc.,
set conditionally) — note for the Linux lane, not macOS.

**Live operating-point overlay.** The dot at (rpm, fuelLoad) reads the two channels
named by the `[TableEditor]` `xBins`/`yBins` second tokens (`rpm`, `fuelLoad`) —
currently dropped by the parser (§A gap #1), so parse them. Reuse the reflect-only
realtime store + rAF pattern: `GaugeCanvas.tsx:105` reads
`useRealtimeStore.getState().getChannel(...)` imperatively, off React
reconciliation — the same technique drives the surface cursor.

## E. VE analyze — the first `opentune-analysis` capability

**Placement.** Seed `opentune-analysis` as its own crate **now** (ARCHITECTURE
§5.9), not buried in `datalog`. M4 fills only `ve_analyze`; M5 adds
`virtual_dyno`/`log_stats`/`detect_anomaly`. Pure fn:
`ve_analyze(log: &[Sample], current_table, params) -> ProposedTableChange {
per_cell_delta, confidence, sample_count, filtered_reason }`.

**Speeduino ships the authoritative binding** — `[VeAnalyze]` (l. 5984), which M4
should parse rather than hardcode:

```ini
[VeAnalyze]
#if LAMBDA
  veAnalyzeMap = veTable1Tbl, lambdaTable1Tbl, lambda, egoCorrection
#else
  veAnalyzeMap = veTable1Tbl, afrTable1Tbl,   afr,    egoCorrection
  filter = std_DeadLambda ; O2 invalid
  filter = minCltFilter, "Minimum CLT", coolant, <, 71 (°C) / 160 (°F)
  filter = accelFilter,  "Accel Flag" , engine,  &, 16   ; AE transient
  filter = aseFilter,    "ASE Flag"   , engine,  &, 4    ; after-start
  filter = overrunFilter, "Overrun"   , pulseWidth, =, 0 ; DFCO / fuel cut
  filter = std_xAxisMin/Max, std_yAxisMin/Max, std_Custom
```

The write target is **`veTable` (page 2, *not* aliased)**; the AFR-target read
reference is **`afrTable`/`lambdaTable` (page 5, *aliased*)** — keep them distinct.

**Correction math** (MS Extra manual + Speeduino `includeAFR`; EFI Analytics does
*not* publish the formula — treat as well-corroborated, not spec):
`VE_new = VE_old × (AFR_measured / AFR_target)` (lambda form identical, stoich
cancels); measured **lean** (AFR number higher) → ratio > 1 → **raise VE**. If EGO
closed-loop was active during logging, the table must absorb the trim:
`VE_best = VE_old × (1 + egoCorr/100) × (AFR_meas / AFR_target)` (collapses to the
pure ratio at egoCorr ≈ 0). `includeAFR`/`incorporateAFR` flags change whether the
target ratio is already in delivered fuel — read them to avoid double-counting.

**Filters → `filtered_reason` enum:** `DeadLambda`, `MinCLT`, `AccelFlag`
(`engine & 16`), `ASEFlag` (`engine & 4`), `Overrun` (`pulseWidth == 0`),
`AxisOutOfBounds`, `Custom`; plus a fixed integer **wideband-delay** record-shift
(MLV's lag alignment) and optional `EgoTrimTooLarge`.

**Confidence.** Per-cell **Hit Count** (`sample_count`) and **Total Weight** are
*separate* MLV statistics → a sample contributes a fractional, proximity-weighted
amount to the 4 surrounding cells (bilinear), not nearest-cell binning. Confidence
= f(hit count, summed weight, variance) with a deterministic prior-blend toward
`current_table` (MLV's "Cell Change resistance") so sparse cells move predictably.

**Determinism must-pins** (why TS's VE-Analyze *Live* is criticized as
non-deterministic / "tunes backwards": live incremental apply, EGO feedback while
trimming, lag misattribution, order dependence): offline **batch** over the whole
log; **fixed sample ordering + tie-breaking**; explicit thresholds as `params`;
neutralize EGO trim; fixed lag shift; confidence a pure function; **no RNG**; and
**no HashMap iteration in float accumulation** (sort/index cells, fixed reduction
order, no parallel reduce) — order-of-summation changes float results.

**Input seam (the M4 crux — no `datalog` crate yet).** `ve_analyze` consumes an
in-memory `Vec<Sample>` (channel-name → f64 rows + timestamp) captured by a
**bounded ring-buffer tap** on the realtime owner during a live session — shaped
like what M5's `datalog` reader will later emit, so it does **not** constrain M5's
format design (ARCHITECTURE §5.5/§5.6). The owner already decodes full frames at
25 Hz (`realtime/poll.rs`); M4 adds an opt-in capture ring, not a datalog format.

## F. Simulator — what M4 must add

Today `engine/mod.rs` physics emit rpm / thermal (coolant, iat) / throttle (tps) /
MAP / ignition (advance) / voltage; `och_codec.rs` `ChannelValues` carries
`secl, rpm, map, baro, coolant, iat, tps, battery, advance, afr_target` (hardcoded
stoich 14.7), `running`, `cranking`. **Gap:** there is **no measured `afr`, no
`egoCorrection`, no `veCurr`** — so `ve_analyze` gets a *target* and nothing to
correct against. Both sim INIs also lack the channels: `resources/speeduino.sample.ini`
and `tests/fixtures/realtime-owner.ini` declare `secl/engine/running/rpm/
coolantRaw/tps` only.

**Additions (precise):** (1) `ChannelValues` gains measured `afr` (+ `egoCorrection`,
`veCurr`); (2) physics produce a measured AFR that **deliberately deviates from
target in a deterministic, cell-dependent way** (a seeded VE-error surface, e.g.
lean at high load) so analyze yields a non-trivial, auditable correction — *this*,
not "more channels" generically, is the key addition; (3) sample/fixture INIs add
`afr`/`afrTarget`/`egoCorrection`/`veCurr` output channels (real offsets: `afr`@10,
`egoCorrection`@11, `VE1`@19, `afrTarget`@21) and optionally a `[VeAnalyze]` block.
`och_codec` is definition-driven, so encoding falls out once the struct + INI
channels exist.

## Port-vs-fresh ledger (ADR-0006)

| Surface | Decision | Source (license) | Notes |
| --- | --- | --- | --- |
| Richer `[TableEditor]`/`[CurveEditor]` parse (page, axis-channel, labels, geometry, curve axes) | **Port** | hyper-tuner/ini + types MIT (`parseTables`/`parseCurves`) | Extends our thin `TableDef`/`CurveDef`; bins stay lazy string refs |
| `lastOffset` → previous-field-*start* (aliasing) | **Write fresh** | — (hyper-tuner doesn't handle `lastOffset`) | Parser-local, `constants_fields.rs`; all 5 uses are AFR↔Lambda aliases |
| Comms-scatter fix (`[Constants]`/`[OutputChannels]` keys) | **Write fresh** | speeduino.ini GPL-3 (truth source) | Widen `parse_comms` scan / make `ochGetCommand` optional-with-override |
| `[VeAnalyze]`/`[WueAnalyze]` parse | **Write fresh** | speeduino.ini GPL-3 (grammar l. 5984) | hyper-tuner doesn't parse them |
| Table math (interpolate/smooth/scale/set-equal) | **Write fresh** (Rust core) | LibreTune GPL-2 + TS — behavioral only | Bilinear / Gaussian; unit-tested; shared 2D+3D+analyze |
| 2D grid editor UI | **Write fresh** | hypertuner-cloud MIT is read-only (nothing to lift) | DOM grid; LibreTune GPL-2 study-only for UX |
| Curve (1D) editor | **Write fresh** | — | Reuse grid + line view |
| 3D surface (three.js) | **Write fresh** | `three` MIT dep (ROADMAP-sanctioned) | Raw three.js, lazy; LibreTune structure study-only |
| `ve_analyze` algorithm | **Write fresh** (deterministic) | Speeduino `[VeAnalyze]` GPL-3 + MS Extra/MLV behavioral | No code port (MLV/TS proprietary) |
| Simulator measured-AFR + VE-error surface | **Write fresh** | extends askrejans MIT port | Definition-driven `och_codec` |
| TSV clipboard | **Write fresh** | `navigator.clipboard` (no dep) | WKWebView read may need a gesture |

## Open decisions for the planner

1. **Grid substrate:** DOM `<table>` (a11y/keyboard/clipboard free) vs canvas
   (ARCHITECTURE §3 wording). Recommend DOM for the 16×16 grid; canvas/WebGL for
   overlay + 3D only.
2. **Where the cell-write seam lives:** `model::set_cell(name, index, value)` +
   per-cell undo, vs a command-layer element-diff over `set(Value::Array)`.
   Recommend a real sub-range write in `model` (minimal serial bytes, per-cell
   undo).
3. **Does M4 load the *unmodified* real speeduino.ini?** Requires both walls
   (comms-scatter + `lastOffset`) fixed. Recommend yes — it's the honest gate for
   "edit VE/AFR tables" and both fixes are small; add a real-INI golden test.
4. **`analysis` crate name/placement** (§5.9 says `analysis`; design doc floats
   `tuning`). Seed it in M4 with `ve_analyze` only.
5. **Capture source for `ve_analyze`:** ring-buffer tap on the realtime owner
   (recommended, minimal) vs a `datalog`-lite writer (pulls M5 forward).
6. **Parse `[VeAnalyze]` binding** (data-driven table/channel/filter) vs hardcode
   the Speeduino names. Recommend parse — it's the ADR-0002 way and rusEFI/MS
   differ.
7. **three.js: raw vs `@react-three/fiber`.** Recommend raw (no-new-deps); revisit
   if 3D grows.
8. **Curve editor value input:** reuse the 2D grid ops on a 1×N table vs a
   bespoke drag-the-line widget. Recommend the grid path first (shared ops).

## Risks

- **Two-wall real-INI ingestion is M4's hidden prerequisite.** Table editors that
  target the real INI can't load it today (comms-scatter fails *first*, then the
  `lastOffset` overflow). Mitigate: land both parser fixes before any editor UI;
  pin M2 behavior with existing tests + a real-INI golden smoke test.
- **Write amplification / burn over serial.** A whole-array 256-byte write per
  single-cell edit + coarse page-level undo. Mitigate: `set_cell`/sub-range write
  + per-cell undo; debounce/commit-on-blur (the M2 follow-up); coalesce a
  multi-cell selection into one write.
- **`ve_analyze` determinism.** Float accumulation order, HashMap iteration,
  EGO-trim double-counting, wideband-lag misattribution can make "same log →
  different result." Mitigate: sorted/indexed cells, fixed sample order, no
  parallel reduce, neutralize `egoCorrection`, fixed lag shift, read
  `includeAFR`/`incorporateAFR`; a golden same-log→identical-output test.
- **three.js in WKWebView.** Retina blur (clamp `setPixelRatio`), micro-stutter,
  and bundle bloat if the static-import trap defeats code-splitting. Mitigate:
  `React.lazy` `<SurfaceView>`, DPR clamp, context-loss handlers, measure the
  chunk, keep the mesh modest.
- **Aliasing correctness beyond the load error.** Scalar aliases
  (`ego_min_lambda`, `ego_max_lambda`, `afrProtectDeviation`, `n2o_maxLambda`)
  land on the *wrong* byte today with **no** overflow to flag it. Mitigate: the
  `lastOffset` fix corrects all five; add a test asserting `afrTable`/`lambdaTable`
  share offset 0 and each ego pair shares its byte. (The model already supports
  aliasing — this is a parser bug, not a reshape.)

## Proposed task decomposition sketch

Seams/ordering only — a planner turns these into tasks.

0. **Parser prerequisites (unblock the real INI).** (a) Widen comms parsing to
   pick up keys scattered into `[Constants]` + `[OutputChannels]` (or make
   `ochGetCommand` optional-with-override); (b) `lastOffset` → previous-field-start
   (aliasing). Gate: unmodified speeduino.ini loads (golden test).
1. **Richer table/curve model.** Port the hyper-tuner `Table`/`Curve` shape into
   `TableDef`/`CurveDef` (page, axis-display channels, labels, grid geometry, curve
   axes; resolve z-shape from the constant). DTO + `bindings.ts` regen (binding_gen
   needles; specta 0.0.12 — no `usize`/`u64` over IPC).
2. **Model cell-write seam.** `set_cell`/array-slice write + per-cell undo, reusing
   validate-on-clone → protocol write → commit.
3. **`opentune-analysis` crate seed + table ops.** Pure bilinear interpolate,
   Gaussian smooth, scale, set-equal; unit-tested; shared by UI and analyze.
4. **2D heatmap DOM grid editor.** Selection, keyboard nav, TSV clipboard +
   Paste-Special, ops via commands, heatmap coloring, dirty/undo.
5. **Curve (1D) editor.** Reuse the grid + a line view; same ops subset.
6. **3D surface view.** Lazy `<SurfaceView>` (raw three.js + OrbitControls) + live
   operating-point overlay (rpm × fuelLoad dot from the realtime store).
7. **Realtime capture ring.** Bounded `Vec<Sample>` tap on the owner during a
   session (datalog-lite, not a format).
8. **`ve_analyze`.** Parse `[VeAnalyze]` binding + filters; correction + confidence;
   deterministic; golden same-log test.
9. **Simulator.** Measured `afr` + `egoCorrection` + `veCurr` + a seeded VE-error
   surface; sample/fixture INIs gain the channels (+ `[VeAnalyze]`).
10. **AutoTune UI.** Run `ve_analyze` over the captured log; show per-cell delta +
    confidence heatmap + a visible filtered-data view; apply to `veTable` via the
    `set_cell` path.

Constraints carried verbatim: specta 0.0.12 DTO pattern (no `usize`/`u64` over
IPC); bindings regenerated via `binding_gen` needles; new files < 400 lines; SPDX
`GPL-3.0-or-later`; fail-open per item; `three.js` is the *only* ROADMAP-sanctioned
new dep (no others without justification); cargo needs `. "$HOME/.cargo/env"`;
frontend uses npm + vitest; crates live at `src-tauri/crates/*`.
