# 0007 — Positioning relative to LibreTune

- **Status:** Proposed
- **Date:** 2026-06-21

## Context

Market research ([market-and-user-research.md](../research/market-and-user-research.md))
found [**LibreTune**](https://github.com/RallyPat/LibreTune): a Rust + Tauri +
React/TypeScript desktop ECU tuner — nearly the same stack and thesis as OpenTune.
It parses `.ini`/`.msq`/`.mlg`, does live serial tuning (gauges, table editing,
burn-to-ECU, AutoTune), and supports Speeduino/rusEFI/FOME/epicEFI + MS2/MS3.
Created Jan 2026, active Jun 2026. This is the project's single biggest "this
already exists" risk, so we deliberately investigated before committing to a
direction.

### What the code review found (verified against the repo)

**Genuine strengths — real, working domain code:**

- An INI parser and a generic, data-driven core that are cleanly separated from the
  Tauri app (core library scores well on architecture).
- A real protocol layer including page write and **burn** — not stubs.
- A production-grade *realtime* demo simulator (correlated TPS/MAP/AFR, coolant
  warm-up, EGO, an RPM state machine) and a well-engineered realtime store
  (Zustand + `subscribeWithSelector` + an off-state circular buffer to avoid 20 Hz
  re-render storms).

**A thin trust/verification layer — the real gap:**

- **No public firmware/INI corpus** and **the flash path is untested end-to-end** —
  the demo simulator covers realtime but **not** page read/write/burn.
- **TS types are hand-duplicated** (`src-tauri/src/commands/types.rs` ↔
  `src/types/app.ts`), not generated — a standing drift/maintenance risk.
- **Sparse frontend tests** (~12 vitest files for ~191 TS files / ~40k LOC) and no
  E2E harness.
- **App layer is less disciplined than the core:** 71 commands over a single
  mutex-locked `AppState`, plus God-components (`App.tsx` ~1432 LOC,
  `TableEditor2D` ~1190, `SettingsDialog` ~1015).
- **Bus factor of 1** (solo + AI authorship), alpha, no stable release, **GPL-2.0
  only**.

### The decisive constraint

LibreTune is **GPL-2.0-only**, which is **incompatible with GPL-3** (OpenTune's
recommendation, [ADR-0005](0005-license.md)). Lifting LibreTune source into a GPL-3
project is therefore **not legally available** regardless of technical merit. That
removes "fork it" and "copy its code" from the table by default.

## Decision

**Differentiate: build OpenTune as an independent GPL-3 project, and compete on the
trust/verification layer LibreTune lacks — not on breadth of domain code.**

Concretely, OpenTune's defensible differentiators are:

1. **A reproducible firmware corpus** — golden-file tests against real Speeduino /
   rusEFI / MS INIs and `.msq`/`.mlg` fixtures, committed and run in CI.
2. **A flash-path that is actually tested** — the simulator must serve page
   read/write/**burn** against a backing memory image (already required by
   [ADR-0004](0004-ecu-simulator.md)), so write-to-ECU is verified end-to-end
   without hardware. This is the single biggest trust gap in the incumbent *and* in
   LibreTune.
3. **Generated TS types from Rust** (ts-rs/specta) — no hand-duplicated contract.
4. **Real `.mlg` support** and the analysis features users beg for (two-log compare,
   GUI math channels) — see the roadmap.
5. **Multi-contributor momentum** — the failure mode that killed every prior OSS
   attempt (and threatens LibreTune) is bus-factor-1. Governance, docs, the
   simulator, and CI all exist to lower the contribution barrier.

**Re-derive, do not copy.** We may *study* LibreTune's protocol/INI design as one
reference among several (alongside the open firmware source and the MIT parsers in
[ADR-0006](0006-reuse-existing-parsers.md)), but implementations are written fresh
from openly-licensed sources. Any future reuse of LibreTune code is blocked unless
its license changes or OpenTune's license choice is revisited.

## Consequences

**Positive**

- A clear, honest reason to exist: not "another tuner" but *the trustworthy,
  verifiable, multi-maintainer* one. The differentiators map onto verified user
  pain (untested writes, no diff/merge, no two-log compare) and onto LibreTune's
  specific weaknesses.
- No legal entanglement; OpenTune keeps full control of its license and direction.
- The work that proves trustworthiness (corpus, burn tests, generated types) is
  exactly the work that also makes the project contributable — they reinforce.

**Negative / costs**

- We forgo a head start: LibreTune has working domain code we cannot lift, so we
  re-derive the INI/protocol layers (mitigated by [ADR-0006](0006-reuse-existing-parsers.md)
  — porting MIT-licensed references, not starting blank).
- Two similar OSS projects may split an already small contributor pool. Mitigation:
  win on reliability/verification and interoperate via shared open formats rather
  than competing on identical breadth.
- LibreTune may mature or relicense; this ADR should be revisited if it does.

## Alternatives considered

- **Contribute to LibreTune instead.** Tempting (it solves the bus-factor problem
  for *that* project), but it means adopting GPL-2.0-only, a hand-duplicated type
  contract, and an app layer with a single shared-state mutex and God-components —
  and ceding control of the license and direction OpenTune was created to set.
  Rejected, but the door stays open to upstreaming format/parser fixes where
  licensing permits.
- **Compete head-on on the same breadth.** Maximum duplicated effort, weakest
  story. Rejected — racing to re-implement the same features is how you lose to the
  incumbent, not to LibreTune.
- **Abandon OpenTune as redundant.** Rejected: LibreTune is alpha, bus-factor-1, and
  unverified on the flash path; the niche (a *trustworthy, multi-maintainer* native
  OSS tuner) is demonstrably still open.

## Notes

Depends on [ADR-0005](0005-license.md) (the GPL-3 choice is what makes reuse
infeasible) and [ADR-0006](0006-reuse-existing-parsers.md) (reuse openly-licensed
references). If OpenTune's owner picks a GPL-2-compatible license, or LibreTune
relicenses, the reuse calculus changes and this ADR should be revised.
