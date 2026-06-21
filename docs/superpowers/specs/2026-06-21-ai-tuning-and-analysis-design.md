# Design: AI-assisted tuning & analysis (with a deterministic core)

- **Date:** 2026-06-21
- **Status:** Approved (design); pending implementation plan
- **Related:** [ARCHITECTURE.md](../../ARCHITECTURE.md),
  [ROADMAP.md](../../ROADMAP.md),
  [market-and-user-research.md](../../research/market-and-user-research.md)

## 1. Purpose

Add four capabilities to OpenTune and define how they fit together:

1. **Built-in log viewer** — already planned (M5); here it also becomes a data
   source for analysis.
2. **Virtual dyno** — estimate WHP/torque curves from a datalog.
3. **AutoTune** — TunerStudio-style data-driven table correction (VE analyze).
4. **AI agent integration** — the differentiator: AI models that analyze live data
   and logs and help tune the car.

Plus one explicitly-future direction:

5. **Sensor/component database** — pick a sensor/injector/MAP model and have its
   calibration parameters filled in, instead of hunting for them.

## 2. Guiding decisions (from brainstorming)

- **Deterministic base first, AI second.** Every analytical/tuning operation is a
  pure, deterministic Rust function. The AI is an *orchestration layer on top* —
  valuable features exist and work even if AI is never enabled.
- **AI authority is a configurable policy, not a hardcoded limit.** Ship at the
  `advisory` level; the same engine later unlocks `assisted` and `autonomous`
  levels. The ultimate goal is fully autonomous tuning; users get the choice.
