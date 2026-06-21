# 0002 — Data-driven, universal core based on firmware INI

- **Status:** Accepted
- **Date:** 2026-06-21

## Context

OpenTune's goal is to support **many ECUs at once** — Speeduino, MegaSquirt,
rusEFI, and others — without a combinatorial explosion of per-ECU code. These
platforms already share a common asset: a firmware-supplied **INI definition file**
(the TunerStudio format) that describes each ECU's memory layout, real-time
channels, UI, and communication settings.

We must decide whether to (a) hand-code support per ECU, or (b) build a generic
engine driven by the INI.

## Decision

Make the application core **generic and data-driven from the INI**. The core knows
nothing about any specific ECU; it loads an INI, builds an in-memory `Definition`
(pages/constants, output channels, dialogs/menus, tables/curves/gauges, comms
settings), and drives everything — UI, memory access, and protocol — from that.

ECU-specific behavior that genuinely cannot be expressed in INI is isolated behind
small, **signature-keyed extension points** in the protocol layer, and nowhere
else.

## Consequences

**Positive**

- **Universality for free.** Supporting a new ECU (or a new firmware version)
  usually means using its INI — no code change.
- **One codebase to maintain and test.** Bug fixes and features benefit every ECU.
- **Interoperability.** We consume the same definitions the ecosystem already
  produces, keeping us aligned with `.msq`/`.mlg` formats too.
- **A clean architecture.** Forces a strong separation between generic engine and
  firmware-specific data.

**Negative / costs**

- **The INI parser is critical infrastructure.** Its correctness and coverage
  gate everything. The format is large, evolving, and unevenly documented.
  Mitigated by golden-file tests against real INIs and graceful degradation on
  unknown constructs.
- **Some firmware quirks resist data-driving.** A small amount of conditional code
  may be unavoidable; we contain it behind signature-keyed extension points and
  document each one.
- **An expression evaluator is required** (for INI `visible`/`enable`/scaling
  expressions), which must be safe and correct.

## Alternatives considered

- **Per-ECU hand-coded support.** Maximum control per ECU, but unscalable,
  duplicative, and slow to add ECUs/versions — the opposite of the project's goal.
  Rejected.
- **Support only one ECU initially.** Simpler short-term, but bakes in assumptions
  that make later generalization painful, and underdelivers on the vision.
  Rejected in favor of building generic from day one (the user's chosen direction).

## Notes

This is the project's defining decision and shapes the module boundaries in
[ARCHITECTURE.md](../ARCHITECTURE.md). See also [ini-format.md](../ini-format.md)
and [protocol.md](../protocol.md).
