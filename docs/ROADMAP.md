# Roadmap

This roadmap turns the [architecture](ARCHITECTURE.md) into an ordered sequence of
milestones. Each milestone is meant to be **independently demonstrable** and to
build on the previous one. Dates are intentionally omitted — this is an
open-source project; milestones ship when they're ready.

Legend: ⬜ not started · 🟦 in progress · ✅ done

> **Research-driven priorities.** Market & user research
> ([market-and-user-research.md](research/market-and-user-research.md)) validated
> *which* features actually matter to TunerStudio users, and which would
> differentiate OpenTune. Two findings reshaped this roadmap:
>
> 1. **Reliability is a headline feature, not polish.** The #1 user complaint is
>    connections dropping mid-tune with no auto-reconnect. Reconnect-with-resync is
>    pulled into **M1** as a first-class requirement of the transport/realtime
>    layers, not deferred.
> 2. **Ship the things users beg for that TunerStudio still lacks** — tune
>    **diff/merge** (M2), two-log **scatter compare** and a GUI **math-channel**
>    library (M5). These are explicit, long-standing unmet needs.
>
> Per [ADR-0006](adr/0006-reuse-existing-parsers.md), parser/format work
> (`ini`, `datalog`, protocol, simulator) should **port from proven open
> references** rather than re-derive specs — this de-risks the critical path that
> sank prior projects.

---

## M0 — Foundations ✅

Goal: a buildable, runnable, empty-but-real Tauri app and the dev infrastructure.

- ✅ Tauri v2 app skeleton (`src/` + `src-tauri/`) that builds on macOS/Win/Linux.
- ✅ Rust workspace with empty `ini`, `model`, `protocol`, `transport`,
  `realtime`, `datalog`, `project`, `simulator` crates.
- ✅ Frontend skeleton (React + TS + Vite), theming, i18n (PL/EN) scaffolding.
  (Routing deferred — one screen; added when a second screen exists.)
- ✅ Typed IPC plumbing (`app_info` command + `Heartbeat` event) with TS types
  **generated** from Rust via `tauri-specta` (the no-hand-duplication guardrail).
- ✅ CI: build + test + lint/format (Cargo tests, clippy, rustfmt, Vitest,
  eslint, prettier) on **macOS + Linux + Windows**. Repository text files are
  normalized to LF through `.gitattributes` so Prettier is deterministic on
  every runner.
- ✅ `CONTRIBUTING.md` dev setup verified end-to-end.

**Demo:** the app builds to an empty shell; the full lint/test/build gate suite is
green locally and runs in CI on macOS, Linux, and Windows. Implemented via the
[M0 plan](superpowers/plans/2026-06-21-m0-foundations.md).

## M1 — Connect & identify ✅

Goal: parse an INI, connect to a (simulated) ECU, and confirm identity.

> Planned in the [M1 plan](superpowers/plans/2026-06-21-m1-connect-identify.md);
> the cross-crate contracts (`transport::Transport`, `ini::CommsSettings`,
> `protocol` identity types) are landed and test-pinned so the component agents
> build in parallel.

- ✅ `ini` crate: parse enough of a real INI to extract comms settings + signature
  (port from an open reference per [ADR-0006](adr/0006-reuse-existing-parsers.md)).
