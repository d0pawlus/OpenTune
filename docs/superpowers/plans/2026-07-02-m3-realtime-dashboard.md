# M3 — Real-time Dashboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stream live output-channel data from the ECU (real or simulator) into a
configurable canvas gauge dashboard: parse `[OutputChannels]`/`[GaugeConfigurations]`/
`[FrontPage]` into the `Definition`, migrate the backend to a Tokio owner-task + command
channel (ARCHITECTURE §9), decode frames to already-scaled physical values, coalesce them
to ≤30 Hz events, and render hand-rolled gauges with an editable, persisted layout — all
demoable against the simulator with link-drop recovery.

**Architecture:** Two seams freeze first (Task 0): the `Definition` extension
(`output_channels` + `gauges` + `frontpage`, plus `och_block_size` on `CommsSettings`) and
the `realtime` crate's public types (`ChannelValue`, `RealtimeFrame`, the `RealtimeFrameEvent`
IPC payload). The §9 owner-task migration (Task 1) lands early because the realtime poll loop
must live *inside* the single wire-owner; it **wraps** the existing synchronous `Session`
rather than rewriting it, so every M2 test stays green. `ini` parses the channel/gauge grammar
(Tasks 2–3); `protocol` gains `read_output_channels` (Task 4); `simulator` animates the och
block (Task 5); `realtime` owns poll + decode + throttle (Task 6); the frontend renders
imperative canvas gauges off a reflect-only store (Task 7); the demo proves it end-to-end
(Task 8). Per [ADR-0002](../../adr/0002-data-driven-ini.md) the core knows no specific ECU —
only the parsed `Definition`.

**Tech Stack:** Rust (stable), `tokio` (owner task + `spawn_blocking` for serial I/O),
`thiserror`, `serde` (+ `specta`/`tauri-specta` for generated IPC types — never hand-write
`src/ipc/bindings.ts`), React + TS, hand-rolled HTML Canvas gauges (no new frontend deps).

## Global Constraints

