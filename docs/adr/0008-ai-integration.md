# 0008 — AI integration on a deterministic core

- **Status:** Accepted
- **Date:** 2026-06-21

## Context

AI assistance is OpenTune's chosen differentiator: models that analyze live data and
logs and help tune the car, with the long-term goal of fully autonomous tuning.
This must coexist with the project's existing principles — offline-first,
privacy-by-default ([ARCHITECTURE.md §12](../ARCHITECTURE.md#12-cross-cutting-concerns))
— and with the hard reality that tuning writes to a **running engine**, where a
wrong value can cause damage.

Research also showed that TunerStudio's AutoTune (VE Analyze) is criticized for
being **non-deterministic** — it can produce a different table each run and
occasionally tune the wrong way (see
[market-and-user-research.md](../research/market-and-user-research.md) §2). Any AI
feature that inherits that non-determinism would repeat the incumbent's worst flaw.

The full design is in
[AI tuning & analysis design](../superpowers/specs/2026-06-21-ai-tuning-and-analysis-design.md);
this ADR records the load-bearing decisions.

## Decision

**Build AI as a thin orchestration layer over a thick deterministic core.**

1. **Deterministic core first.** All analytical/tuning logic lives in a pure,
   deterministic `analysis` crate (`ve_analyze`, `virtual_dyno`, `log_stats`,
   `detect_anomaly`). Same input → identical output, with an auditable justification
   on every result. It is built and useful *before* and *independent of* any AI.
2. **Thin AI.** The LLM orchestrates and explains; it never computes tuning numbers
   itself. All numbers come from the deterministic tools (Approach A, evolving
   toward a richer LLM *review* role — Approach C — without surrendering numeric
   determinism).
3. **Authority is configuration, not code.** A permission policy with levels
   `advisory` (ship default) → `assisted` → `autonomous`. The engine is identical at
   each level; only which mutating tools are unlocked changes. This realizes the
   long-term autonomy goal without re-architecting.
4. **Guardrails in the tool layer, not the prompt.** Mutating tools validate every
   change against INI `low`/`high` limits, rate-limit change magnitude, require a
   healthy connection, and audit every action. The LLM cannot bypass them because it
   has no other path to the ECU.
5. **BYOK cloud, opt-in.** An `AiProvider` trait; cloud providers via the user's own
   key, off by default. Enabling AI is the explicit consent step for data leaving
   the machine — preserving privacy-by-default. Local models are addable later
   behind the same trait (not built now — YAGNI).
6. **Two channels, one engine.** A shared tool registry backs both the embedded
   assistant (in-app, live tuning) and an MCP server (external agents, autonomy
   path).

## Consequences

**Positive**

- **Determinism and auditability** fix the incumbent's worst AutoTune flaw and make
  every AI-driven action reproducible and explainable.
- **One implementation** of each tuning operation serves AutoTune, the AI assistant,
  and future autonomy — no divergent logic.
- **Safety is structural**, not prompt-dependent: guardrails sit on the only path to
  the ECU.
- **Privacy preserved**: AI is opt-in; nothing leaves the machine by default.
- **A clean autonomy runway**: unlocking `assisted`/`autonomous` is a policy change
  on an unchanged engine.

**Negative / costs**

- Building the deterministic core first defers the visible AI feature to a later
  milestone (M7) — accepted; base-first is deliberate.
- The thin-AI rule constrains the assistant to what the tools expose; new analyses
  require new deterministic tools, not just prompt changes. This is the point.
- Maintaining a provider abstraction and an MCP server is extra surface area.

## Alternatives considered

- **Thick AI** (feed raw logs/tables to the LLM, let it compute corrections).
  Flexible but non-deterministic — exactly the TunerStudio complaint — plus
  expensive and prone to numeric hallucination, and unsafe to ever make autonomous.
  Rejected.
- **Embedded assistant only** or **MCP server only.** Each forecloses a real use
  case (live in-app tuning vs. external/autonomous agents). The shared registry
  makes "both" cheap. Rejected in favor of both.
- **Cloud-only with app-managed keys / always-on.** Violates privacy-by-default and
  offline-first. Rejected for BYOK + opt-in.
- **Ship `assisted`/`autonomous` now.** Unacceptable live-engine and liability risk
  in an unproven area. Rejected; design for them, ship `advisory`.

## Notes

Depends on [ADR-0004](0004-ecu-simulator.md) (the simulator's backing-memory burn
path is how the mutating/guardrail tools are tested without hardware) and the
generated-types decision in [ADR-0003](0003-frontend-stack.md) (tool/command schemas
should not be hand-duplicated). Revisit when `assisted`/`autonomous` levels or local
models are scheduled.
