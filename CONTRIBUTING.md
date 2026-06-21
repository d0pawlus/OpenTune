# Contributing to OpenTune

Thanks for your interest! OpenTune is an open-source effort to build a modern,
universal tuning application for engine management ECUs. This guide explains how
the project is organized and how to get involved.

> **Status:** pre-alpha / design phase. There is **no application code yet** — the
> repository currently holds the architecture and design docs. The first code
> milestone (M0) is the Tauri skeleton; see [docs/ROADMAP.md](docs/ROADMAP.md).

## Start here

1. Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — the system design.
2. Skim the [ADRs](docs/adr/) — the *why* behind key decisions.
3. Check [docs/ROADMAP.md](docs/ROADMAP.md) — what we're building, in order.
4. New to engine tuning? See the [glossary](docs/glossary.md).

## How you can help right now (design phase)

- **Review the architecture and ADRs.** Challenge assumptions, point out risks,
  suggest alternatives — open an issue or a PR against the docs.
- **Share real INI/.msq/.mlg files** (for firmwares you own) to become test
  fixtures. This is hugely valuable for building a robust parser.
- **Document protocol/INI specifics** for ECUs you know well.
- **Help shape the roadmap** — what workflows matter most to you as a tuner?

## Planned tech stack (so you know what's coming)

- **App shell:** Tauri v2 ([ADR-0001](docs/adr/0001-tauri-stack.md))
- **Backend:** Rust (Cargo workspace of focused crates), `serialport`, Tokio
- **Frontend:** React + TypeScript + Vite, Zustand, uPlot, three.js
  ([ADR-0003](docs/adr/0003-frontend-stack.md))
- **Tooling:** rustfmt + clippy, eslint + prettier, GitHub Actions CI

Once M0 lands, this section will include exact setup steps (`pnpm`, Rust toolchain,
`cargo tauri dev`, etc.).

## Working with hardware (or without)

You will **not** need a physical ECU for most work: a first-class **ECU simulator**
([ADR-0004](docs/adr/0004-ecu-simulator.md)) lets you run the full app and tests
without hardware. If you *do* have a Speeduino, MegaSquirt, or rusEFI board,
hardware testing and bug reports against real firmware are especially welcome.

## Decision records (ADRs)

Significant, hard-to-reverse decisions are recorded as ADRs in
[docs/adr/](docs/adr/). If you're proposing something architectural, include (or
update) an ADR. See [docs/adr/README.md](docs/adr/README.md) for the format.

## Pull requests

- Keep PRs focused and explain the *why*, linking related issues/ADRs.
- For docs, prioritize **accuracy** — when describing INI/protocol behavior, defer
  to real firmware and note uncertainty rather than guessing.
- Be respectful and constructive. We want a welcoming community.

## Code of conduct

Be kind, assume good faith, and help others learn. A formal Code of Conduct will be
added before the project opens to wider contribution.

## Questions

Open an issue. During the design phase, discussion and review of the docs are the
most useful contributions you can make.
