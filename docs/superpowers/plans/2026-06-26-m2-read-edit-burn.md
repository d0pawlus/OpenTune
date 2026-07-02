# M2 — Read, Edit & Burn the Tune Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fully parse a firmware INI into an immutable `Definition`, build an
editable `Tune` from ECU pages with scaled accessors / dirty tracking / undo-redo,
read·write·burn pages over the protocol, render the config UI from a data-driven
dialog engine, and diff/merge two tunes — all demoable against the simulator in CI.

**Architecture:** Two new typed seams gate everything and are frozen first
(Task 0): `Definition` (out of `ini` — pages, constants, UI) and `Tune` (out of
`model` — the editable RAM image). `protocol` moves page bytes; `simulator` backs
them with a memory image; the React **dialog engine** renders `Definition` and
writes through `Tune`. The expression evaluator is a pure, isolated function kept
*off* the read/edit/burn critical path. Per
[ADR-0002](../../adr/0002-data-driven-ini.md) the core stays generic — it knows no
specific ECU, only the parsed `Definition`.

**Tech Stack:** Rust (stable), `thiserror` (typed errors), `serde` (+ `specta` for
generated IPC types — never hand-write `src/ipc/bindings.ts`), `tokio` (owner
task), React + TS frontend. No new heavy deps without recording the reason.

## Global Constraints