These apply to **every task**. Values from
[ARCHITECTURE.md](../../ARCHITECTURE.md), [ROADMAP.md §M3](../../ROADMAP.md#m3--real-time-dashboard-),
the [M3 research dossier](../../notes/m3-research.md), [m2-decisions.md](../../notes/m2-decisions.md),
and the ADRs.

- **Port, don't re-derive.** Per [ADR-0006](../../adr/0006-reuse-existing-parsers.md):
  `[OutputChannels]` parsing **ports** from
  [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini) (MIT — `parseOutputChannels`
  reuses the `parseConstAndVar` shape M2 already ported); the simulator's animated engine
  model **ports** from [`askrejans/speeduino-serial-sim`](https://github.com/askrejans/speeduino-serial-sim)
  (MIT — `EngineSimulator.{h,cpp}` state machine + correlations); `'r'`/0x30 wire bytes are
  **confirmed** (truth-source, not port-source) against Speeduino `comms.cpp` @
  `noisymime/speeduino@63fd68e9` (GPL-3). `[GaugeConfigurations]`/`[FrontPage]` parsing,
  the canvas gauges, and the owner-task concurrency are **write-fresh** (no adoptable source;
  canvas-gauges MIT used only as visual reference; LibreTune is GPL-2-only → study only,
  never port). **Each port task's first sub-step confirms the source actually covers that
  surface; if not, write fresh and record the choice.** Record each source + license in the
  test/module header.
- **TDD is mandatory.** Failing test first, minimal code to pass, then refactor. No
  implementation without a failing test. Each crate's `tests/contract.rs` (where present)
  and the M2 test suite must stay green.
- **Immutable `Definition`, owned `Tune`.** The parsed `Definition` is frozen and shared
  (`Arc`); realtime decode reads it but never mutates it.
- **Single conversation.** All hardware access is serialized through **one owner task**
  (ARCHITECTURE §9); the realtime poll loop and page writes/burns interleave on one wire —
  the poll must yield to writes/burns, never run concurrently.
  [ARCHITECTURE.md §9](../../ARCHITECTURE.md#9-concurrency--performance-model)
- **Throttled realtime.** Acquisition runs at device rate; UI events are **coalesced to
  ≤30 Hz** decoupled from the poll rate (ARCHITECTURE §9). Poll serial at **25 Hz**
  (`Duration::from_millis(40)`); the simulator may be polled faster. Gauges render
  imperatively on canvas — React reconciliation stays **off** the hot path.
- **Fail-open, per item.** One unresolvable computed channel (or one unresolvable constant
  in `get_values`) degrades to a diagnostic/skip — it **never** blanks the whole frame or
  panel. This is the single rule shared by realtime decode (Task 6) and the M3 follow-up (b)
  `get_values` fix (Task 6).
- **Fail-safe & graceful degradation.** Never index past a received buffer (INI/firmware
  skew: `ochBlockSize` 139 vs firmware `LOG_ENTRY_SIZE` 138 — tolerate short/padded
  responses). An unknown INI construct records a `Diagnostic`; it never fails the whole parse.
- **IPC types are generated from Rust** via `tauri-specta` — never hand-write
  `src/ipc/bindings.ts`. **specta 0.0.12 forbids `usize`/`u64` over IPC** → new IPC-reachable
  numeric fields use `u32`/`f64` (the `dto.rs` pattern). New commands/events **must** be added
  to `collect_commands!`/`collect_events!` **and** to the `binding_gen` needle tests in
  `src-tauri/src/lib.rs`.
- **No heavy frontend deps.** Gauges are hand-rolled canvas (ARCHITECTURE §3 already mandates
  "2D table grids / gauges: HTML Canvas (custom)"); record the no-dep reason in the component.
- **License header** on every new source file: `// SPDX-License-Identifier: GPL-3.0-or-later`
  (Rust) / the TS equivalent.
- **Small focused files** (<400 lines); immutable patterns; `cargo fmt` +
  `cargo clippy -- -D warnings` clean; `prettier` + `eslint` clean on TS.
- **i18n:** new UI strings go through the PL/EN dictionaries; `pl` must mirror `en` keys.
- **Commits:** conventional-commit format, scoped per component.
- **`cargo` is not on PATH** in this environment — prefix every cargo command with
  `. "$HOME/.cargo/env" &&`.

---

## Env facts (carry into every task)

- Workspace crates live at **`src-tauri/crates/*`** (NOT `crates/*` — the M2 plan's paths
  were wrong). Package names: `opentune-{ini,model,protocol,transport,simulator,realtime,datalog,project}`.
- `binding_gen` test in `src-tauri/src/lib.rs`: `build_specta()` is the single command/event
  registration list; a `BINDINGS_LOCK` mutex serializes the export→read tests. New commands/
  events need a needle assertion there.
- Backend commands today: `#[tauri::command] #[specta::specta]` + `State<SessionStore>` +
  `Result<_, String>`; they lock `SessionStore = Arc<Mutex<Option<Session>>>` and emit events
  **after** the lock is dropped (see `tune_commands.rs`).
- `Session` (`src-tauri/src/connection.rs`) owns `conn: ActiveConnection` + `def: Arc<Definition>`
  + `tune: Option<Tune>` + `snapshot: Option<Tune>`; all wire I/O goes through `&mut Session`
  methods in `session.rs` / `session_diff.rs`.
- Frontend: vitest + @testing-library/react; Zustand stores (reflect-only for connection);
  event listening is inline `useEffect` + `events.x.listen(cb)` calling
  `useStore.getState().applyY(payload)` (App.tsx); `tokens.css` design tokens; typed i18n
  dicts (`src/i18n/{en,pl}.ts`).

---

## Decisions locked for M3 (record in `docs/notes/m3-decisions.md` as they land)

Resolving the dossier's 8 open decisions + the M2 final-review follow-ups. Criterion (as M2):
optimal, non-blocking for future development.

1. **Owner task = Tokio** (§9 + the prompt mandate), not a plain OS thread. `serialport` is
   blocking → wrap blocking transport calls in `tokio::task::spawn_blocking`. Rationale: §9
   names Tokio explicitly; Tauri already hosts a Tokio runtime; the thread-only alternative
   the dossier floats contradicts the mandated architecture. (Decision 1)
2. **Channel decode lives in `realtime`** (frame-oriented, per §5.5), not `model`
   (tune-oriented). `ini` holds only the parsed `OutputChannelDef`; `realtime` reuses
   `opentune_ini::eval` for computed channels. (Decision 2)
3. **Backend evaluates + emits already-scaled physical values** — one source of truth,
   matching the M2 `eval_conditions` precedent; fewer surprises than a TS re-port. (Decision 3)
4. **Event payload = full decoded frame at ≤30 Hz**, `Vec<(String, f64)>` (JS-safe; ~200
   channels × f64 is fine at 30 Hz). Per-gauge subsetting is an M5 optimization. (Decision 4)
5. **Support template windows (`%2o/%2c`) but poll the full `ochBlockSize`** in M3; windowed
   subset reads are an M5 (high-speed-logging) optimization. (Decision 5)
6. **Gauge layout persistence = minimal JSON in the app config dir** via a small
   command pair, **not** the `project` crate (still a placeholder — pulling it forward is
   out of scope). Record the M6 migration path (`project` will absorb this). (Decision 6)
7. **Trimmed section fixtures + one full-file smoke test.** M3 does **not** solve the
   real-INI blockers (aliased tables on page 5, comms keys in `[Constants]`, `groupMenu`) —
   the gauge sections dodge all three. Record: full unmodified `speeduino.ini` ingestion
   stays deferred. (Decision 7)
8. **Explicit `start_realtime`/`stop_realtime` commands** (§7 — testable, and the loop must
   yield the wire to writes/burns), not auto-start on connect. (Decision 8)

**M2 final-review follow-ups** (M3-gating): (a) write-commit semantics — no per-keystroke ECU
writes → debounce/commit-on-blur in `Field.tsx` (Task 7); (b) per-name `get_values` fail-open —
one unresolvable constant must not blank the panel (Task 6); (c) tune invalidation/re-read on
reboot-detected reconnect (secl-based) (Task 1); (d) one manual `tauri dev` GUI smoke-run
**step** in the demo task (Task 8, a step not a task).

**Definition reshape (field display labels + `groupMenu`) is NOT M3.** The `[OutputChannels]`/
gauge work adds new top-level `Definition` fields but does not touch `MenuDef`/`DialogField`,
so the M2-deferred reshape is not forced. Recorded, deferred to M4+.

---

## File structure (created / modified)

| Crate / area | Files | Responsibility |
| --- | --- | --- |
| `ini` | `src/lib.rs` (extend `CommsSettings` + re-exports), `src/output_channels.rs` (new), `src/gauges.rs` (new), `src/definition.rs` (extend + parse wiring), `src/expr_parser.rs` (extend: bitwise `&`/`<<`), `src/parser.rs` (extend: `ochBlockSize`) | `OutputChannelDef`/`GaugeDef`/`FrontPageDef` types + parsing; `och_block_size`; bitwise ops |
| `realtime` | `src/lib.rs` (fill), `src/decode.rs` (new), `src/loop.rs` (new) | frame types, per-channel fail-open decode → physical, poll+coalesce loop |
| `protocol` | `src/pages.rs` (extend: `read_output_channels`), `src/lib.rs` (extend `Protocol` trait) | windowed `'r'`/0x30 realtime read |
| `simulator` | `src/engine.rs` (new — animated model), `src/ecu.rs` (extend: `'r'` arm + `plain_command_len`), `src/lib.rs` (extend) | animated och block + `'r'`/0x30 windowed dispatch |
| `src-tauri` | `src/owner.rs` (new — Tokio owner + command channel), `src/connection.rs` (extend: struct-literal fixups), `src/realtime_commands.rs` (new), `src/layout.rs` (new — JSON persistence), `src/dto.rs` (extend), `src/events.rs` (extend: `RealtimeFrameEvent`), `src/lib.rs` (extend `collect_*!` + needles) | owner-task migration, start/stop, layout save/load |
| frontend | `src/components/gauges/{GaugeCanvas,RoundGauge,BarGauge,DigitalGauge,IndicatorLamp}.tsx`, `src/components/gauges/gauges.css`, `src/components/dashboard/{Dashboard,GaugeBinder}.tsx`, `src/stores/realtime.ts` (new), `src/components/dialogs/Field.tsx` (commit-on-blur), i18n dicts | canvas gauges + editable/persisted dashboard + reflect-only realtime store |

---

## Task 0 — Freeze the M3 contracts (`Definition` extension + `realtime` seam)

**Why first:** M2 parallelized only because its seams were frozen + contract-tested. Adding
fields to `Definition` and `CommsSettings` is a **frozen-contract change** (M2 Task-0 style):
it breaks every struct literal in the tree. Land the new types + the `realtime` seam +
`RealtimeFrameEvent` with `todo!()`/stub parse bodies, fix every literal site, and pin each
seam with a contract test — so Tasks 1–8 build without interface drift.

**Files:**
- Modify: `src-tauri/crates/ini/src/lib.rs` (add `och_block_size` to `CommsSettings`; re-export new types)
- Create: `src-tauri/crates/ini/src/output_channels.rs`, `src-tauri/crates/ini/src/gauges.rs`
- Modify: `src-tauri/crates/ini/src/definition.rs` (add fields; wire stub parse calls)
- Modify: `src-tauri/crates/realtime/src/lib.rs` (seam types + stubs)
- Modify: `src-tauri/src/events.rs` (`RealtimeFrameEvent`)
- Modify (struct-literal fixups): `src-tauri/src/connection.rs` (`plain_comms`, `plain_definition`),
  `src-tauri/crates/simulator/src/lib.rs` (`speeduino_plain_comms`), and any other
  `CommsSettings {` / `Definition {` literal (grep in step 0.1)
- Test: `src-tauri/crates/ini/tests/contract.rs` (extend), `src-tauri/crates/realtime/tests/contract.rs` (new)

**Interfaces — Produces (the frozen seams):**

```rust
// ── ini: CommsSettings gains one field (JS-safe u32, like blocking_factor) ──
pub struct CommsSettings {
    // ... all M1/M2 fields unchanged ...
    /// `ochBlockSize` — total byte length of one full realtime frame (e.g. 139).
    /// The `%2c` count for a full `read_output_channels`. `0` when the INI omits it.
    pub och_block_size: u32,
}

// ── ini: output_channels.rs ─────────────────────────────────────────────────
/// One `[OutputChannels]` entry. Realtime frames decode against these.
pub enum OutputChannelDef {
    /// `map = scalar, U16, 4, "kpa", 1.000, 0.000` — offset into the och block.
    /// physical = raw*scale + translate. No min/max/digits (unlike ConstantDef).
    Scalar {
        name: String,
        kind: ScalarType,          // reuse the existing ini ScalarType
        offset: usize,
        units: String,
        scale: f64,
        translate: f64,
    },
    /// `running = bits, U08, 2, [0:0]` — flag/enum over a byte already in the block.
    Bits {
        name: String,
        storage: ScalarType,
        offset: usize,
        bit_lo: u8,
        bit_hi: u8,
    },
    /// `coolant = { coolantRaw - 40 }`, `throttle = { tps }, "%"`. Expression is an
    /// opaque string (ported hyper-tuner shape), evaluated lazily by `realtime`.
    Computed {
        name: String,
        expr: String,
        units: String,             // "" when the entry declares no trailing units
    },
}
impl OutputChannelDef { pub fn name(&self) -> &str; }

// ── ini: gauges.rs ──────────────────────────────────────────────────────────
/// One `[GaugeConfigurations]` entry. Any positional field may be a `{ expr }`
/// referencing PcVariables/constants → captured as `Number` (Lit or Expr),
/// reusing the M2 `Number` type. `bitStringValue(...)` units degrade to `Expr`.
pub struct GaugeDef {
    pub name: String,          // gauge id referenced by FrontPage slots
    pub channel: String,       // the output-channel var it displays (e.g. "rpm")
    pub title: String,
    pub units: Number,         // usually Lit-via-string; may be {expr}/bitStringValue
    pub low: Number,
    pub high: Number,
    pub lo_danger: Number,
    pub lo_warn: Number,
    pub hi_warn: Number,
    pub hi_danger: Number,
    pub value_digits: u8,      // vd
    pub label_digits: u8,      // ld
    pub category: String,      // gaugeCategory, for grouping menus
}

/// `[FrontPage]` — the default dashboard layout.
pub struct FrontPageDef {
    /// gauge1..gauge8 → GaugeConfigurations names, in slot order (2 rows × 4).
    pub gauge_slots: Vec<String>,
    /// `indicator = { expr }, "off", "on", offBg, offFg, onBg, onFg`.
    pub indicators: Vec<IndicatorDef>,
}
pub struct IndicatorDef {
    pub expr: String,          // bare bit-channel or comparison; evaluated by realtime
    pub off_label: String,
    pub on_label: String,
    pub off_bg: String,        // named color, verbatim
    pub off_fg: String,
    pub on_bg: String,
    pub on_fg: String,
}

// ── ini: Definition gains three fields ──────────────────────────────────────
pub struct Definition {
    // ... all M2 fields unchanged ...
    pub output_channels: Vec<OutputChannelDef>,
    pub gauges: Vec<GaugeDef>,
    pub frontpage: FrontPageDef,   // empty Vecs when the INI declares no [FrontPage]
}
impl Definition {
    /// Look up an output channel by name (mirrors `constant`).
    pub fn output_channel(&self, name: &str) -> Option<&OutputChannelDef>;
}

// ── realtime crate seam ─────────────────────────────────────────────────────
/// One decoded channel value in physical units, or a diagnostic if it failed.
pub struct ChannelValue { pub name: String, pub value: f64 }

/// A fully decoded realtime frame: every successfully decoded channel.
pub struct RealtimeFrame {
    pub channels: Vec<ChannelValue>,
    /// Names that failed to decode this frame (fail-open — never blanks the frame).
    pub diagnostics: Vec<String>,
}

/// Decode one raw och block against the definition's channels into physical values.
/// Fails open per channel: a bad expr/short buffer records a diagnostic and skips.
pub fn decode_frame(def: &opentune_ini::Definition, block: &[u8]) -> RealtimeFrame;

pub enum RealtimeError { NotConnected, Poll(String) }
```

```rust
// ── src-tauri events.rs: the IPC frame payload (JS-safe: Vec<(String,f64)>) ──
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type, Event)]
pub struct RealtimeFrameEvent {
    /// (channel name, physical value) pairs — the full decoded frame, ≤30 Hz.
    pub channels: Vec<(String, f64)>,
}
```

- [ ] **0.1 Grep every struct-literal site that will break.**

Run: `. "$HOME/.cargo/env" && cd src-tauri && grep -rn "CommsSettings {\|Definition {" --include="*.rs" .`
Expected sites (fix in later steps of this task): `crates/simulator/src/lib.rs`
(`speeduino_plain_comms`), `src/connection.rs` (`plain_comms`, `plain_definition`),
plus any `crates/*/tests/*.rs` fixtures. Record the list.

- [ ] **0.2 Add `och_block_size: u32` to `CommsSettings`** in `crates/ini/src/lib.rs` with a
  doc comment (verbatim from the interface block). Add the new modules
  (`mod output_channels; mod gauges;`) and re-exports
  (`pub use output_channels::OutputChannelDef; pub use gauges::{GaugeDef, FrontPageDef, IndicatorDef};`).

- [ ] **0.3 Define the `ini` types** in `output_channels.rs` + `gauges.rs` (full doc comments,
  verbatim from the interface block). Derive `Debug, Clone, PartialEq` (+ `serde::Serialize`
  + `specta::Type` — they are reachable from `Definition`). Add `output_channels`, `gauges`,
  `frontpage` fields to `Definition` and the `output_channel(&self, name)` accessor. In
  `parse_definition`, call stub parsers that return empty collections
  (`Vec::new()` / an empty `FrontPageDef`) — real parsing is Tasks 2–3. `parse_comms` must
  set `och_block_size: 0` for now (Task 2 fills it).

- [ ] **0.4 Define the `realtime` seam** in `crates/realtime/src/lib.rs`: the types above,
  `decode_frame` body `todo!()`, `RealtimeError`. Add `opentune-ini` to
  `crates/realtime/Cargo.toml` `[dependencies]`.

- [ ] **0.5 Define `RealtimeFrameEvent`** in `src-tauri/src/events.rs` (verbatim). Do **not**
  register it yet (registration is Task 6, alongside the commands that emit it).

- [ ] **0.6 Fix every struct literal from 0.1.** Add `och_block_size: 0` to each
  `CommsSettings` literal; add `output_channels: Vec::new(), gauges: Vec::new(),
  frontpage: FrontPageDef { gauge_slots: Vec::new(), indicators: Vec::new() }` to each
  `Definition` literal (import `FrontPageDef` where needed).

- [ ] **0.7 Contract tests.** `crates/ini/tests/contract.rs`: hand-build a `Definition` with
  one `OutputChannelDef::Scalar` and one `GaugeDef`, assert `output_channel("map")` finds it
  and `def.gauges[0].channel == "rpm"`. `crates/realtime/tests/contract.rs`: build a
  hand-made `Definition` + a zeroed block and assert `decode_frame` **compiles** (it may
  `todo!()`-panic — mark the test `#[should_panic]` OR return an empty frame from a minimal
  stub; prefer a minimal stub returning `RealtimeFrame { channels: vec![], diagnostics: vec![] }`
  so the test passes and pins the signature).

- [ ] **0.8 Build + lint the workspace clean.**

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo build && cargo test -p opentune-ini -p opentune-realtime && cargo clippy -- -D warnings`
Expected: PASS (every M2 literal now compiles with the new fields).

- [ ] **0.9 Commit.**

```bash
git add -A
git commit -m "feat(ini,realtime): freeze M3 Definition + realtime seams (output channels, gauges, frame event)"
```

---

## Task 1 — §9 owner-task migration: Tokio owner + command channel *(early — unblocks the poll loop)*

**Why here:** the realtime poll loop must live *inside* the single wire-owner. This is the
**deliberate M2 §9 deviation** ([m2-decisions.md](../../notes/m2-decisions.md) lines 64–76):
M2 used `Arc<Mutex<Option<Session>>>` + synchronous commands; M3 replaces it with one Tokio
owner task that *owns* `Session` and a `mpsc` command channel. **This is plumbing, not
behavior:** it **wraps** the existing synchronous `Session` — the `session.rs`/`connection.rs`
unit tests call `Session` methods directly and MUST stay green. Also folds in follow-up (c):
tune re-read on reboot-detected reconnect.

**Files:**
- Create: `src-tauri/src/owner.rs`
- Modify: `src-tauri/src/lib.rs` (manage the command sender instead of `SessionStore`;
  spawn the owner in `setup`), `src-tauri/src/commands.rs` + `src-tauri/src/tune_commands.rs`
  + `src-tauri/src/realtime_commands.rs` (become thin async senders)
- Modify: `src-tauri/src/connection.rs` (move `Session` ownership into the owner;
  `simulate_link_drop` routes through the owner)
- Test: `src-tauri/src/owner.rs` (`#[tokio::test]` unit tests)

**Interfaces:**
- Consumes: the existing synchronous `Session` and its methods (`load_tune`, `set_value`,
  `burn`, `undo`, `redo`, `read_values`, `eval_conditions`, `snapshot_tune`, `diff_tune`,
  `merge_tune`, `definition`) — unchanged.
- Produces:

```rust
// owner.rs
/// One request to the wire owner. Each carries a oneshot reply channel so the
/// async command facade can await the synchronous Session result.
pub enum Command {
    Connect { source: ConnectSource, reply: oneshot::Sender<Result<(), String>> },
    Disconnect { reply: oneshot::Sender<Result<(), String>> },
    SimulateLinkDrop { reply: oneshot::Sender<Result<(), String>> },
    GetDefinition { reply: oneshot::Sender<Result<DefinitionDto, String>> },
    LoadTune { reply: oneshot::Sender<Result<TuneDirtyEvent, String>> },
    GetValues { names: Vec<String>, reply: oneshot::Sender<Result<Vec<Value>, String>> },
    SetValue { name: String, value: Value, reply: oneshot::Sender<Result<TuneDirtyEvent, String>> },
    Burn { reply: oneshot::Sender<Result<TuneDirtyEvent, String>> },
    Undo { reply: oneshot::Sender<Result<TuneDirtyEvent, String>> },
    Redo { reply: oneshot::Sender<Result<TuneDirtyEvent, String>> },
    EvalConditions { exprs: Vec<String>, reply: oneshot::Sender<Result<Vec<bool>, String>> },
    SnapshotTune { reply: oneshot::Sender<Result<(), String>> },
    DiffTune { reply: oneshot::Sender<Result<Vec<FieldDiffDto>, String>> },
    MergeTune { picks: Vec<String>, reply: oneshot::Sender<Result<TuneDirtyEvent option, String>> },
    StartRealtime { reply: oneshot::Sender<Result<(), String>> },   // Task 6 fills the handler
    StopRealtime { reply: oneshot::Sender<Result<(), String>> },    // Task 6 fills the handler
}

/// The managed Tauri state: the command sender + an AppHandle for event emission.
pub type OwnerHandle = tokio::sync::mpsc::Sender<Command>;

/// Spawn the owner task. It holds `Option<Session>` and a realtime `polling` flag,
/// serves commands sequentially, runs blocking Session calls via spawn_blocking,
/// and (Task 6) ticks the realtime poll between commands when polling is on.
pub fn spawn_owner(app: tauri::AppHandle) -> OwnerHandle;
```

> **Confirmed (research):** WRITE FRESH — ARCHITECTURE §9 is the spec; LibreTune's owner/store
> is GPL-2-only (study only, ADR-0007). Serial transport (`serialport`) is blocking, so
> **wrap blocking `Session` calls in `tokio::task::spawn_blocking`** (Decision 1). The owner
> owns `Session` by value; each command matches, runs the synchronous method, and sends the
> result back over its oneshot channel.

- [ ] **1.1 Failing test `owner_serves_commands_sequentially`** (`#[tokio::test]`): spawn the
  owner with a test `AppHandle` shim (or an `emit` closure — mirror `connect.rs`'s test
  pattern of injecting an emit fn); send `Connect { Simulator }`, then `LoadTune`, then
  `SetValue("reqFuel", Scalar(12.5))`, awaiting each oneshot. Assert the `SetValue` reply is
  `Ok` with `dirty == true`. Run → RED (no `spawn_owner`).

- [ ] **1.2 Implement the owner loop.** `spawn_owner` spawns a `tokio::task` holding
  `session: Option<Session>` and `polling: bool`. Loop: `while let Some(cmd) =
  rx.recv().await`, match each variant, call the synchronous `Session` method **inside
  `spawn_blocking`** (move the session in, get it back out — use an `Option::take`/put-back
  or hold the session in a `tokio::task::spawn_blocking` closure that returns
  `(Session, Result)`), then `let _ = reply.send(result)`. `Connect`/`Disconnect`/
  `SimulateLinkDrop` construct/tear down the `Session` (reuse `connect_simulator`/
  `connect_serial`/`simulate_link_drop_async`'s logic, now inline in the owner). Emit
  `ConnectionStateEvent` + `TuneDirtyEvent` via the `AppHandle` after each relevant command.
  `StartRealtime`/`StopRealtime` just set/clear `polling` for now (Task 6 adds the tick).
  Run → GREEN.

- [ ] **1.3 Failing test `reboot_on_reconnect_invalidates_and_rereads_tune`** (follow-up c):
  Reboot detection is **secl-change-based** — `connect()` seeds `last_secl` from `read_secl()`,
  and a reconnect flags a reboot only when secl went *backwards*. So the test must simulate
  uptime **before** the drop or it exercises nothing: connect to the simulator, `LoadTune`,
  `SetValue` + `Burn` a value, then `simulator.advance_secl(50)` (simulate running uptime),
  then drive a link drop where the simulator's `secl` **resets to 0** (`reset_secl` — reboot).
  Assert that after reconnect the owner **re-reads the tune** (a fresh `load_tune`) rather than
  trusting the stale in-memory tune — assert the re-read tune reflects the burned (flash) value
  and `dirty == false`. Run → RED. *(Cross-check the existing M1 test
  `secl_reboot_triggers_reidentify` for the exact advance/reset sequence that makes
  `last_reconnect_caused_reidentify()` return true.)*

- [ ] **1.4 Failing test `glitch_on_reconnect_preserves_unburned_tune`** (the safety-critical
  twin — follow-up c is *reboot-detected* re-read, NOT re-read-on-every-reconnect): connect,
  `LoadTune`, `SetValue` an **unburned** edit (dirty == true), `simulator.advance_secl(10)`,
  then drive a link drop that does **not** reset secl (a cable glitch — secl stays continuous).
  Assert that after reconnect the in-memory tune is **preserved intact**, including the unburned
  edit (`dirty == true`, the edited value still present). Re-reading here would silently discard
  the user's unburned work and regress M1's "silent recovery". Run → RED.

- [ ] **1.5 Implement reboot-detected re-read.** In the owner's reconnect handling, after
  `reconnect_collect_states()`, check `ConnectionManager::last_reconnect_caused_reidentify()`
  (already exists — secl-reset ⇒ reboot). **Only when it reports a reboot**, call
  `session.load_tune()` to invalidate + re-read (the in-memory tune may diverge from a rebooted
  ECU) and emit the resulting `TuneDirtyEvent`. On a glitch reconnect (no reboot), leave the
  tune untouched. Confirm `reconnect_collect_states` + `last_reconnect_caused_reidentify()`
  actually expose the reboot-vs-glitch distinction both tests assume (they do — M1 relies on
  it). Run 1.3 + 1.4 → GREEN.

- [ ] **1.6 Rewire the Tauri commands as thin async senders.** Each
  `#[tauri::command] #[specta::specta] async fn` takes `State<'_, OwnerHandle>`, builds a
  `oneshot::channel()`, sends the matching `Command`, and `.await`s the reply
  (`rx.await.map_err(|_| "owner task gone".to_string())?`). `src-tauri/src/lib.rs`:
  `.manage(spawn_owner(app.handle().clone()))` in `setup` (replacing the
  `.manage(Arc::new(Mutex::new(None)))`); keep `Session` types where they are.

- [ ] **1.7 Run the full M2 + owner suite.**

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test`
Expected: PASS — every `session.rs`/`connection.rs`/`dto.rs` unit test still green (they call
`Session` directly, untouched), plus the new owner tests. `cargo clippy -- -D warnings` clean.

- [ ] **1.8 Commit.**

```bash
git add -A
git commit -m "refactor(app): Tokio owner-task + command channel (ARCHITECTURE §9); re-read tune on reboot-reconnect only"
```

---

## Task 2 — `ini`: parse `[OutputChannels]` + `ochBlockSize` *(roadmap: realtime output-channel decoding, INI half)*

**Files:** `src-tauri/crates/ini/src/output_channels.rs` (parser),
`src-tauri/crates/ini/src/parser.rs` (extend `parse_comms` for `ochBlockSize`),
`src-tauri/crates/ini/src/definition.rs` (wire the real parser);
fixture `src-tauri/crates/ini/tests/fixtures/speeduino-output-channels.ini`;
test `src-tauri/crates/ini/tests/output_channels.rs`.

**Interfaces:**
- Consumes: the M1/M2 tokenizer in `parser.rs`, `ScalarType` (existing), the `OutputChannelDef`
  type (Task 0).
- Produces: `parse_definition` populates `output_channels`; `parse_comms` fills `och_block_size`.

> **Confirmed (research):** PORT the *shape* from
> [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini) (MIT) — `parseOutputChannels`
> (ini.ts:235-266) reuses the same `parseConstAndVar` we already ported for `[Constants]` in
> M2 (`constants_fields.rs`), and stores computed channels' `{expr}` as an **opaque string**.
> hyper-tuner does **not** parse `[GaugeConfigurations]`/`[FrontPage]` — those are Task 3
> (fresh). Field order for a scalar: `name = scalar, TYPE, offset, "units", scale, translate`
> (no min/max/digits, unlike `[Constants]`). `bits`: `name = bits, TYPE, offset, [lo:hi]`.
> Computed: `name = { expr }` with optional trailing `, "units"`. `#if CELSIUS` blocks are
> already handled by the existing `preprocessor`.

**Fixture excerpt** (`speeduino-output-channels.ini` — trimmed, real-grammar; include a
computed channel that chains, and the header keys):

```ini
; source: reference speeduino.ini @ noisymime/speeduino 63fd68e9 (GPL-3), trimmed.
[OutputChannels]
ochGetCommand    = "r\$tsCanId\x30%2o%2c"
ochBlockSize     =  16

secl        = scalar, U08,  0, "sec",   1.000, 0.000
engine      = scalar, U08,  2, "bits",  1.000, 0.000
running     = bits,   U08,  2, [0:0]
rpm         = scalar, U16,  4, "rpm",   1.000, 0.000
coolantRaw  = scalar, U08,  6, "C",     1.000, 0.000
tps         = scalar, U08,  7, "%",     0.500, 0.000
coolant     = { coolantRaw - 40 }, "C"
throttle    = { tps }, "%"
```

- [ ] **2.1 Confirm the port source covers this surface** (ADR-0006 first sub-step): open
  hyper-tuner `src/ini.ts` `parseOutputChannels` (lines ~235-266) + `parseConstAndVar`;
  confirm it handles scalar/bits and stores computed as opaque strings. It does. Record the
  source + MIT license in the test module header. (If a surface is missing, write fresh + record.)

- [ ] **2.2 Write the fixture** (above) into
  `src-tauri/crates/ini/tests/fixtures/speeduino-output-channels.ini`.

- [ ] **2.3 Failing test `parses_output_channels_and_block_size`:**

```rust
let ini = include_str!("fixtures/speeduino-output-channels.ini");
let def = parse_definition(ini).expect("parses");
assert_eq!(def.comms.och_block_size, 16);
// scalar with offset + scale
match def.output_channel("rpm").unwrap() {
    OutputChannelDef::Scalar { kind, offset, units, scale, .. } => {
        assert_eq!(*kind, ScalarType::U16);
        assert_eq!(*offset, 4);
        assert_eq!(units, "rpm");
        assert!((scale - 1.0).abs() < 1e-9);
    }
    other => panic!("expected Scalar, got {other:?}"),
}
// bits over the `engine` byte
match def.output_channel("running").unwrap() {
    OutputChannelDef::Bits { offset, bit_lo, bit_hi, .. } => {
        assert_eq!((*offset, *bit_lo, *bit_hi), (2, 0, 0));
    }
    other => panic!("expected Bits, got {other:?}"),
}
// computed channel keeps its expression verbatim + trailing units
match def.output_channel("coolant").unwrap() {
    OutputChannelDef::Computed { expr, units, .. } => {
        assert_eq!(expr.trim(), "coolantRaw - 40");
        assert_eq!(units, "C");
    }
    other => panic!("expected Computed, got {other:?}"),
}
```

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-ini --test output_channels`
Expected: FAIL (stub parser returns empty).

- [ ] **2.4 Implement `parse_output_channels`** in `output_channels.rs`: iterate the
  `[OutputChannels]` section lines, split each `name = ...` on the first `=`. If the RHS
  starts with `{`, it's `Computed` — capture the text between `{` and `}` as `expr`, and an
  optional trailing `, "units"` after the `}`. Otherwise split the RHS on commas: token[0]
  `scalar`|`bits` decides the variant; map the type token → `ScalarType` (reuse the M2
  `constants_fields` type-map helper); parse offset; for `scalar` take `units`, `scale`,
  `translate` (parse to `f64` — these are literal in `[OutputChannels]`, not `{expr}`); for
  `bits` parse `[lo:hi]`. Unknown entry kind → `Diagnostic`, skip (graceful). Wire the call in
  `parse_definition`.

- [ ] **2.5 Implement `ochBlockSize` capture** in `parse_comms` (`parser.rs`): read the
  `ochBlockSize` key from `[OutputChannels]` as a `u32` (default `0` when absent). Keep the
  existing `ochGetCommand` capture unchanged.

- [ ] **2.6 Run the test → GREEN**, then a robustness test: an `[OutputChannels]` with an
  unknown entry kind (`foo = weird, ...`) records a `Diagnostic` and does not fail the parse;
  a computed channel with no trailing units → `units == ""`. Keep `tests/contract.rs` green.

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-ini`
Expected: PASS.

- [ ] **2.7 Commit.**

```bash
git add -A
git commit -m "feat(ini): parse [OutputChannels] + ochBlockSize (port hyper-tuner shape)"
```

---

## Task 3 — `ini`: parse `[GaugeConfigurations]` + `[FrontPage]`; expression bitwise ops *(realtime, gauge grammar)*

**Files:** `src-tauri/crates/ini/src/gauges.rs` (parser),
`src-tauri/crates/ini/src/expr_parser.rs` (extend: bitwise `&`, `<<`),
`src-tauri/crates/ini/src/definition.rs` (wire), fixture
`src-tauri/crates/ini/tests/fixtures/speeduino-gauges.ini`;
tests `src-tauri/crates/ini/tests/gauges.rs`, additions to `src-tauri/crates/ini/tests/expr.rs`.

**Interfaces:**
- Consumes: tokenizer, `Number` (M2), `GaugeDef`/`FrontPageDef`/`IndicatorDef` (Task 0), the
  expression evaluator (`expr_parser.rs`).
- Produces: `parse_definition` fills `gauges` + `frontpage`; `eval`/`eval_bool` gain bitwise
  `&`/`<<`.

> **Confirmed (research): WRITE FRESH** — hyper-tuner's section switch ends at `[Datalog]`; it
> does **not** parse `[GaugeConfigurations]` or `[FrontPage]` (ini.ts:146-190). Grammar is
> documented in the dossier §A. Record write-fresh per ADR-0006. Bitwise `&`/`<<` appear in
> real indicators/computed channels (`syncStatus = { halfSync + (sync << 1) }`,
> `{ sd_status & 1 }`) and the M2 evaluator lacks them — add them (small grammar extension)
> or degrade; adding is cheap and unblocks indicators. `bitStringValue(...)` in gauge `units`
> stays `UnsupportedFn` → captured as `Number::Expr`, degrades to `""` at render.

**Fixture excerpt** (`speeduino-gauges.ini` — trimmed real grammar):

```ini
; source: reference speeduino.ini @ noisymime/speeduino 63fd68e9 (GPL-3), trimmed.
[GaugeConfigurations]
gaugeCategory = "Main"
;Name       = Var,     Title,          Units, Lo, Hi,   LoD, LoW, HiW,  HiD,  vd, ld
tachometer  = rpm,     "Engine Speed", "RPM", 0,  8000,  300, 600, 7000, 7500, 0,  0
cltGauge    = coolant, "Coolant",      "C",   -40, 215, -15, 1,   95,   105,  0,  0

[FrontPage]
gauge1 = tachometer
gauge2 = cltGauge
indicator = { running }, "Not Running", "Running", white, black, green, black
```

- [ ] **3.1 Failing test `parses_gauges_and_frontpage`:**

```rust
let ini = include_str!("fixtures/speeduino-gauges.ini");
let def = parse_definition(ini).expect("parses");
let tach = def.gauges.iter().find(|g| g.name == "tachometer").unwrap();
assert_eq!(tach.channel, "rpm");
assert_eq!(tach.title, "Engine Speed");
assert_eq!(tach.category, "Main");
assert_eq!(tach.high, Number::Lit(8000.0));
assert_eq!(tach.hi_danger, Number::Lit(7500.0));
assert_eq!(def.frontpage.gauge_slots, vec!["tachometer", "cltGauge"]);
let ind = &def.frontpage.indicators[0];
assert_eq!(ind.expr.trim(), "running");
assert_eq!(ind.on_label, "Running");
assert_eq!(ind.on_bg, "green");
```

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-ini --test gauges`
Expected: FAIL (stub returns empty).

- [ ] **3.2 Implement `parse_gauges`** in `gauges.rs`: track the current `gaugeCategory` as a
  string carried down entries. For each `name = var, "title", units, lo, hi, loD, loW, hiW,
  hiD, vd, ld`, split on commas honoring quotes; the first token is the channel var; parse
  the 12 positional fields; any of units/lo/hi/loD/loW/hiW/hiD may be `{expr}` or a literal →
  parse each into `Number` (reuse the M2 number-or-expression helper); `vd`/`ld` → `u8`.
  Malformed row → `Diagnostic`, skip. Implement `parse_frontpage`: `gaugeN = name` lines fill
  `gauge_slots` in numeric slot order; `indicator = { expr }, off, on, offBg, offFg, onBg,
  onFg` fills `indicators`. Wire both into `parse_definition`.

- [ ] **3.3 Run → GREEN.** Robustness test: a `[GaugeConfigurations]` row referencing a
  `{ rpmhigh }` PcVariable in its `Hi` field parses to `Number::Expr("rpmhigh")` (not a hard
  error); a malformed gauge row records a `Diagnostic`.

- [ ] **3.4 Failing test `eval_supports_bitwise_and_and_shift`** in `expr.rs`:

```rust
let lookup = |n: &str| match n { "sync" => Some(1.0), "halfSync" => Some(1.0),
    "sd_status" => Some(3.0), _ => None };
assert_eq!(eval("halfSync + (sync << 1)", &lookup).unwrap(), 3.0);
assert_eq!(eval("sd_status & 1", &lookup).unwrap(), 1.0);
```

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-ini --test expr`
Expected: FAIL (`&`/`<<` unsupported).

- [ ] **3.5 Extend the evaluator** (`expr_parser.rs`): add `&` (bitwise AND) and `<<` (left
  shift) operators, operating on `f64` values cast to `i64` and back (`((lhs as i64) &
  (rhs as i64)) as f64`). Slot them into the precedence chain **below** comparison, **above**
  arithmetic (standard C precedence: shifts bind tighter than comparisons, `&` between
  equality and `&&`). Keep `UnsupportedFn` degradation for function calls unchanged. Run → GREEN.

- [ ] **3.6 Run the full ini suite.**

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-ini && cargo clippy -p opentune-ini -- -D warnings`
Expected: PASS.

- [ ] **3.7 Commit.**

```bash
git add -A
git commit -m "feat(ini): parse [GaugeConfigurations]/[FrontPage] (fresh) + bitwise & and << in expr"
```

---

## Task 4 — `protocol`: `read_output_channels(offset, len)` *(realtime wire read)*

**Files:** `src-tauri/crates/protocol/src/pages.rs` (add the method),
`src-tauri/crates/protocol/src/lib.rs` (extend `Protocol` trait);
test `src-tauri/crates/protocol/tests/output_channels.rs` (simulator-backed — depends on Task 5's
`'r'` arm; if run before Task 5, use a scripted transport that returns a full frame).

**Interfaces:**
- Consumes: `Transport`, `CommsSettings.och_get_command` + `och_block_size`, `expand_template`
  + `TemplateParams` (existing), `envelope_read_bytes` (existing, CRC-verified).
- Produces:

```rust
// on the Protocol trait
/// Read `len` bytes of the output-channel block starting at `offset` by
/// expanding `ochGetCommand` (which carries `%2o`/`%2c` windows) and reading the
/// response. In MsEnvelope10 the payload is `[SERIAL_RC_OK, ...len bytes]`; the
/// leading status byte is stripped. In Plain the response is `len` raw bytes.
fn read_output_channels(&mut self, offset: u16, len: u16) -> Result<Vec<u8>>;
```

> **Confirmed (research):** the `'r'` request is `[ 'r', $tsCanId(0), 0x30, offset_LE(2),
> len_LE(2) ]` — `expand_template` already handles `$tsCanId`, `\x30`, `%2o`(LE), `%2c`(LE)
> (pages.rs, with a unit test expanding exactly `"r$tsCanId\x30%2o%2c"`). Response payload:
> `[0]=0x00 SERIAL_RC_OK` then `len` block bytes (comms.cpp:359-374). `read_secl`
> (engine.rs:227-271) is the precedent but **inlines** the envelope read without CRC verify —
> the new method must reuse `send_page_command` / `envelope_read_bytes` (CRC-verified) instead.
> **Do NOT change `read_secl`** — M1/M2 reconnect relies on it sending `ochGetCommand`'s first
> byte against the sim's `'A'` fixture; leave that path alone.

- [ ] **4.1 Add the trait method** to `Protocol` in `lib.rs` (verbatim doc above); default the
  M1/M2 `MsProtocol` impl to delegate to a new `do_read_output_channels` in `pages.rs`.

- [ ] **4.2 Failing test `reads_full_och_frame`** (scripted transport for now): given a
  `CommsSettings` with `och_get_command = "r$tsCanId\x30%2o%2c"`,
  `envelope = MsEnvelope10`, drive `read_output_channels(0, 16)` against a scripted
  transport that returns a CRC-framed `[0x00, b0..b15]`. Assert the returned `Vec<u8>` is
  exactly `[b0..b15]` (status byte stripped, 16 bytes). Also assert the **request bytes** the
  transport received expand to `[ 'r', 0x00, 0x30, 0x00,0x00, 0x10,0x00 ]`. Run → RED.

- [ ] **4.3 Implement `do_read_output_channels`** in `pages.rs`: build
  `TemplateParams { page: 0, offset, count: len, value: &[], can_id: 0 }`,
  `expand_template(&self.comms.och_get_command, &params)?`, then `send_page_command(&payload,
  len as usize, MAX_PAGE_RESPONSE)`. In `MsEnvelope10`, strip the leading `0x00` status byte
  and return the remaining bytes (tolerate a short response — never index past what was
  received; `Vec::truncate`/`get(1..)` guards). In `Plain`, return the raw bytes. Run → GREEN.

- [ ] **4.4 Commit.**

```bash
git add -A
git commit -m "feat(protocol): read_output_channels(offset,len) via ochGetCommand (comms.cpp@63fd68e9)"
```

---

## Task 5 — `simulator`: animated engine model + `'r'`/0x30 windowed arm *(roadmap: animated correlated channels)*

**Files:** `src-tauri/crates/simulator/src/engine.rs` (new — animated model),
`src-tauri/crates/simulator/src/ecu.rs` (extend: `plain_command_len(b'r')`, an `'r'` arm in
`respond_plain`/`respond_crc`, first-och secl reset), `src-tauri/crates/simulator/src/lib.rs`
(extend); tests in `src-tauri/crates/simulator/tests/realtime.rs`.

**Interfaces:**
- Consumes: the `Definition` (for och channel offsets/sizes), `EngineSimulator` port structure.
- Produces: a `SimEngine` that writes correlated values into the INI-declared och offsets; the
  simulator answers `read_output_channels` with a live, animated frame; the first och request
  after connect resets `secl` to 0.

> **Confirmed (research): PORT** the animation state machine from
> [`askrejans/speeduino-serial-sim`](https://github.com/askrejans/speeduino-serial-sim) (MIT)
> — `EngineSimulator.{h,cpp}` is a mode state machine (STARTUP → WARMUP_IDLE → IDLE →
> LIGHT_LOAD → ACCELERATION → HIGH_RPM → DECELERATION → WOT) with correlated physics
> (`simulateRPM/Thermal/MAP/Throttle`, sensor noise, 20 Hz). **The animation model is
> separable from `SpeeduinoProtocol.cpp`** (its `EngineStatus` struct is the boundary) — port
> the *state machine + correlations*, but **write values into the INI-declared och offsets**
> (definition-driven), not its fixed 130-byte struct. The `'r'` **protocol** dispatch is
> fresh (memory.rs's port-note already established the protocol/memory side is written fresh
> against Speeduino `comms.cpp`; only the animation is a port). Record MIT source + this split
> in the module header.

- [ ] **5.1 Confirm the port split** (ADR-0006 first sub-step): confirm `EngineSimulator.h/.cpp`
  is separable from `SpeeduinoProtocol.cpp` (it is — the `EngineStatus` struct is the seam).
  Record source + MIT license + the "animation ported, protocol/offset-mapping fresh" note in
  `engine.rs`'s header.

- [ ] **5.2 Failing test `sim_animates_correlated_channels`:** build a `SimEngine` from the
  Task 2 output-channels fixture definition; tick it several times (`engine.tick(dt)`);
  read back the och block; decode `rpm`, `coolant`, `tps`. Assert (a) `rpm` is within a
  plausible running range (e.g. `> 0` after leaving STARTUP), (b) `coolant` **rises
  monotonically** across ticks during warm-up (correlation), (c) values land at the correct
  INI offsets (byte 4-5 for the U16 rpm). Run → RED.

- [ ] **5.3 Implement `SimEngine`** in `engine.rs`: port the mode state machine + correlation
  functions (RPM, thermal warm-up ramp, MAP↔throttle, a little noise) as a `tick(dt: Duration)`
  that advances state and computes physical values, then **encodes** each into the och block at
  its declared offset/type (raw = round((physical - translate) / scale), written per
  `ScalarType` + endianness — reuse or mirror the model codec's raw-encode logic). Store the
  och block in the `Pipe` (extend `Pipe` with `och_block: Vec<u8>` sized to `och_block_size`).
  Run → GREEN.

- [ ] **5.4 Failing test `sim_answers_r_command_windowed`:** via `MsProtocol` over the sim's
  transport (both `Plain` and `MsEnvelope10` comms), call `read_output_channels(0,
  och_block_size)` and `read_output_channels(4, 2)`; assert the full read returns
  `och_block_size` bytes and the windowed read returns exactly bytes 4-5 of the block
  (offset/len windowing incl. zero-pad past end). Run → RED.

- [ ] **5.5 Implement the `'r'` arm** in `ecu.rs`: add `b'r' => Some(7)` to
  `plain_command_len` (request is `cmd + $tsCanId(1) + subcmd(1) + %2o(2) + %2c(2)` = 7
  bytes — note the layout differs from `'p'`: byte[1] is tsCanId, byte[2] is subcmd 0x30, and
  offset/len are at [3-4]/[5-6]). Add an `'r'` arm to both `respond_plain` (raw `len` bytes,
  no status prefix) and `respond_crc` (`[0x00, ...len bytes]`) that windows into `och_block`
  via a new `MemoryImage`-style read (or a small `och_read(offset, len)` helper on `Pipe`),
  zero-padding past the end. On the **first** `'r'` request after connect, reset `secl` to 0
  (matches `generateLiveValues` comms.cpp:361-365) — track a `first_och_done: bool` on `Pipe`.
  Run → GREEN.

- [ ] **5.6 Run the simulator suite + confirm M1/M2 stay green.**

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test -p opentune-simulator && cargo clippy -p opentune-simulator -- -D warnings`
Expected: PASS (existing `'A'`/`'p'`/`'M'`/`'b'` tests unchanged; `read_secl`'s `'A'` path untouched).

- [ ] **5.7 Commit.**

```bash
git add -A
git commit -m "feat(simulator): animated engine model + 'r'/0x30 windowed och reads (port EngineSimulator, MIT)"
```

---

## Task 6 — `realtime` crate: decode + poll/coalesce loop; start/stop; get_values fail-open *(roadmap: polling loop, decoding, throttled events)*

**Files:** `src-tauri/crates/realtime/src/decode.rs` (new),
`src-tauri/crates/realtime/src/loop.rs` (new — the coalescing tick used by the owner),
`src-tauri/crates/realtime/src/lib.rs` (fill `decode_frame`);
`src-tauri/src/realtime_commands.rs` (new — `start_realtime`/`stop_realtime`),
`src-tauri/src/owner.rs` (add the poll tick + emit `RealtimeFrameEvent`),
`src-tauri/src/session.rs` (fix `read_values` fail-open — follow-up b),
`src-tauri/src/lib.rs` (register command + event + needles).
Tests: `src-tauri/crates/realtime/tests/decode.rs`, `src-tauri/src/session.rs` (add a test).

**Interfaces:**
- Consumes: `Definition` (channels), `opentune_ini::eval` (computed channels),
  `read_output_channels` (Task 4), the owner (Task 1).
- Produces: `decode_frame` (Task 0 signature); the owner emits `RealtimeFrameEvent` ≤30 Hz.

> **Confirmed (research):** WRITE FRESH — decode lives in `realtime` (Decision 2), evaluates
> computed channels via `ini::expr` with a lookup over already-decoded scalar/bits channels
> (Decision 3), emits full-frame `Vec<(String,f64)>` ≤30 Hz (Decision 4). **Fail-open per
> channel** (Global Constraint): a bad expr / short buffer records a diagnostic and skips —
> never blanks the frame. The **same rule** fixes `get_values` (follow-up b): one unresolvable
> constant must not error the whole call.

- [ ] **6.1 Failing test `decode_frame_scales_and_computes`** in `crates/realtime/tests/decode.rs`:
  build the Task 2 fixture definition; craft a raw block where `rpm`(U16@4) = 3000,
  `coolantRaw`(U08@6) = 80, `tps`(U08@7 scale 0.5) = 40. Assert `decode_frame` returns
  `rpm == 3000.0`, `tps == 20.0` (40 × 0.5), and the computed `coolant == 40.0` (`coolantRaw
  - 40` = 80 - 40). Run → RED.

- [ ] **6.2 Failing test `decode_frame_fails_open_on_bad_channel`:** add a computed channel
  `broken = { nonexistentVar * 2 }` to a hand-built definition; assert `decode_frame` returns
  the other channels intact and `broken` appears in `diagnostics` (not in `channels`). Run → RED.

- [ ] **6.3 Implement `decode_frame`** in `decode.rs`: first pass decodes every `Scalar`/`Bits`
  channel from the block (raw → physical via `scale`/`translate`, per `ScalarType` +
  endianness; guard every slice against the block length — short buffer ⇒ that channel goes to
  `diagnostics`). Build a `HashMap<String, f64>` of decoded values. Second pass evaluates each
  `Computed` channel via `opentune_ini::eval(expr, &lookup)` where `lookup` reads the map (and
  falls back to earlier computed results — evaluate in file order so chains like
  `cycleTime → revolutionTime → rpm` resolve). Any `ExprError` (incl. `UnsupportedFn`,
  `UnknownVar`) ⇒ push the channel name to `diagnostics`, skip. Run 6.1 + 6.2 → GREEN.

- [ ] **6.4 Implement the coalescing gate** in `loop.rs`: a `RealtimePoller` holding the last
  emit `Instant` and the min emit interval (`Duration::from_millis(33)` ≈ 30 Hz). To keep
  `realtime` **decode-only** (no `opentune-protocol` dependency — Task 0.4 added only
  `opentune-ini`), the poller takes the raw block via a closure rather than a `Protocol`:
  `pub fn poll_once(&mut self, read_block: impl FnOnce() -> Result<Vec<u8>, RealtimeError>,
  def: &Definition) -> Result<Option<RealtimeFrame>, RealtimeError>`. It calls `read_block`,
  `decode_frame`s the result, and returns `Ok(Some(frame))` only if ≥ the emit interval has
  elapsed since the last emit (else `Ok(None)` — acquire but don't emit). The owner (6.5)
  supplies the closure as `|| proto.read_output_channels(0, block_size).map_err(...)`. Poll
  interval is 40 ms (25 Hz) driven by the owner; the 33 ms gate is the ≤30 Hz UI coalesce.
  Unit-test the gate with a stub `read_block`: two `poll_once` calls < 33 ms apart emit at most
  once.

- [ ] **6.5 Wire the poll into the owner** (`owner.rs`): when `polling == true`, the owner's
  select loop also arms a `tokio::time::interval(Duration::from_millis(40))`; on each tick, if
  no command is pending, run `poll_once` **inside `spawn_blocking`** (it touches the wire —
  serialized with commands, so a write/burn always preempts a poll), and emit
  `RealtimeFrameEvent { channels }` via the `AppHandle` when a frame comes back.
  `StartRealtime`/`StopRealtime` set/clear `polling`.

- [ ] **6.6 Add `start_realtime`/`stop_realtime` commands** (`realtime_commands.rs`) as thin
  async senders (Task 1 pattern). Register both in `collect_commands!`, register
  `RealtimeFrameEvent` in `collect_events!`, and add needle assertions
  (`"startRealtime"`, `"stopRealtime"`, `"RealtimeFrameEvent"`) to the `binding_gen` test in
  `lib.rs`.

- [ ] **6.7 Fix `get_values` fail-open** (follow-up b) in `session.rs::read_values`: instead of
  `.collect()` into a `Result` (all-or-nothing — one bad constant blanks the whole panel), map
  each name to `tune.get(n).unwrap_or(Value::Scalar(f64::NAN))` so one unresolvable constant
  degrades to a sentinel while every other value still renders. This keeps the IPC shape stable
  (`Vec<Value>`, one per requested name). **Note the JSON caveat:** `serde_json` serializes
  `f64::NAN` as `null`, and `Field.tsx`'s `value.Scalar ?? 0` would render it as `0` — Task 7.6
  handles the `Scalar === null` case to show "—" instead. Add a test: a definition with one
  `Number::Expr`-scaled constant that can't resolve does **not** error `read_values`; the other
  values come back intact and the unresolvable one is the sentinel.

- [ ] **6.8 Run everything.**

Run: `. "$HOME/.cargo/env" && cd src-tauri && cargo test && cargo clippy -- -D warnings`
Expected: PASS (incl. the regenerated `bindings.ts` containing `RealtimeFrameEvent`,
`startRealtime`, `stopRealtime`).

- [ ] **6.9 Commit.**

```bash
git add -A
git commit -m "feat(realtime): decode frames + coalesced ≤30 Hz poll loop; start/stop; get_values fail-open"
```

---

## Task 7 — Frontend: canvas gauges + editable/persisted dashboard; Field commit-on-blur *(roadmap: gauge dashboard)*

**Files:** `src/components/gauges/{RoundGauge,BarGauge,DigitalGauge,IndicatorLamp,GaugeCanvas}.tsx`,
`src/components/gauges/gauges.css`, `src/components/dashboard/{Dashboard,GaugeBinder}.tsx`,
`src/stores/realtime.ts` (new — reflect-only), `src/components/dialogs/Field.tsx`
(commit-on-blur), `src-tauri/src/layout.rs` (new — JSON persistence) +
`save_layout`/`load_layout` commands + `App.tsx` (listen for `RealtimeFrameEvent`); i18n dicts.
Bindings **regenerated**, not hand-written.

**Interfaces:**
- Consumes: `DefinitionDto` (extend the DTO with gauges/frontpage — see 7.1), the
  `RealtimeFrameEvent` stream, `save_layout`/`load_layout` commands.
- Produces: a live dashboard; a reflect-only realtime store read imperatively by canvas gauges.

> **Confirmed (research): WRITE FRESH** hand-rolled canvas gauges (ARCHITECTURE §3 mandates
> custom canvas; canvas-gauges MIT is a *visual* reference only). **30 Hz must bypass React
> reconciliation:** the realtime store is reflect-only and gauges read it via a vanilla Zustand
> `subscribe`/`requestAnimationFrame` + `getState()` loop, NOT a selector that re-renders
> (App.tsx's inline-`useEffect`-listen pattern). Layout persistence = minimal JSON in the app
> config dir (Decision 6), not the `project` crate.

- [ ] **7.1 Extend `DefinitionDto`** (`src-tauri/src/dto.rs`) with the gauge/frontpage
  projection the UI needs: a `GaugeDto { name, channel, title, units: Option<f64-or-string?>,
  low, high, lo_danger, lo_warn, hi_warn, hi_danger, value_digits, label_digits, category }`
  (numeric bounds as `Option<f64>` — `lit()` helper, `None` for `{expr}` bounds, like
  `ConstantDto`) and a `FrontPageDto { gauge_slots: Vec<String>, indicators: Vec<IndicatorDto> }`.
  Add both to `DefinitionDto`. Update the `From<&Definition>` impl. Regenerate bindings; add
  needles (`"GaugeDto"`, `"FrontPageDto"`) to the `binding_gen` test.

- [ ] **7.2 Vitest the reflect-only realtime store** (`src/stores/realtime.ts`): `applyFrame`
  sets a `Record<string, number>` channel map from a `RealtimeFrameEvent`; a getter
  `getChannel(name)` reads it. Test: applying a frame updates the map; the store never
  computes physics. Implement the store (no selector subscriptions in the hot path — expose
  `getState()`-style access for the rAF loop).

- [ ] **7.3 Vitest `RoundGauge`/`BarGauge` render logic** (pure helpers): a
  `gaugeGeometry(value, low, high) -> angle`/`fillFraction` pure function and a
  `zoneColor(value, loD, loW, hiW, hiD) -> "danger"|"warn"|"ok"` classifier. Test boundaries
  (value at `hi_warn` → warn; above `hi_danger` → danger; mid-range → ok). Implement the pure
  helpers first (RED → GREEN), then the canvas components draw with them.

- [ ] **7.4 Implement the canvas gauge components + the rAF read loop.** `GaugeCanvas` mounts a
  `<canvas>`, and on mount starts a `requestAnimationFrame` loop that reads
  `useRealtimeStore.getState().getChannel(channel)` and repaints — **no React state per
  frame**. `RoundGauge`/`BarGauge`/`DigitalGauge` wrap it with gauge-specific drawing;
  `IndicatorLamp` reads a boolean channel (backend already emits the indicator expr result as a
  channel, or the frontend evaluates the indicator's bound bit channel from the frame map).
  Use `tokens.css` colors for ok/warn/danger zones. Record the "hand-rolled, no dep" reason in
  a comment.

- [ ] **7.5 Implement the editable, persisted `Dashboard`.** Render the `FrontPageDto`
  `gauge_slots` as a grid of gauges bound to their `GaugeDto`. A `GaugeBinder` lets the user
  change a slot's bound channel/gauge (a `<select>` over `definition.gauges`) and rearrange
  slots (a minimal drag or up/down buttons — keep it simple, no dnd dep). Persist the layout
  (slot→gauge mapping + any reordering) via `commands.saveLayout(json)`; load it on connect via
  `commands.loadLayout()`. Backend `layout.rs`: `save_layout(json: String)` writes to
  `<app config dir>/dashboard-layout.json` (via `tauri::path` / `dirs`), `load_layout() ->
  Option<String>` reads it back. Register both commands + needles.

- [ ] **7.6 Field commit-on-blur** (follow-up a) in `src/components/dialogs/Field.tsx`: the
  `Scalar` number input must **not** write to the ECU on every keystroke. Hold the in-progress
  text in local `useState`; call `onChange` only on `onBlur` (or Enter keydown), and reset
  local state from `value` when the prop changes. Also handle the fail-open sentinel from Task
  6.7: when `value.Scalar` is `null` (a `NaN` an unresolvable constant produced, serialized to
  JSON `null`), render an empty/"—" display instead of `0` — do **not** treat `?? 0` as the
  value for a `null` scalar. Update `Field.test.tsx`: typing does not call `onChange`; blurring
  does; a `null` scalar shows "—". This closes the per-keystroke-write risk before realtime adds
  serial-write contention.

- [ ] **7.7 Wire the realtime event + i18n.** In `App.tsx` (or `Dashboard`), add an inline
  `useEffect` listening to `events.realtimeFrameEvent.listen((e) =>
  useRealtimeStore.getState().applyFrame(e.payload))` (mirrors the existing
  `connectionStateEvent` listener). Add EN + PL i18n keys: `dashboard.title`,
  `dashboard.startLive`, `dashboard.stopLive`, `dashboard.bindChannel`, `dashboard.noGauges`,
  `dashboard.editLayout`, `dashboard.saveLayout` (PL mirrors EN keys exactly).

- [ ] **7.8 Run the frontend + backend suites.**

Run: `pnpm test` (or `npm test`) and
`. "$HOME/.cargo/env" && cd src-tauri && cargo test`
Expected: PASS. `pnpm lint && pnpm prettier --check .` (or the repo equivalents) clean.

- [ ] **7.9 Commit.**

```bash
git add -A
git commit -m "feat(app): canvas gauge dashboard (bindable, persisted layout); Field commit-on-blur"
```

---

## Task 8 — End-to-end demo: live gauges from the simulator with link-drop recovery *(roadmap demo)*

**Files:** `src/components/dashboard/Dashboard.e2e.test.tsx` (or a vitest integration test
against the simulator command path); no new production files.

**Interfaces:**
- Consumes: everything from Tasks 1–7 wired together.
- Produces: the M3 demo — a live, configurable dashboard driven by the simulator, surviving a
  simulated link drop.

- [ ] **8.1 E2E (simulator, no hardware):** connect to the simulator → `loadTune` +
  `getDefinition` (now carrying gauges/frontpage) → `startRealtime` → assert the dashboard
  receives `RealtimeFrameEvent`s and at least one gauge's bound channel updates over time
  (values change across frames). Bind a different channel to a slot → assert it re-binds and
  animates. `stopRealtime` → assert frames stop.

- [ ] **8.2 E2E link-drop recovery:** while realtime is running, trigger
  `simulateLinkDrop` → assert the connection store shows `Reconnecting` then `Connected`,
  the tune is re-read (reboot ⇒ secl reset, Task 1.4), and realtime frames **resume** after
  reconnect without an app restart. **This is the M3 demo.**

- [ ] **8.3 Manual `tauri dev` GUI smoke run** (step, not a task): run
  `pnpm tauri dev` (or the repo dev command), connect to the simulator, click "Start live",
  visually confirm the gauges animate, bind a channel, trigger "Simulate connection drop",
  and confirm the gauges recover. Record the observed behavior in the commit body. (This is a
  human-in-the-loop confidence check — automate what you can in 8.1/8.2, but do this once by
  hand before declaring the milestone done.)

- [ ] **8.4 Run the full gate.**

Run: `pnpm test && . "$HOME/.cargo/env" && cd src-tauri && cargo test && cargo clippy -- -D warnings && cargo fmt --check`
Expected: PASS.

- [ ] **8.5 Commit.**

```bash
git add -A
git commit -m "test(app): E2E live gauge dashboard demo with link-drop recovery (M3)"
```

---

## Self-review

**Spec coverage (M3 roadmap bullets + follow-ups):**
- `realtime` polling loop → Task 6 (`loop.rs`, owner tick). ✅
- `realtime` output-channel decoding → Task 2 (`ini` parse) + Task 6 (`decode.rs`). ✅
- `realtime` throttled events → Task 6 (33 ms coalesce gate, `RealtimeFrameEvent`). ✅
- `simulator` animated, correlated realtime channels → Task 5 (`engine.rs` port). ✅
- Frontend gauge dashboard: canvas gauges bindable to channels → Task 7 (7.3/7.4/7.5). ✅
- Editable layout saved with the project → Task 7.5 (minimal JSON persistence; `project` crate
  deferred, recorded). ✅
- Demo: live gauges from simulator + link-drop recovery → Task 8. ✅
- §9 owner-task/channel migration (m2-decisions deviation) → Task 1. ✅
- Follow-up (a) commit-on-blur → Task 7.6. ✅
- Follow-up (b) get_values fail-open → Task 6.7. ✅
- Follow-up (c) reboot-*detected* tune re-read (and glitch → preserve unburned tune) →
  Task 1.3/1.4/1.5. ✅
- Follow-up (d) manual `tauri dev` smoke → Task 8.3 (a step). ✅
- Bitwise `&`/`<<` for indicators → Task 3.4/3.5. ✅
- `read_output_channels` wire read → Task 4. ✅

**Parallelism map (after Task 0 freezes the seams, and Task 1 lands the owner):**
- Task 0 is the gate (frozen contracts + struct-literal fixups).
- Task 1 (owner migration) is the **second gate** — Task 6's loop lives inside the owner, and
  the frontend event wiring (Task 7.7) needs the emit path. Land it before 6/7/8.
- Tasks 2, 3 (both `ini`) can run in parallel with Task 4 (`protocol`) and Task 5 (`simulator`)
  against the frozen Task-0 types — but Task 3 depends on Task 2's fixture-parsing being
  wired (same `parse_definition`), and Task 5's `sim_animates` test consumes Task 2's fixture,
  so 2 → {3, 5}. Task 4 is independent (scripted transport) but its integration test wants
  Task 5's `'r'` arm.
- Task 6 depends on 2 (channels), 4 (wire read), 5 (sim data), 1 (owner). Task 7 depends on
  6 (event) + 0 (DTO). Task 8 depends on all.
- Each seam is pinned by a contract/needle test so an accidental shape change breaks a fast
  test before integration. **Note (per m2-decisions):** M2 ran tasks sequentially in the main
  checkout despite the parallelism map — same is fine here given a single implementer and a
  shared `Cargo.lock`.

**Placeholder scan:** no `TBD`/`TODO`/"add error handling"/"similar to Task N" — every code
step shows the code or the exact assertion; every fixture is included inline; every command is
exact and cargo-env-prefixed. `todo!()` appears only in Task 0 stub bodies, which is the
frozen-contract pattern (bodies filled in later tasks) — matching M2 Task 0.

**Type consistency:** `OutputChannelDef` (variants `Scalar`/`Bits`/`Computed`),
`GaugeDef.channel`, `FrontPageDef.gauge_slots`/`indicators`, `IndicatorDef`,
`och_block_size: u32`, `ChannelValue`/`RealtimeFrame`/`decode_frame`, `RealtimeFrameEvent
{ channels: Vec<(String, f64)> }`, `read_output_channels(offset: u16, len: u16)`,
`OwnerHandle`/`Command`, `RealtimePoller::poll_once`, DTO `GaugeDto`/`FrontPageDto` — names are
used identically in every task that references them. `Number` (M2), `ScalarType` (M2), and
`Value` (M2) are reused, not redefined. (Note: the `Command::MergeTune` reply type in Task 1's
interface block reads `Result<TuneDirtyEvent option, String>` — implement as
`Result<Option<TuneDirtyEvent>, String>`, matching `Session::current_dirty_event`.)

**Ported-source ledger (ADR-0006) — confirmed against the research dossier + code:**
- Task 2 `[OutputChannels]` → **PORT `hyper-tuner/ini` (MIT)** — `parseOutputChannels` reuses
  the `parseConstAndVar` shape M2 ported; computed channels stored as opaque strings.
- Task 3 `[GaugeConfigurations]`/`[FrontPage]` → **WRITE FRESH** (hyper-tuner's section switch
  ends at `[Datalog]`; grammar from dossier §A). Bitwise `&`/`<<` → **WRITE FRESH** (small
  grammar extension; keep `UnsupportedFn` degradation).
- Task 4 `read_output_channels` → **FRESH, byte-confirmed** against Speeduino `comms.cpp`
  @ `noisymime/speeduino@63fd68e9` (GPL-3, truth-source); reuses existing `expand_template` +
  `envelope_read_bytes`. `read_secl` left untouched.
- Task 5 simulator: **animation PORT** from `askrejans/speeduino-serial-sim` (MIT,
  `EngineSimulator.{h,cpp}`), re-targeted into INI-declared och offsets; **protocol/`'r'` arm
  FRESH** (Speeduino `comms.cpp`, matching the existing memory.rs write-fresh precedent).
- Task 1 owner-task/channel → **WRITE FRESH** (ARCHITECTURE §9 is the spec; LibreTune is
  GPL-2-only — study only, ADR-0007).
- Task 7 canvas gauges → **WRITE FRESH** (ARCHITECTURE §3 mandates custom canvas; canvas-gauges
  MIT is a visual reference only).
Each task's first commit records source + license; Task 3/5 first sub-steps re-confirm the
port source covers the surface before porting (ADR-0006 escape clause).

**Known risks to watch:**
1. **Concurrency migration is the real M3 cost** (dossier risk #1). Mitigation: Task 1 lands
   first and **wraps** the synchronous `Session` (owner *above* Session), pinned by every M2
   test staying green (Task 1.6). If a M2 test breaks, the migration changed behavior — stop
   and fix the wrapping, don't edit the test.
2. **INI/firmware skew** (`ochBlockSize` 139 vs `LOG_ENTRY_SIZE` 138). Mitigation: decode
   guards every slice against the received buffer length (Task 6.3); `read_output_channels`
   tolerates a short response (Task 4.3); sim zero-pads past the block end (Task 5.5).
3. **Computed-channel coverage** — `bitStringValue`/`arrayValue`/`smoothBasic`/`stringValue`/
   `timeNow` are unsupported; `&`/`<<` are added (Task 3). Mitigation: fail-open per channel
   (Task 6.2/6.3) — one bad expr never kills the dashboard; the ~74/78 arithmetic channels and
   all 8 FrontPage default gauges work.
4. **20–30 Hz through the WebView** → re-render storms. Mitigation: coalesce in Rust (Task 6.4)
   **and** render gauges imperatively on canvas off a reflect-only store (Task 7.2/7.4) — no
   React state per frame.
5. **`read_secl` vs `read_output_channels`** — `ochGetCommand` is `"r$tsCanId\x30%2o%2c"`, not
   `"A"`; `read_secl` inlines a first-byte read. Mitigation: leave `read_secl` untouched (M1/M2
   reconnect + the sim's `'A'` fixture depend on it); realtime uses the new `read_output_channels`
   (Task 4) with a dedicated fixture/sim `'r'` arm (Task 5). The first-och secl reset is
   modeled in the sim (Task 5.5) so reconnect resync expects it.
6. **Struct-literal ripple** — adding `Definition`/`CommsSettings` fields breaks every literal.
   Mitigation: Task 0.1 greps all sites, 0.6 fixes them, 0.8 proves the workspace builds before
   any feature work.
7. **`binding_gen` needle drift** — new commands/events must be in `collect_*!` **and** the
   needle tests (`startRealtime`/`stopRealtime`/`RealtimeFrameEvent`/`GaugeDto`/`FrontPageDto`).
   Mitigation: explicit steps (6.6, 7.1, 7.5).