- **BYOK cloud, opt-in.** Users supply their own API key (Claude/OpenAI/…). AI is
  off by default — no data leaves the machine until the user opts in. This
  preserves the project's offline-first / privacy-by-default stance
  ([ARCHITECTURE.md §12](../../ARCHITECTURE.md#12-cross-cutting-concerns)). A
  provider abstraction leaves room for local models later (not built now — YAGNI).
- **Embedded assistant *and* MCP server, on one shared tool engine.** One tool
  definition, two access channels.
- **Thin AI, thick deterministic tools (Approach A → C).** The LLM orchestrates and
  explains; all numeric tuning logic lives in deterministic tools. Evolution toward
  a richer LLM review role (Approach C) is expected, but the numbers stay
  deterministic. This directly fixes the verified pain point that TunerStudio's VE
  Analyze is non-deterministic and can tune the wrong way
  (research doc §2, pain point #6).

## 3. Architecture

### 3.1 The deterministic core — `analysis` crate

A new Rust crate (working name `analysis`), independent of AI and UI. Every
capability is a pure function with an explicit contract:
`(input data + parameters) → result + justification`, no side effects.

Initial capabilities:

- `ve_analyze(log, current_table, params) -> ProposedTableChange`
  — `{ per_cell_delta, confidence, sample_count, filtered_reason }`
- `virtual_dyno(log, vehicle_params) -> DynoRun`
  — `{ whp_curve, torque_curve, conditions, assumptions }`
- `log_stats(log, channels, filters) -> ChannelStats`
- `detect_anomaly(log) -> Vec<Finding>`
  — `{ channel, where, severity }` (lean spike, knock, sensor dropout, …)

Required properties:

- **Determinism.** Same input → same output. No RNG, explicit sort orders, explicit
  thresholds.
- **Auditability.** Every result carries *why* — which log samples drove it, how
  many, what was filtered out. Surfaced in both the UI and to the AI.
- **One engine, many consumers.** The same `ve_analyze` powers the manual AutoTune
  button, the AI assistant, and future autonomy. There is exactly one
  implementation of each tuning operation.

```
            ┌─────────────────────────────────────────┐
            │  analysis crate (deterministic core)      │
            │  ve_analyze · virtual_dyno · log_stats ·  │
            │  detect_anomaly · ...                     │
            └─────────────────────────────────────────┘
                 ▲              ▲              ▲
                 │              │              │
          AutoTune (UI)   AI assistant    Autonomy (future)
```

### 3.2 The AI layer

The AI never touches the ECU or logs directly — only through the deterministic
tools, wrapped in an agent-capability layer. Three elements:

**1. Tool registry (shared engine).** Each `analysis` capability is exposed as a
tool with a description and JSON schema. Two kinds:

- *read-only* (analysis): `ve_analyze`, `virtual_dyno`, `get_log_stats`,
  `detect_anomaly`, `read_tune`, `read_realtime` — called freely.
- *mutating* (write): `propose_change`, `apply_change`, `burn` — gated by the
  permission policy.

The same registry feeds the embedded assistant and the MCP server.

**2. Permission policy (authority as configuration).**

| Level | What the AI may do | Status |
|-------|--------------------|--------|
| `advisory` | read-only tools + `propose_change` (proposes, never writes) | **MVP — default** |
| `assisted` | `apply_change` after explicit per-change user confirmation | future |
| `autonomous` | apply→observe→correct loop within guardrails | future, behind hard safeguards |

The engine is identical at every level; only which mutating tools are unlocked
changes.

**3. Provider abstraction (BYOK, opt-in).** An `AiProvider` trait (cloud:
Claude/OpenAI/… via the user's key; local: addable later). Disabled by default;
enabling it (entering a key + turning AI on) is the consent step for any data
leaving the machine.

**Guardrails (critical — live engine).** Mutating tools validate every change
against the INI's `low`/`high` limits (clamp or reject out-of-range), rate-limit the
magnitude of a single change, require a healthy connection, and write every AI
action to an audit trail (who/what/when/why). **These live in the tool layer, not
in the prompt** — the LLM cannot bypass them because it has no other path to the
ECU.

```
  Embedded assistant ─┐                ┌─ AiProvider (BYOK cloud, opt-in)
                      ├─ Tool registry ─┤
  MCP server ─────────┘   + permission  └─ analysis crate (§3.1) → model/protocol
                          policy
                          + guardrails
```

### 3.3 Access channels

- **Embedded assistant.** A chat/assistant panel in OpenTune. The app calls the
  provider API; the model is given the tool registry. Best for live in-car tuning;
  coherent UX; full control over guardrails.
- **MCP server.** OpenTune exposes itself as an MCP server so external agents
  (Claude Desktop, Claude Code, …) can connect. Maximum flexibility for power users
  and the future autonomy path. Same tool registry, same guardrails.

## 4. Supporting features & their place

- **Log viewer (M5, already planned):** unchanged in plan; additionally it is the
  shared data source for `analysis` — the same logs the user views are what the
  AI/AutoTune analyze.
- **Virtual dyno (new, M5):** `analysis::virtual_dyno`; consumers are a UI dyno view
  and an AI tool. Lives in M5 because it needs logs.
- **AutoTune (M4, already planned):** the first consumer of `analysis::ve_analyze`,
  deterministic and auditable. No AI involved.
- **AI assistant + MCP server (new, M7):** a new milestone *after* the analysis core
  exists, per the "base first" decision. Ships at `advisory`.
- **Sensor/component database (Beyond 1.0):** a curated DB mapping a
  sensor/injector/MAP model to ready calibration parameters (MAP transfer curve,
  injector dead time, IAT/CLT calibration). Plugs into the data-driven core: instead
  of typing calibration by hand, the user picks a model and the values populate the
  matching INI constants. Deferred because it requires building the database.

## 5. Roadmap placement

| Milestone | What is added |
|-----------|---------------|
| M4 | AutoTune as a consumer of deterministic `analysis::ve_analyze` |
| M5 | + **`analysis` crate**, virtual dyno, log analysis tools |
| M6 | unchanged (interop, first release) |
| **M7 (new)** | **Tool registry + permission policy + provider abstraction + embedded assistant + MCP server** (level `advisory`) |
| Beyond 1.0 | `assisted`/`autonomous` levels, sensor database, local models |

## 6. Risks & mitigations

- **Live-engine safety (high).** A wrong write can damage an engine. Mitigation:
  guardrails in the tool layer (INI-limit validation, rate limits, connection
  health, audit trail); `advisory` default means no AI writes at all in the MVP.
- **AI authority creep / liability (high, future).** `assisted`/`autonomous` carry
  real risk. Mitigation: keep them behind explicit opt-in and hard safeguards;
  design now, ship later; the deterministic tools make every action auditable.
- **Privacy (medium).** Cloud AI sends vehicle data off-machine. Mitigation:
  off-by-default, BYOK, explicit opt-in; provider abstraction allows local models
  later.
- **Non-determinism leaking in (medium).** If numeric logic drifts into the LLM,
  reproducibility is lost. Mitigation: thin-AI rule — numbers come only from
  deterministic tools; LLM orchestrates and explains.
- **Scope (medium).** Four features at once. Mitigation: the deterministic core is
  shared and built first; AI is a separate later milestone.

## 7. Testing

- **`analysis` crate:** unit tests asserting determinism (same input → identical
  output) and golden-file tests for `ve_analyze`/`virtual_dyno` against real logs.
- **Tool layer:** tests that mutating tools reject out-of-range / oversized changes
  and that the audit trail records every action — against the simulator's
  backing-memory burn path ([ADR-0004](../../adr/0004-ecu-simulator.md)).
- **Permission policy:** tests that `advisory` exposes no working `apply_change`.
- **Provider abstraction:** a fake provider for deterministic AI-layer tests with no
  network.

## 8. Open items for the implementation plan

- Final crate name (`analysis` vs `tuning`) and exact module boundaries.
- Concrete tool schemas and the assistant's system-prompt scope.
- MCP server transport/registration details (which channels, auth).
- Virtual dyno's physics model and required vehicle parameters.