These apply to **every task**. Values from
[ARCHITECTURE.md](../../ARCHITECTURE.md), [ROADMAP.md §M2](../../ROADMAP.md#m2--read-edit--burn-the-tune-),
[ini-format.md](../../ini-format.md), [protocol.md](../../protocol.md), and the ADRs.

- **Port, don't re-derive.** Per [ADR-0006](../../adr/0006-reuse-existing-parsers.md):
  extend the INI parser ported in M1 from
  [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini) (MIT — GPL-3 compatible);
  page write/burn command bytes come from the open Speeduino `comms.cpp` / rusEFI
  `tunerstudio.cpp` sources. **Each port task's first sub-step confirms the source
  actually covers that surface; if it does not, write fresh and record the choice**
  (ADR-0006's escape clause). **Confirm command bytes with tests against the
  simulator** — never trust memory. Record each source + license in the test module.
- **TDD is mandatory.** Failing test first, minimal code to pass, then refactor.
  No implementation without a failing test. Each crate's `tests/contract.rs` must
  stay green.
- **Immutable `Definition`, owned `Tune`.** The parsed `Definition` is frozen and
  shared (`Arc`); all mutation lives in `Tune` and is recorded as reversible edits.
  No in-place mutation of shared state ([coding-style: immutability](../../../README.md)).
- **Single conversation.** All hardware/page access is serialized through one
  owner task; never interleave two operations on the wire.
  [ARCHITECTURE.md §9](../../ARCHITECTURE.md#9-concurrency--performance-model)
- **Fail safe.** Every page op has a timeout + bounded retry; a failed write or
  burn never leaves the ECU or the in-memory `Tune` half-written. RAM-vs-flash
  state is always explicit.
- **Graceful degradation in parsing.** An unknown INI construct disables just that
  feature and records a `Diagnostic`; it never fails the whole parse.
- **IPC types are generated from Rust** via `tauri-specta` — never hand-write
  `src/ipc/bindings.ts`.
- **License header** on every new source file: `// SPDX-License-Identifier:
  GPL-3.0-or-later` (Rust) / the TS equivalent.
- **Offline-first; no network telemetry.** Small focused files (<400 lines);
  immutable patterns; `cargo fmt` + `cargo clippy -- -D warnings` clean.
- **i18n:** new UI strings go through the PL/EN dictionaries from M0.
- **Commits:** conventional-commit format, scoped per component.

---

## File structure (created / modified)

| Crate / area | Files | Responsibility |
| --- | --- | --- |
| `ini` | `src/lib.rs` (extend), `src/definition.rs` (new), `src/constants.rs` (new), `src/expr.rs` (new), `src/ui.rs` (new), `src/parser.rs` (extend) | `Definition` + full parse: pages/constants, expression evaluator, menus/dialogs/tables/curves |
| `model` | `src/lib.rs` (extend), `src/tune.rs` (new), `src/value.rs` (new), `src/edit.rs` (new), `src/diff.rs` (new) | `Tune` editable RAM image, scaled `Value` accessors, undo/redo, dirty/flash state, diff/merge |
| `protocol` | `src/pages.rs` (new), `src/lib.rs` (extend `Protocol` trait) | page read/write/activation/burn |
| `simulator` | `src/memory.rs` (new), `src/lib.rs` (extend) | backing memory image answering page read/write/burn |
| `src-tauri` | `src/commands.rs`, `src/events.rs` (extend) | tune load/edit/write/burn/diff commands + dirty-state event |
| frontend | `src/components/dialogs/` (new), `src/stores/tune.ts` (new), i18n dicts | data-driven dialog engine + diff view |

---

## Task 0 — Freeze the `Definition` and `Tune` contracts

**Why first:** M1 parallelized only because its seams were already frozen + contract-tested.
M2's two new seams (`Definition`, `Tune`) don't exist yet and everything downstream
builds against them. Land them as real types with `todo!()`/stub bodies + doc
comments, each pinned by a `tests/contract.rs`, so Tasks 1–8 build in parallel
without interface drift.

**Files:**
- Create: `crates/ini/src/definition.rs`, `crates/ini/src/constants.rs`, `crates/ini/src/ui.rs`
- Modify: `crates/ini/src/lib.rs` (re-export, add `parse_definition` signature)
- Create: `crates/model/src/tune.rs`, `crates/model/src/value.rs`, `crates/model/src/edit.rs`
- Modify: `crates/model/src/lib.rs`
- Test: `crates/ini/tests/contract.rs` (extend), `crates/model/tests/contract.rs` (new)

**Interfaces — Produces (the frozen seams):**

```rust
// ── ini ──────────────────────────────────────────────────────────────
pub struct Definition {
    pub comms: CommsSettings,           // from M1, unchanged
    pub pages: Vec<PageDef>,
    pub constants: Vec<ConstantDef>,    // lookup by name via `constant(&self, name)`
    pub pc_variables: Vec<ConstantDef>,
    pub menus: Vec<MenuDef>,
    pub dialogs: Vec<DialogDef>,
    pub tables: Vec<TableDef>,
    pub curves: Vec<CurveDef>,
    pub diagnostics: Vec<Diagnostic>,   // surfaces what was skipped
}
impl Definition { pub fn constant(&self, name: &str) -> Option<&ConstantDef>; }

pub struct PageDef { pub number: u16, pub size: usize }

pub struct ConstantDef {
    pub name: String,
    pub page: u16,
    pub offset: usize,            // `lastOffset` keyword resolved to the running
                                  // offset counter at parse time (not an expr)
    pub kind: ConstantKind,
    pub scale: Number,           // physical = raw * scale + translate
    pub translate: Number,
    pub units: String,
    pub low: Number,
    pub high: Number,
    pub digits: u8,
}

// Real INIs put *expressions* where numbers are expected:
// `scale = { 0.1 / stoich }`, `high = { boostTableLimit }` (speeduino.ini).
// `Lit` is the common case (resolved fully on the critical path); `Expr` defers
// to the Task 2 evaluator with a value lookup. Freezing this now is why Task 0
// runs first — the research caught it before the seam was frozen.
pub enum Number { Lit(f64), Expr(String) }
pub enum ConstantKind {
    Scalar(ScalarType),
    Array { elem: ScalarType, shape: Shape },
    Bits  { storage: ScalarType, bit_lo: u8, bit_hi: u8, options: Vec<String> },
    Text  { len: usize },
}
pub enum ScalarType { U08, S08, U16, S16, U32, S32, F32 }
pub struct Shape { pub rows: usize, pub cols: usize }   // cols == 1 ⇒ 1-D

pub struct MenuDef   { pub label: String, pub items: Vec<MenuItem> }
pub struct MenuItem  { pub label: String, pub dialog: String }
pub struct DialogDef { pub name: String, pub title: String, pub fields: Vec<DialogField> }
pub struct DialogField {
    pub kind: FieldKind,
    pub visible: Option<String>,   // raw expr string, evaluated by `expr` (Task 2)
    pub enable:  Option<String>,
}
pub enum FieldKind { Constant(String), Panel(String), Label(String), Gap }
pub struct TableDef { pub name: String, pub x_bins: String, pub y_bins: String, pub z: String }
pub struct CurveDef { pub name: String, pub x_bins: String, pub y_bins: String }
pub struct Diagnostic { pub section: String, pub detail: String }

pub fn parse_definition(ini_text: &str) -> Result<Definition, IniError>;

// ── model ────────────────────────────────────────────────────────────
pub enum Value { Scalar(f64), Array(Vec<f64>), Enum(u32), Text(String) }

pub struct Tune { /* def: Arc<Definition>, pages: Vec<Vec<u8>>, dirty, undo, redo */ }
impl Tune {
    pub fn new(def: std::sync::Arc<Definition>) -> Self;       // zeroed pages
    pub fn load_page(&mut self, page: u16, bytes: Vec<u8>);    // from protocol read
    pub fn get(&self, name: &str) -> Result<Value, ModelError>;
    pub fn set(&mut self, name: &str, value: Value) -> Result<(), ModelError>;
    pub fn undo(&mut self) -> bool;
    pub fn redo(&mut self) -> bool;
    pub fn is_dirty(&self) -> bool;
    pub fn dirty_pages(&self) -> Vec<u16>;
    pub fn mark_burned(&mut self);
    pub fn page_bytes(&self, page: u16) -> &[u8];
}
pub enum ModelError { UnknownConstant(String), OutOfRange{ name: String, value: f64 }, TypeMismatch(String), UnresolvedExpr(String) }
```

- [ ] **0.1** Define the `ini` types above in `definition.rs`/`constants.rs`/`ui.rs`
  with full doc comments; `parse_definition` body is `todo!()`. Derive
  `Debug, Clone, PartialEq` (+ `serde::Serialize` + `specta::Type` where the
  frontend consumes them).
- [ ] **0.2** Define the `model` types in `tune.rs`/`value.rs`/`edit.rs`; method
  bodies `todo!()`. `Tune` holds `Arc<Definition>`.
- [ ] **0.3** `crates/ini/tests/contract.rs`: construct a `Definition` literal by
  hand (one page, one scalar constant, one dialog with a `Constant` field) and
  assert `constant("name")` finds it — pins the shape. RED is fine (no parse yet);
  the test must **compile and pass on hand-built data**.
- [ ] **0.4** `crates/model/tests/contract.rs`: build a `Tune::new(Arc::new(def))`
  from the hand-built `Definition`, assert `is_dirty() == false` and
  `page_bytes(0).len() == page size`. Compiles + passes against stub `new`.
- [ ] **0.5** `cargo build -p opentune-ini -p opentune-model` clean; `cargo clippy
  -- -D warnings`. Commit: `feat(ini,model): freeze Definition + Tune contracts for M2`.

---

## Task 1 — `ini`: full `[Constants]` / pages parse → `Definition`  *(roadmap bullet 1a)*

**Files:** `crates/ini/src/{parser,constants,definition}.rs`; golden fixture
`crates/ini/tests/fixtures/speeduino-constants.ini`; `crates/ini/tests/constants.rs`.

**Interfaces:**
- Consumes: the M1 tokenizer in `parser.rs`; the `Definition`/`ConstantDef` types (Task 0).
- Produces: `parse_definition` populates `pages` + `constants` (UI/expr filled later).

> **Confirmed (research):** PORT from
> [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini) (MIT) — its
> `parseConstants`/`parseConstAndVar` cover `page=N`, scalar/array/bits, offset and
> the full scalar tail. **Field renames to honor:** INI `translate`→`transform`,
> `lo`/`hi`→`min`/`max`. **Gap to fill:** hyper-tuner drops string-type constants
> (`{}`) — pull `StringVariable` handling from `adbancroft/TunerStudioIniParser`
> (LGPLv3; avoid its `F32`→`U08` typo). **Capture number-or-expression** for
> offset/scale/transform/min/max (both refs already do via `numberOrExpression`).

- [ ] **1.1 Preprocessor first.** hyper-tuner handles only `#define`; real
  speeduino.ini gates definitions (`blockingFactor`, `burnCommand`) behind `#if`
  ×57 / `#else` ×53. **Write fresh** a symbol-based preprocessor (`#define`,
  `#set`/`#unset`, `#if`/`#ifdef`/`#ifndef`/`#elif`/`#else`/`#endif`) — `#if`
  conditions in real Speeduino are **always bare symbols** (`LAMBDA`, `mcu_stm32`),
  never arithmetic, so symbol-only resolution is complete. Use adbancroft's
  `pre_processor.lark` as the structural reference (no `#include` — speeduino uses
  none; record the limitation). Test: an INI with `#if X / #else / #endif` selects
  the right branch given the active symbol set.
- [ ] **1.2** Golden fixture: a real, trimmed Speeduino `[Constants]` block with a
  `page = N`, a scalar (`U08`, `scale`/`transform`/units/`min`/`max`/digits), a 1-D
  array, a 2-D table array, a `bits, U08, [b:e]` field with named options, a
  string-type constant, and one constant whose `scale` is an expression
  (`{ 0.1 / stoich }`). Record source INI + license in the test header.
- [ ] **1.3** Failing test `parses_constants_and_pages`: `parse_definition(fixture)`
  yields the expected `PageDef { number, size }` list and, per constant, the right
  `page`, `offset` (incl. a `lastOffset`-resolved one), `kind` (`Shape` for the
  table; `bit_lo/bit_hi/options` for the bitfield; `Text{len}` for the string),
  `scale`/`translate`/`low`/`high` as `Number::Lit` (and the expression one as
  `Number::Expr("0.1 / stoich")`), `digits`. Run → RED.
- [ ] **1.4** Implement the `[Constants]` parser (port + extensions): `page`
  declarations, field-order with the renamed keys, type→`ScalarType` map, array
  shapes→`Shape`, `bits`→`ConstantKind::Bits`, string→`Text`; resolve `lastOffset`
  via a running counter; parse number-or-expression into `Number`. GREEN.
- [ ] **1.5** Robustness tests: unknown constant `class` → `Diagnostic`, parse
  continues; offset beyond declared page size → `IniError::InvalidValue`; an INI
  with `endianness = big`. Keep `tests/contract.rs` green.
- [ ] **1.6** Commit: `feat(ini): preprocessor + full constants/pages into Definition`.

---

## Task 2 — `ini`: sandboxed expression evaluator  *(roadmap bullet 1b — OFF critical path)*

**Files:** `crates/ini/src/expr.rs`; `crates/ini/tests/expr.rs`.

**Interfaces:**
- Consumes: a variable lookup closure / `&Tune`-like context (kept abstract — a
  `&dyn Fn(&str) -> Option<f64>`), so it has **no dependency on `model`**.
- Produces: `pub fn eval(expr: &str, lookup: &dyn Fn(&str) -> Option<f64>) -> Result<f64, ExprError>`
  and `pub fn eval_bool(...) -> Result<bool, ExprError>` for `visible`/`enable`.

> Isolated and pure — only needed to evaluate `visible`/`enable` conditionals and
> dynamic scaling. The read/edit/burn path (Tasks 4–6) must **not** depend on it.

> **Confirmed (research): WRITE FRESH** — a deliberate, recorded ADR-0006
> exception. hyper-tuner and adbancroft both store `{…}` as an opaque string and
> never evaluate. rusEFI's `ExpressionEvaluator.java` is the *only* working
> reference, but porting it would import rusEFI's GPLv3 **§7 off-road/non-aircraft
> field-of-use additional terms** into the tree — so use it as a *structural*
> reference only, not a line port. The full operator set (below) is enumerated from
> real speeduino.ini. **Record this license-driven exception** in the module + an
> ADR note.

- [ ] **2.1** Failing tests covering the grammar from real INIs: integer/float
  literals; arithmetic `+ - * /` with parentheses + precedence; comparisons
  `< <= > >= == !=`; boolean `&& || !`; bare-symbol variable refs resolved via
  `lookup` (e.g. `injLayout != 0 && nCylinders == 4`); unknown symbol →
  `ExprError::UnknownVar`. RED.
- [ ] **2.2** Implement a small recursive-descent (Pratt) evaluator — sandboxed (no
  I/O, no arbitrary code), structured after rusEFI's evaluator but written fresh.
  `eval_bool` treats non-zero as true. GREEN.
- [ ] **2.3** Function-call forms `bitStringValue(label, sel)` / `table(x, "f.inc")`
  appear in real files and **no source evaluates them** — stub them behind an
  `ExprError::UnsupportedFn(name)` (full support deferred; the constants that use
  them degrade to a `Diagnostic`, not a crash). Test the stub path.
- [ ] **2.4** Edge tests: division by zero → `ExprError::Math`; empty expr; deeply
  nested parens bounded. Commit: `feat(ini): sandboxed expression evaluator (fresh, ADR-0006 exception)`.

---

## Task 3 — `ini`: menus / dialogs / tables / curves parse  *(roadmap bullet 1c)*

**Files:** `crates/ini/src/ui.rs`; `crates/ini/tests/fixtures/speeduino-ui.ini`;
`crates/ini/tests/ui.rs`.

**Interfaces:**
- Consumes: tokenizer (Task 1) + the UI types (Task 0). Stores `visible`/`enable`
  as **raw strings** — evaluation is the dialog engine's job via Task 2.
- Produces: `parse_definition` additionally fills `menus`, `dialogs`, `tables`, `curves`.

> **Confirmed (research): PORT from `hyper-tuner/ini` (MIT)** — covers all four
> sections (`parseMenu`, `parseDialogs`, `parseTables`, `parseCurves`). **Extend**
> for `commandButton`, `slider`, `displayOnlyField`, `settingSelector` (hyper-tuner's
> own TODO; real speeduino.ini uses `commandButton`/`slider`). **Tolerate optional
> trailing tokens:** `subMenu` may carry a page **and** a `{cond}`
> (`subMenu = inj_trimad_B, "...", 9, { nFuelChannels >= 5 }`); `field` has a 4-arg
> form with a `{}` placeholder (`field = "...", var, {}, { cond }`).

- [ ] **3.1** Fixture: a trimmed `[Menu]` (incl. a `subMenu` with page + condition),
  a `[UserDefined]` `dialog` with `field`s (one with a `{ ... }` visible condition,
  one `commandButton`), a `[TableEditor]` referencing x/y/z constant names, a
  `[CurveEditor]`. Record source + license.
- [ ] **3.2** Failing test `parses_ui`: assert the `MenuDef`/`MenuItem` tree, a
  `DialogDef` with the expected `FieldKind::Constant(name)` and raw `visible`
  string, the `commandButton` field, and `TableDef { x_bins, y_bins, z }` /
  `CurveDef` carrying the referenced constant names. RED.
- [ ] **3.3** Implement the section parsers (port + extensions), tolerating optional
  trailing positional tokens; unknown field kinds → `Diagnostic` (graceful).
  Cross-reference check: a table referencing a missing constant records a
  `Diagnostic` (does not panic). GREEN.
- [ ] **3.4** Commit: `feat(ini): parse menus, dialogs, tables, curves`.

---

## Task 4 — `model`: `Tune` — scaled accessors, dirty, RAM-vs-flash, undo/redo  *(roadmap bullet 2)*

**Files:** `crates/model/src/{tune,value,edit}.rs`; `crates/model/tests/tune.rs`.

**Interfaces:**
- Consumes: `Arc<Definition>` (Task 0/1). Independent of `protocol`/`simulator`.
- Produces: the editable `Tune` the UI and protocol use (signatures frozen in Task 0).

- [ ] **4.1** Failing test `get_set_roundtrip`: from a `Definition` with a `U08`
  constant `scale=0.1, translate=0`, `load_page(0, bytes)`, then `get("rpmK")`
  returns the scaled physical value; `set("rpmK", Value::Scalar(x))` writes the
  correct raw byte (inverse scaling, rounded) and `get` reflects it. RED.
- [ ] **4.2** Implement raw↔physical scaling per `ScalarType` + endianness
  (`physical = raw*scale+translate`; write inverts and clamps to `[low, high]` →
  `ModelError::OutOfRange` when outside). Resolve each `Number`: `Lit` directly;
  `Expr` via the Task 2 evaluator with a `&Tune`-backed lookup (an unresolvable
  expr → `ModelError::UnresolvedExpr`, surfaced as a diagnostic, not a panic — the
  common numeric path never invokes the evaluator). Arrays read/write element-wise;
  bits mask in/out; text as bytes. GREEN.
- [ ] **4.3** Dirty + flash state test: a `set` marks `is_dirty()`, records the
  changed page in `dirty_pages()`; `mark_burned()` clears dirty. RED → implement
  (track changed byte ranges per page) → GREEN.
- [ ] **4.4** Undo/redo test: `set` then `undo()` restores the prior bytes and
  `get`; `redo()` re-applies; a fresh `set` clears the redo stack. RED → implement
  the `Edit { page, offset, old, new }` stacks → GREEN.
- [ ] **4.5** `UnknownConstant` + `TypeMismatch` (e.g. `Value::Text` into a scalar)
  tests. Commit: `feat(model): Tune with scaled accessors, dirty/flash, undo-redo`.

---

## Task 5 — `protocol`: page read / write / activation / burn  *(roadmap bullet 3)*

**Files:** `crates/protocol/src/pages.rs`; `crates/protocol/src/lib.rs` (extend
`Protocol` trait); `crates/protocol/tests/pages.rs` (simulator-backed).

**Interfaces:**
- Consumes: `Transport` (M1), `CommsSettings` command templates (`pageReadCommand`,
  `pageValueWrite`, `burnCommand`, `pageActivationDelay`), `PageDef` geometry.
- Produces: trait methods `read_page(page) -> Vec<u8>`, `write(page, offset, &bytes)`,
  `burn(page)`, reusing M1's `%2i/%2o/%2c/%v` expansion + the two framings.

> **Confirmed (research)** against Speeduino `comms.cpp` @ **`noisymime/speeduino@63fd68e9`**
> (pin this SHA — `master` moves), CRC-wrapped path:
> - **WRITE = `'M'`**, template `M%2i%2o%2c%v`: `%2i` page id (2 bytes, **high byte
>   ignored**, page = low byte), `%2o` offset (2 bytes **little-endian**), `%2c`
>   length (2 bytes LE), `%v` value bytes. Live RAM write; EEPROM flush deferred.
> - **BURN = `'b'`**, template `b%2i`: **per-page** (`savePage(page)`), not whole-config.
> - **No stateful page-select** in current firmware — page travels inline as `%2i`
>   in every read/write/burn. The legacy `'P'` select is unused; do not implement it.
> - **`pageActivationDelay`** is a post-command *delay*, not a select command.

- [ ] **5.1 Extend the template expander** (M1 handles `%2i/%2o/%2c/%v`): real
  command strings also embed `$tsCanId` (a `#define` substitution) and `\xNN` hex
  literals (e.g. `ochGetCommand="r$tsCanId\x30%2o%2c"`). Failing test: expanding a
  template with `$tsCanId` + `\x30` yields the right bytes. RED → implement → GREEN.
- [ ] **5.2** Failing test `reads_page`: against a scripted `SimTransport`,
  `read_page(0)` issues the expanded `pageReadCommand` (`p%2i...`) and returns the
  page bytes. RED → implement using the expander + M1 framing. GREEN.
- [ ] **5.3** Failing test `writes_partial`: `write(0, offset, &bytes)` emits `'M'`
  with page low-byte + LE offset + LE length + value bytes, honoring
  `interWriteDelay` and the post-write `pageActivationDelay`; a vanished device →
  `TransportError::Disconnected`, never a partial silent write. RED → implement. GREEN.
- [ ] **5.4** Failing test `burns_page`: `burn(0)` emits `'b'` + page low-byte
  (`savePage` semantics); verify the bytes against the simulator. RED → implement. GREEN.
- [ ] **5.5** Commit: `feat(protocol): page read/write/burn (inline page-id, comms.cpp@63fd68e9)`.

---

## Task 6 — `simulator`: backing memory image for read/write/burn  *(roadmap bullet 4)*

**Files:** `crates/simulator/src/memory.rs`; `crates/simulator/src/lib.rs` (extend);
`crates/simulator/tests/memory.rs`.

**Interfaces:**
- Consumes: `Definition` page geometry (from `ini`). Provides the `Transport` the
  protocol talks to.
- Produces: a RAM image + a separate flash image so write→burn→reconnect semantics
  are testable; reuses M1's signature/version answers and drop control.

- [ ] **6.1** Failing test `write_read_roundtrip`: a `Simulator::from_definition`
  answers `pageReadCommand` from its RAM image; a `pageValueWrite` mutates RAM; a
  subsequent read reflects it. RED → port response logic from
  [`speeduino-serial-sim`](https://github.com/askrejans/speeduino-serial-sim)
  (record license) extended with a `Vec<Vec<u8>>` RAM image. GREEN.
- [ ] **6.2** Failing test `burn_persists`: after `write` + `burnCommand`, a
  simulated reboot keeps the burned bytes; **un-burned** writes are lost on reboot
  (RAM vs flash). RED → add a flash image copied on burn, RAM reset to flash on
  reboot. GREEN.
- [ ] **6.3** Commit: `feat(simulator): memory image with page write + burn/flash`.

---

## Task 7 — Frontend data-driven dialog engine + live write  *(roadmap bullet 5)*

**Files:** `src/components/dialogs/{DialogEngine,Field,TableField}.tsx`,
`src/stores/tune.ts`; Tauri `src-tauri/src/commands.rs` (+ `events.rs`); i18n dicts.
Bindings **regenerated**, not hand-written.

**Interfaces:**
- Consumes: a `Definition` + `Tune` snapshot over generated IPC; can develop
  against a frozen `Definition` JSON fixture before the backend lands.
- Produces: a rendered menu→dialog UI; edits call a `set_value` command → live write.

- [ ] **7.1** Backend commands: `load_tune(page_reads)` / `get_definition()` →
  `Definition`; `set_value(name, value)` (writes via `Tune` + `protocol`, emits a
  `tune_dirty` event); `burn()`; `undo()`/`redo()`. Hold `Tune` in the single owner
  task (ARCHITECTURE §9). Unit-test command logic against the simulator. Regenerate
  `src/ipc/bindings.ts` (`cargo run` debug) — never hand-edit.
- [ ] **7.2** Vitest the `tune` store reducer: applying a `tune_dirty` event flips a
  dirty indicator; `set_value` optimistic update + rollback on command error.
- [ ] **7.3** `DialogEngine` renders a `DialogDef`: each `FieldKind::Constant`
  renders an editable `Field` bound to the constant's value/units/limits;
  `visible`/`enable` evaluated by a TS port of the expr rules (or a backend
  `eval_visibility` command — pick one, record it). `Panel` recurses; `TableEditor`
  renders a minimal grid (full editor is M4). PL/EN strings for all labels.
- [ ] **7.4** E2E (simulator, no hardware): open a tune → change a constant through
  the generated dialog → see the live write + a clear **"modified, not burned"**
  badge → burn → badge clears → undo/redo round-trips. **This is the M2 demo.**
- [ ] **7.5** Commit: `feat(app): data-driven dialog engine with live write + burn`.

---

## Task 8 — Tune diff / merge  *(roadmap bullet 6 — most-requested missing TS feature)*

**Files:** `crates/model/src/diff.rs`; `crates/model/tests/diff.rs`; frontend
`src/components/diff/TuneDiff.tsx`; `src-tauri/src/commands.rs` (`diff_tune`).

**Interfaces:**
- Consumes: two `Tune`s (or a `Tune` + a loaded `.msq`/snapshot — file import is M6;
  here diff against another in-memory `Tune` built from a saved page set).
- Produces: `pub fn diff(a: &Tune, b: &Tune) -> Vec<FieldDiff>` and
  `pub fn merge(base: &mut Tune, incoming: &Tune, picks: &[String])`.

```rust
pub struct FieldDiff {
    pub name: String,
    pub a: Value,           // current
    pub b: Value,           // other
    pub cells: Vec<CellDiff>, // for arrays/tables: per-element differences
}
pub struct CellDiff { pub index: usize, pub a: f64, pub b: f64 }
```

- [ ] **8.1** Failing test `diffs_scalar_and_table`: two `Tune`s differing in one
  scalar and two cells of a table produce exactly those `FieldDiff`/`CellDiff`
  entries (and nothing for unchanged constants). Builds on Task 4's field-level
  model. RED.
- [ ] **8.2** Implement `diff` by walking `Definition.constants`, comparing scaled
  `Value`s; for arrays/tables emit per-cell `CellDiff`. GREEN.
- [ ] **8.3** Failing test `merge_selected`: `merge(base, incoming, &["scalarName"])`
  applies only the picked constant (recording undo edits on `base`), leaving
  un-picked differences alone. RED → implement via `Tune::set`. GREEN.
- [ ] **8.4** Frontend `TuneDiff`: list per-constant differences with a current/other
  column and a per-row "take" checkbox; "merge selected" calls `diff_tune` + merge.
  PL/EN strings. Vitest the selection→merge payload.
- [ ] **8.5** Commit: `feat(model,app): tune diff and selective merge`.

---

## Self-review

**Spec coverage (M2 roadmap bullets):**
- `ini` full constants/pages → Task 1. ✅
- `ini` expression evaluator → Task 2. ✅
- `ini` dialogs/menus → Task 3. ✅
- `model` Tune: pages, scaled accessors, dirty/undo-redo, RAM-vs-flash → Task 4. ✅
- `protocol` page read/write, activation, burn, CRC variants (reuses M1 framing) → Task 5. ✅
- `simulator` backing memory image for read/write/burn → Task 6. ✅
- Frontend data-driven dialog engine (conditional visible/enable, live write) → Task 7. ✅
- Tune diff/merge → Task 8. ✅
- **Demo** (open → edit via dialogs → live write → burn → undo/redo) → Task 7.4. ✅

**Parallelism after Task 0:** Tasks 1, 2, 4, 6 are independent against the frozen
contracts. Task 3 depends on Task 1's tokenizer; Task 5 depends on Task 1 (page
geometry) + reuses M1's transport/framing; Task 7 depends on 1/4/5 (can start
against a `Definition` fixture); Task 8 depends on Task 4. Each seam is pinned by a
`tests/contract.rs` so an accidental shape change breaks a fast test before any
integration.

**Critical-path isolation:** the expression evaluator (Task 2) takes a `lookup`
closure, not `&Tune`, so the read/edit/burn path (4→5→6→7) never blocks on it.

**Ported-source ledger (ADR-0006) — confirmed by investigation:**
- Task 1 constants/pages → **PORT `hyper-tuner/ini` (MIT)**; field renames
  `translate→transform`, `lo/hi→min/max`; string constants from
  `adbancroft/TunerStudioIniParser` (LGPLv3 — record the grant caveat: classifier
  only, no LICENSE file).
- Task 1 preprocessor → **WRITE FRESH** (symbol-based `#if/#else/#set/#define`),
  adbancroft `pre_processor.lark` as structural ref; hyper-tuner's `#define`-only
  is insufficient (speeduino gates real keys behind `#if`).
- Task 2 evaluator → **WRITE FRESH** — deliberate ADR-0006 exception for license
  cleanliness (rusEFI's working evaluator carries GPLv3 §7 field-of-use terms);
  use its *structure* only.
- Task 3 UI → **PORT `hyper-tuner/ini` (MIT)**, extend for `commandButton`/`slider`/
  `displayOnlyField`.
- Task 5 write/burn → Speeduino **`comms.cpp` @ `noisymime/speeduino@63fd68e9`**
  (pinned). Task 6 → `speeduino-serial-sim`.
Each task's first commit records source + license; the Task 2 commit + an ADR note
record the deliberate write-fresh exception.

**Known risks to watch:** (1) INI constant field-order and the four roles of `{…}`
(boolean cond / arithmetic scale / function call / placeholder) vary — drive from
the parsed `Number`/raw-string structure, cover with multiple fixtures, never
hardcode one example ([ini-format.md](../../ini-format.md)); (2) command templates
embed `$tsCanId` + `\xNN` literals — the expander (Task 5.1) must handle them or
read/write bytes are wrong; (3) `bitStringValue`/`table` function-call expressions
have no port source — stubbed in Task 2.3, the constants using them degrade to a
diagnostic until a later milestone.
