# 0003 — React + TypeScript + Vite for the frontend

- **Status:** Accepted
- **Date:** 2026-06-21

## Context

With Tauri chosen ([ADR-0001](0001-tauri-stack.md)), the frontend is a web
application in a native WebView. We need a stack that is **easy to contribute to**
(large talent pool), **type-safe** across the IPC boundary, and **performant** for
real-time gauges, large tables, and big datalog charts.

## Decision

Use **React + TypeScript**, built with **Vite**. Complementary choices:

- **State:** **Zustand** — minimal, fast, low-ceremony global state.
- **Time-series charts:** **uPlot** — extremely fast canvas charting for large
  datalogs.
- **3D tables:** **three.js** via a thin React wrapper — GPU-accelerated surfaces.
- **2D grids & gauges:** **custom HTML Canvas** rendering for high-frequency
  redraws, kept off the React reconciliation path.
- **IPC types:** **generated from Rust** (e.g., `ts-rs`/`specta`) so the contract
  can't silently drift.

## Consequences

**Positive**

- **Largest contributor pool** of any frontend stack — important for an open-source
  project that wants community momentum.
- **Type safety end-to-end** when IPC types are generated from the Rust source of
  truth.
- **Performance where needed**: heavy/real-time rendering uses canvas/WebGL, not
  the DOM, so frame rates stay high regardless of React.
- **Great DX**: Vite HMR makes iteration fast.

**Negative / costs**

- **React re-render pitfalls** if real-time data is naively pushed through React
  state. Mitigated by rendering hot paths on canvas and coalescing realtime events.
- **Ecosystem churn.** React tooling moves fast; we pin versions and keep
  dependencies lean.

## Alternatives considered

- **Svelte / SolidJS.** Excellent performance and ergonomics, smaller bundles, but
  smaller contributor pools than React. Strong runners-up; rejected primarily on
  community size for an open-source project. (The architecture doesn't hard-depend
  on React, so this could be revisited.)
- **Vue.** Large community and pleasant DX, but React's ecosystem and TS story edge
  it out for this use case. Rejected narrowly.
- **No framework (vanilla + canvas).** Maximum control/performance, but far slower
  to build complex UI (dialog engine, layouts) and harder to onboard contributors.
  Rejected.

## Notes

Because real-time and heavy rendering live on canvas/WebGL, the choice of UI
framework is deliberately **non-load-bearing** for performance — it's chosen for
developer experience and community, and could be revisited without touching the
backend or rendering layers.
