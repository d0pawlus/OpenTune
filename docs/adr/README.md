# Architecture Decision Records (ADRs)

This directory captures the significant, long-lived decisions behind OpenTune —
the *why*, not just the *what*. The [architecture document](../ARCHITECTURE.md)
describes the system as it is; ADRs explain how we got there and what we ruled out.

We use a lightweight [MADR](https://adr.github.io/madr/)-style format.

## Format

Each ADR is a numbered file: `NNNN-short-title.md`, with sections:

- **Status** — Proposed / Accepted / Superseded (by #NNNN) / Deprecated.
- **Context** — the forces and constraints at play.
- **Decision** — what we chose.
- **Consequences** — the trade-offs, good and bad.
- **Alternatives considered** — what else we evaluated and why we passed.

## When to write one

Write an ADR when a decision is **hard to reverse** or **broadly affects** the
codebase: choosing a framework, a core data model, a cross-cutting pattern, a file
format, or a license. Routine changes don't need an ADR.

To supersede an old decision, write a new ADR and mark the old one
`Superseded by #NNNN`.

## Index

| # | Title | Status |
| --- | --- | --- |
| [0001](0001-tauri-stack.md) | Use Tauri (Rust + web) for the application shell | Accepted |
| [0002](0002-data-driven-ini.md) | Data-driven, universal core based on firmware INI | Accepted |
| [0003](0003-frontend-stack.md) | React + TypeScript + Vite for the frontend | Accepted |
| [0004](0004-ecu-simulator.md) | A first-class ECU simulator | Accepted |
| [0005](0005-license.md) | Open-source license (GPL-3.0-or-later) | Accepted |
| [0006](0006-reuse-existing-parsers.md) | Port/reuse existing parsers rather than re-deriving them | Proposed |
| [0007](0007-positioning-vs-libretune.md) | Positioning relative to LibreTune | Proposed |
| [0008](0008-ai-integration.md) | AI integration on a deterministic core | Accepted |
