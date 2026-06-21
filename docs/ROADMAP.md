# Roadmap

This roadmap turns the [architecture](ARCHITECTURE.md) into an ordered sequence of
milestones. Each milestone is meant to be **independently demonstrable** and to
build on the previous one. Dates are intentionally omitted — this is an
open-source project; milestones ship when they're ready.

Legend: ⬜ not started · 🟦 in progress · ✅ done

---

## M0 — Foundations ⬜

Goal: a buildable, runnable, empty-but-real Tauri app and the dev infrastructure.

- ⬜ Tauri v2 app skeleton (`src/` + `src-tauri/`) that launches on macOS/Win/Linux.
- ⬜ Rust workspace with empty `ini`, `model`, `protocol`, `transport`,
  `realtime`, `datalog`, `project`, `simulator` crates.
- ⬜ Frontend skeleton (React + TS + Vite), routing, theming, i18n scaffolding.
- ⬜ Typed IPC plumbing (one demo command + one demo event) with generated TS types.
- ⬜ CI: build + test on all three OSes; lint/format (clippy, rustfmt, eslint,
  prettier).
- ⬜ `CONTRIBUTING.md` dev setup verified end-to-end.

**Demo:** the app opens to an empty shell on all platforms; CI is green.

## M1 — Connect & identify ⬜

Goal: parse an INI, connect to a (simulated) ECU, and confirm identity.

- ⬜ `ini` crate: parse enough of a real INI to extract comms settings + signature.
- ⬜ `transport`: serial port enumeration + open/close; `SimTransport`.
- ⬜ `protocol`: signature/version query; generic MS/TS handshake.
- ⬜ `simulator`: minimal virtual ECU (answers signature/version).
- ⬜ UI: pick port + INI, Connect/Disconnect, show signature & connection state.

**Demo:** connect to the simulator (and, for testers, a real Speeduino) and see
its signature.

## M2 — Read, edit & burn the tune ⬜

Goal: full configuration editing — the core of a tuning tool.

- ⬜ `ini`: full constants/pages parsing; expression evaluator; dialogs/menus.
- ⬜ `model`: build `Tune` from pages; typed scaled accessors; dirty/undo-redo;
  RAM-vs-flash state.
- ⬜ `protocol`: page read/write, page activation, burn, CRC variants.
- ⬜ `simulator`: backing memory image for page read/write/burn.
- ⬜ Frontend **data-driven dialog engine**: render menus/dialogs/fields with
  conditional visible/enable; edit values → live write.

**Demo:** open a tune, change settings through auto-generated dialogs, write live,
burn to flash, undo/redo.

## M3 — Real-time dashboard ⬜

Goal: live gauges — the other half of day-to-day tuning.

- ⬜ `realtime`: polling loop, output-channel decoding, throttled events.
- ⬜ `simulator`: animated, correlated realtime channels.
- ⬜ Frontend **gauge dashboard**: canvas gauges, bindable to channels, editable
  layout saved with the project.

**Demo:** a live, configurable dashboard driven by the simulator (and real ECUs).

## M4 — Table editors & auto-tune ⬜

Goal: edit VE/ignition/AFR tables and improve them from data.

- ⬜ 2D heatmap table editor (interpolate, smooth, scale, copy/paste, keyboard).
- ⬜ 3D surface view (three.js) with live operating-point overlay.
- ⬜ Curve editors (1D).
- ⬜ First auto-tune (VE analyze) producing suggested table changes from logs.

**Demo:** tune a VE table in 2D/3D and apply a data-driven correction.

## M5 — Datalogging & analysis ⬜

Goal: record, replay, and analyze.

- ⬜ `datalog`: CSV writer first, then MLG read/write.
- ⬜ Frontend datalog viewer (uPlot time-series + scatter), playback synced to
  the dashboard.
- ⬜ Analysis tooling (markers, math channels, export).

**Demo:** record a session, replay it, and analyze it in-app.

## M6 — Interop, polish & first release ⬜

Goal: a real 1.0 people can use instead of TunerStudio for common workflows.

- ⬜ `.msq` import/export verified against TunerStudio.
- ⬜ Validate against multiple firmwares (Speeduino, rusEFI, an MS family member).
- ⬜ Signed, notarized macOS builds; signed Windows builds; Linux AppImage/deb.
- ⬜ Auto-update; first-run/onboarding; documentation site.
- ⬜ Accessibility & i18n pass (Polish + English).

**Demo:** download a signed build, open an existing tune/log, and work end-to-end.

## Beyond 1.0 (candidate ideas)

- CAN bus transport; Wi-Fi/BT bridges.
- Plugin API for custom gauges, math channels, and analysis tools.
- Scripting for automated tuning workflows.
- Cloud-optional tune/log sharing (privacy-first, never required).
- Mobile companion (the architecture should not preclude it).

---

## How milestones map to the architecture

| Milestone | Primary modules (see [ARCHITECTURE.md](ARCHITECTURE.md)) |
| --- | --- |
| M0 | app shell, IPC plumbing, CI |
| M1 | `transport`, `protocol`, `ini` (partial), `simulator` |
| M2 | `ini` (full), `model`, `protocol`, dialog engine |
| M3 | `realtime`, gauge dashboard |
| M4 | table editors, auto-tune |
| M5 | `datalog`, log viewer |
| M6 | `project` (.msq), packaging/signing, i18n |