- ✅ `transport`: serial port enumeration + open/close; `SimTransport`.
- ✅ `protocol`: signature/version query; generic MS/TS handshake.
- ✅ **Reliable reconnect (pain point #1):** detect drops, auto-reconnect with
  backoff, and resync the firmware second-counter (`secl`) so the link survives
  USB power-save / cable glitches instead of needing an app restart.
- ✅ `simulator`: minimal virtual ECU (answers signature/version); able to simulate
  a dropped link for reconnect testing.
- ✅ UI: pick port + INI, Connect/Disconnect, show signature & connection state
  (including a clear reconnecting state).

**Demo:** connect to the simulator (and, for testers, a real Speeduino), see its
signature, and watch the app silently recover from a simulated connection drop.

## M2 — Read, edit & burn the tune ✅

Goal: full configuration editing — the core of a tuning tool.

- ✅ `ini`: full constants/pages parsing; expression evaluator; dialogs/menus.
- ✅ `model`: build `Tune` from pages; typed scaled accessors; dirty/undo-redo;
  RAM-vs-flash state.
- ✅ `protocol`: page read/write, page activation, burn, CRC variants.
- ✅ `simulator`: backing memory image for page read/write/burn.
- ✅ Frontend **data-driven dialog engine**: render menus/dialogs/fields with
  conditional visible/enable; edit values → live write.
- ✅ **Tune diff/merge (most-requested missing TS feature):** compare two tunes
  (current vs. file/snapshot), show per-setting and per-table-cell differences, and
  selectively merge individual changes. Builds naturally on the `model`'s
  field-level dirty tracking.

**Demo:** open a tune, change settings through auto-generated dialogs, write live,
burn to flash, undo/redo — then diff against a saved tune and merge in selected
changes.

## M3 — Real-time dashboard ✅

Goal: live gauges — the other half of day-to-day tuning.

- ✅ `realtime`: polling loop (25 Hz owner tick), output-channel decoding
  (fail-open per channel), throttled events (≤30 Hz coalesced
  `RealtimeFrameEvent`).
- ✅ `simulator`: animated, correlated realtime channels (ported
  `EngineSimulator` state machine, MIT) behind windowed `'r'`/0x30 reads.
- ✅ Frontend **gauge dashboard**: hand-rolled canvas gauges (rAF off React
  reconciliation), bindable to channels, editable layout persisted to the
  app config dir. Dashboard and tune store survive link glitches.
- ✅ ARCHITECTURE §9 owner-task/command-channel migration (closes the M2
  deviation); reboot-vs-glitch reconnect semantics incl. the secl
  false-reboot fix.

**Demo:** a live, configurable dashboard driven by the simulator, surviving a
simulated link drop (owner-level E2E; real-ECU serial polling deferred to the
M3-serial follow-up — see `docs/notes/m3-decisions.md`).

## M4 — Table editors & auto-tune ✅

Goal: edit VE/ignition/AFR tables and improve them from data.

- ✅ 2D heatmap table editor: DOM grid, full pinned keymap, TSV clipboard
  round-trip, interpolate/smooth/scale/set-equal ops, cell writes via
  `set_cells` (one gesture = one undo step).
- ✅ 3D surface view (three.js) with a live operating-point overlay — lazy
  `React.lazy` chunk so three.js never lands in the eager bundle
  (WKWebView-hardened: capped pixel ratio, context-loss recovery), the dot
  driven by the M3 realtime store off a zero-per-frame-allocation rAF loop.
- ✅ Curve editors (1D) — reuse the table editor's grid/selection/ops/TSV
  machinery (a curve is a `Grid` with `rows: 1`) + an SVG polyline preview
  with a live cursor.
- ✅ First auto-tune (VE analyze) as the **first consumer of the deterministic
  `analysis::ve_analyze`** (the `analysis` crate lands in M5; AutoTune introduces
  its first capability here as `opentune-analysis`, seeded with `ve_analyze`
  only). **Deterministic and auditable** (same log → same result; visible
  data filtering and per-cell confidence) — directly addressing the #1
  complaint about TunerStudio's VE Analyze (non-deterministic, can tune the
  wrong way). Fed by the owner-side realtime capture ring; the AutoTune UI
  renders the delta/confidence heatmap and filtered-sample counts and applies
  corrections back through `set_cells`. No AI involved. See the
  [AI tuning & analysis design](superpowers/specs/2026-06-21-ai-tuning-and-analysis-design.md).

**Demo:** an owner-level E2E (`ve_analyze_flattens_the_sim_ve_error`) connects to
the simulator, seeds a deliberately-wrong flat VE table against the sim's hidden
true-VE surface, captures a live session, analyzes, applies the confident
corrections, re-measures under the same operating conditions, and proves the
seeded error more than halves — "tune a VE table in 2D/3D and apply a
data-driven correction, with a clear view of which logged data drove each
cell." Interactive `tauri dev` GUI walkthrough not exercised this pass (no
native-app driver available) — see `docs/notes/m4-decisions.md`.

## M5 — Datalogging & analysis ⬜

Goal: record, replay, and analyze.

- ⬜ **`analysis` crate (deterministic core):** pure, side-effect-free,
  deterministic, auditable capabilities — `ve_analyze`, `virtual_dyno`,
  `log_stats`, `detect_anomaly`. One engine consumed by AutoTune, the UI, and
  (later) the AI layer. See the
  [design doc](superpowers/specs/2026-06-21-ai-tuning-and-analysis-design.md).
- ⬜ **Virtual dyno:** `analysis::virtual_dyno` estimates WHP/torque curves from a
  log + vehicle parameters; deterministic and auditable (shows conditions and
  assumptions). UI dyno view consumes it.
- ⬜ `datalog`: CSV writer first, then MLG read/write (port from an open reference;
  note only MLG v1 has a published byte-level spec — see
  [ADR-0006](adr/0006-reuse-existing-parsers.md) and the research doc).
- ⬜ Frontend datalog viewer (uPlot time-series + scatter), playback synced to
  the dashboard.
