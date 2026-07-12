# OpenTune

> **OpenTune** A modern, open-source, cross-platform tuning
> application for engine management ECUs. The goal is to be a first-class
> replacement for TunerStudio, with native support for Apple Silicon macOS,
> Windows, and Linux.

---

## Download (pre-release)

Unsigned test builds will be published on the
[Releases page](https://github.com/d0pawlus/OpenTune/releases) once the first
`v*` release tag is cut (planned as part of M6 — see
[ROADMAP — M6](docs/ROADMAP.md)). Until then, build from source with
`npm run tauri build` (see [CONTRIBUTING.md](CONTRIBUTING.md)).

When pre-release builds are available, expect OS warnings:

- **macOS** — the app is not notarized yet. Right-click the app → _Open_
  (once), or clear the quarantine flag: `xattr -cr /Applications/OpenTune.app`.
- **Windows** — SmartScreen will warn about an unknown publisher.
  _More info_ → _Run anyway_.
- **Linux** — download the `.AppImage`, then `chmod +x OpenTune_*.AppImage`
  and run it. A `.deb` is also provided for Debian/Ubuntu. Serial-port access
  may require adding your user to the `dialout` group
  (`sudo usermod -aG dialout $USER`, then re-login).

Signed and notarized builds are planned before 1.0 (see
[ROADMAP — M6](docs/ROADMAP.md)).

## Why this project exists

[TunerStudio](https://www.tunerstudio.com/) is the de-facto standard tuning
application for a large family of open and semi-open ECUs (Speeduino, MegaSquirt,
rusEFI, and many others). It is, however:

- **Aging and effectively closed.** It is a Java/Swing application whose future
  on modern macOS (Apple Silicon, tightened notarization/Gatekeeper rules) is
  uncertain, and it is not open source.
- **Not optimized for modern hardware.** Real-time gauges, large datalogs, and
  3D table rendering are heavier than they need to be.
- **Hard to extend by the community.** There is no open contribution model.

**OpenTune** aims to fix all three: a fast, modern, genuinely open-source tool
that the community can own and evolve.

## Vision & principles

1. **Universal by design.** Almost everything ECU-specific is _data-driven_ from
   the firmware's `.ini` definition file (the same format TunerStudio uses). The
   application core is generic — supporting a new ECU means supporting its INI,
   not writing new code. This is how we target Speeduino, MegaSquirt, rusEFI and
   "many others" _at once_.
2. **Fast and lean.** Tauri (Rust backend + web frontend) gives us native
   performance, tiny binaries, and a serial/real-time data path written in Rust.
3. **Easy to develop.** A documented, modular architecture, a built-in **ECU
   simulator** so contributors can work without physical hardware, and a
   mainstream frontend stack (React + TypeScript).
4. **Truly open source.** Open license, open roadmap, open contribution model.
5. **Interoperable.** Read and write the file formats people already have:
   `.msq` tunes, `.mlg`/CSV datalogs, and standard `.ini` definitions.
6. **AI-assisted, deterministically.** The differentiator: an optional AI assistant
   that analyzes live data and logs and helps tune — built on a **deterministic,
   auditable core** (the AI orchestrates and explains; the numbers come from
   reproducible tooling). Off by default, opt-in (BYOK), designed toward future
   autonomous tuning. See
   [the AI design](docs/superpowers/specs/2026-06-21-ai-tuning-and-analysis-design.md)
   and [ADR-0008](docs/adr/0008-ai-integration.md).

## Target platforms

| Platform                     | Status (planned)   |
| ---------------------------- | ------------------ |
| macOS (Apple Silicon, arm64) | First-class target |
| macOS (Intel, x64)           | Supported          |
| Windows 10/11 (x64)          | Supported          |
| Linux (x64, arm64)           | Supported          |

## Supported ECUs (goal)

Because support is driven by the firmware INI, the goal is to work with any ECU
that ships a TunerStudio-compatible `.ini` definition, including:

- **Speeduino**
- **rusEFI**
- **MegaSquirt** (MS1/MS2/MS3 family)
- and other MS-protocol-compatible controllers.

See [`docs/protocol.md`](docs/protocol.md) and
[`docs/ini-format.md`](docs/ini-format.md) for how this works.

## Project status

🚧 **Pre-alpha — active implementation.** Milestones M0–M5 are implemented:
the Tauri application can parse firmware INIs, identify the simulator or a
serial ECU, and run the end-to-end flow on the hardware-free simulator —
read/edit/burn, realtime dashboard, table/curve/3D editors, deterministic
auto-tune, and datalog capture with analysis. M6 (interop hardening, packaging,
signed/notarized builds, first public release) is in progress; see
[ROADMAP — M6](docs/ROADMAP.md#m6--interop-polish--first-release-).

The project is not ready for production tuning yet. Real-hardware coverage,
packaging/signing, and broader firmware compatibility remain pre-1.0 work; see
the roadmap for the current status.

Start here:

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — the system architecture.
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — milestones and what we build, in order.
- [`docs/research/market-and-user-research.md`](docs/research/market-and-user-research.md)
  — what TunerStudio users actually need, the competitive landscape, and the
  formats/protocol terrain (with sources).
- [`docs/adr/`](docs/adr/) — Architecture Decision Records (the _why_ behind key
  choices).
- [`docs/ini-format.md`](docs/ini-format.md) — the firmware definition format.
- [`docs/protocol.md`](docs/protocol.md) — the ECU communication protocol.
- [`docs/glossary.md`](docs/glossary.md) — domain terms for newcomers.
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — how to get involved.

## Planned high-level architecture

```mermaid
flowchart LR
    ECU["ECU\n(Speeduino / MS / rusEFI)"] <-->|USB serial / CAN / TCP| Transport
    subgraph Backend["Rust backend (src-tauri)"]
        Transport[Transport layer] --> Protocol[Protocol layer]
        INI[INI definition parser] --> Model[Tune & channel model]
        Protocol --> Model
        Model --> RT[Real-time engine]
        RT --> Logger[Datalogger]
        Model --> Store[(Project / tune files)]
    end
    subgraph Frontend["React + TS frontend (src)"]
        Dialogs[Data-driven dialog engine]
        Tables[2D/3D table editors]
        Dash[Gauge dashboard]
        LogView[Datalog viewer & analysis]
    end
    Backend <-->|Tauri commands + events| Frontend
```

## License

**GPL-3.0-or-later** — see [`LICENSE`](LICENSE) and
[`docs/adr/0005-license.md`](docs/adr/0005-license.md). This matches the ethos of
the open ECU ecosystem (Speeduino and rusEFI firmware are GPL) and protects the
work from a closed-source fork. Source files will carry
`SPDX-License-Identifier: GPL-3.0-or-later` headers in source files.

## Acknowledgements

This project stands on the shoulders of the open ECU community — Speeduino,
rusEFI, MegaSquirt/MShift, MegaTunix, and the documented TunerStudio INI format.
We aim to be a good citizen of that ecosystem and remain interoperable with it.