- ⬜ **Two-log scatter compare (missing in MegaLogViewer):** overlay/compare two
  logs in scatter and time-series views with shared, user-controllable axes — the
  thing users currently export to Excel for.
- ⬜ **GUI math-channel library (vs TS raw string expressions):** common derived
  channels (derivatives, smoothing/filters, data-gating) from a UI, not just a
  free-text expression box.
- ⬜ Analysis tooling (markers, export).
- ⬜ **Performance target:** smooth zoom/pan on 100k+ record logs (TS/MLV's known
  weak spot) — validate against large fixtures.

**Demo:** record a session, replay it, compare two logs, and build a derived
channel — all without leaving the app or touching Excel.

## M6 — Interop, polish & first release ⬜

Goal: a real 1.0 people can use instead of TunerStudio for common workflows.

- ⬜ `.msq` import/export verified against TunerStudio.
- ⬜ Validate against multiple firmwares (Speeduino, rusEFI, an MS family member).
- ⬜ Signed, notarized macOS builds; signed Windows builds; Linux AppImage/deb.
- ⬜ Auto-update; first-run/onboarding; documentation site.
- ⬜ Accessibility & i18n pass (Polish + English).

**Demo:** download a signed build, open an existing tune/log, and work end-to-end.

## M7 — AI assistant & MCP server ⬜

Goal: the differentiator — AI that analyzes live data and logs and helps tune,
built *on top of* the deterministic core (base first, AI second). Ships at the
`advisory` authority level. Full design:
[AI tuning & analysis design](superpowers/specs/2026-06-21-ai-tuning-and-analysis-design.md).

- ⬜ **Tool registry:** expose `analysis` capabilities as schema'd tools
  (read-only: analysis/read; mutating: `propose_change`/`apply_change`/`burn`).
- ⬜ **Permission policy:** `advisory` (default, ships now) → `assisted` →
  `autonomous` (latter two designed for, deferred). Authority is configuration, not
  hardcoded.
- ⬜ **Guardrails in the tool layer** (not the prompt): validate every change vs INI
  `low`/`high`, rate-limit change magnitude, require a healthy connection, audit
  every AI action.
- ⬜ **Provider abstraction:** `AiProvider` trait; BYOK cloud (Claude/OpenAI/…),
  **off by default, opt-in** — preserves offline-first/privacy-by-default. Local
  models addable later.
- ⬜ **Embedded assistant:** chat/assistant panel wired to the tool registry — for
  live in-car tuning.
- ⬜ **MCP server:** OpenTune as an MCP server so external agents can connect — same
  registry, same guardrails; the path toward future autonomy.

**Demo:** opt in with an API key, ask the assistant to analyze a log, and have it
propose an auditable VE correction (advisory — user applies it). Optionally connect
an external agent over MCP.

## Beyond 1.0 (candidate ideas)

- **AI authority levels `assisted` / `autonomous`** — unlock mutating writes
  behind explicit opt-in and hard safeguards; the autonomous apply→observe→correct
  loop on the same deterministic engine.
- **Sensor/component database** — pick a sensor/injector/MAP model and have its
  calibration parameters populate the matching INI constants, instead of hunting
  for them.
- **Local AI models** via the provider abstraction (no data leaves the machine).
- CAN bus transport; Wi-Fi/BT bridges.
- Plugin API for custom gauges, math channels, and analysis tools.
- Scripting for automated tuning workflows.
- Cloud-optional tune/log sharing (privacy-first, never required).
- **Mobile app (Android/iOS) — part of the OpenTune ecosystem.** Staged like the
  AI authority levels: **live parameter view first** (read-only realtime
  dashboard), **tune editing later**. Open architectural question for when it's
  scheduled: does mobile talk *directly* to the ECU (reusing the decoupled Rust
  core crates — `ini`/`model`/`protocol`/`transport`/`analysis` — compiled for
  mobile) or act as a *remote view* of a running desktop session? The decoupled
  core (see [ARCHITECTURE.md §5](ARCHITECTURE.md#5-backend-rust-modules)) is what
  keeps both options open.

---

## How milestones map to the architecture

| Milestone | Primary modules (see [ARCHITECTURE.md](ARCHITECTURE.md)) |
| --- | --- |
| M0 | app shell, IPC plumbing, CI |
| M1 | `transport`, `protocol`, `ini` (partial), `simulator` |
| M2 | `ini` (full), `model`, `protocol`, dialog engine |
| M3 | `realtime`, gauge dashboard |
| M4 | table editors, auto-tune (first `analysis` capability) |
| M5 | `analysis` (deterministic core), virtual dyno, `datalog`, log viewer |
| M6 | `project` (.msq), packaging/signing, i18n |
| M7 | tool registry, permission policy, provider abstraction, embedded assistant, MCP server |
