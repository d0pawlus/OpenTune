# 0006 — Port/reuse existing parsers rather than re-deriving them

- **Status:** Proposed
- **Date:** 2026-06-21

## Context

The architecture identifies the **INI parser as critical-path infrastructure**
(see [ADR-0002](0002-data-driven-ini.md), [ARCHITECTURE.md §5.3](../ARCHITECTURE.md#53-ini--firmware-definitions)):
its correctness and coverage gate everything. The TunerStudio INI "dictionary" is
large (~111-page spec) and unevenly documented, and the `.mlg` binary log format
has subtle versioning.

Market research (see [market-and-user-research.md](../research/market-and-user-research.md))
surfaced that this work has **already been done, openly, in multiple languages**:

- [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini) — TS `.ini` parser (MIT, JS).
- [`adbancroft/TunerStudioIniParser`](https://github.com/adbancroft/TunerStudioIniParser) — `.ini` parser (Python).
- [`karniv00l/mlg-converter`](https://github.com/karniv00l/mlg-converter) — `.mlg` reader (JS).
- [`hyper-tuner/mlg-cli`](https://github.com/hyper-tuner/mlg-cli) — `.mlg` (Rust, abandoned at v0.1.0).
- [`askrejans/speeduino-serial-sim`](https://github.com/askrejans/speeduino-serial-sim) — full protocol **simulator**.
- Speeduino `comms.cpp` / rusEFI `tunerstudio.cpp` + `ts_protocol.txt` — the
  de-facto protocol truth.

The original plan implied writing these from scratch in Rust. Re-deriving a
111-page spec by hand is exactly the kind of effort that has sunk prior projects
before they reached feature parity.

## Decision

**Default to porting from a proven reference, not re-deriving from the spec.** For
each format/protocol surface, before writing new code:

1. Identify the most authoritative open implementation(s) above.
2. Port its structure/behavior into the relevant Rust crate (`ini`, `datalog`,
   `protocol`, `simulator`), keeping our module boundaries and types.
3. Capture the **real input files** those projects test against as golden-file
   fixtures (see [ARCHITECTURE.md §13](../ARCHITECTURE.md#13-testing-strategy)).
4. Treat the open firmware source (Speeduino/rusEFI) — **not** the proprietary EFI
   Analytics PDFs — as the citable source of truth for protocol behavior.

This is a default, not a mandate: where a reference is low-quality, abandoned at an
early version, or licensed incompatibly, writing fresh is fine — but that should be
a deliberate, recorded choice.

## Consequences

**Positive**

- Avoids the most common failure mode (burnout re-deriving the format) and reaches
  a working parser far sooner.
- Inherits years of accumulated edge-case handling and real test corpora.
- Frees effort for the actual differentiators (reliability, diff/merge, auto-tune).

**Negative / costs**

- **License compatibility must be checked per source.** MIT (`hyper-tuner/ini`,
  `mlg-converter`) ports cleanly into a GPL project. A **GPL-2-only** source (e.g.
  if reused from LibreTune) is **incompatible with GPL-3** — see [ADR-0005](0005-license.md).
  Ports must record the source license and the basis for compatibility.
- A port is a derivative work: attribution and license headers are required.
- Porting JS/Python idioms to idiomatic Rust still takes real work — this lowers
  risk, not effort to zero.

## Alternatives considered

- **Write everything from scratch in Rust.** Maximum control and idiomatic code,
  but slow, duplicative, and the highest-risk path for the critical component.
  Rejected as the default.
- **Depend on an external crate at runtime.** No mature, maintained Rust crate
  covers the full INI dialect; the closest (`mlg-cli`) is abandoned at v0.1.0.
  Porting into our own crates gives us control over correctness and coverage.

## Notes

Pairs with the (pending) ADR on positioning relative to LibreTune, which may itself
be a reuse source — subject to the GPL-2/GPL-3 caveat above.

### Note (2026-07-02) — write-fresh exception: `ini::expr` evaluator

Task 2 (`ini` crate expression evaluator, `src-tauri/crates/ini/src/expr.rs`)
is a **deliberate, recorded exception** to the port-first default above.

The only working reference implementation for the sandboxed
arithmetic/boolean grammar used in `visible`/`enable` conditions and
constant-scaling expressions is rusEFI's `ExpressionEvaluator.java`. That
source is licensed GPLv3 **plus additional §7 field-of-use terms**
(restricting use in aircraft and off-road applications) that this project
does not carry anywhere else in the tree. Porting it — even faithfully —
would import those additional terms into this crate. The other candidate
references (`hyper-tuner/ini`, `adbancroft/TunerStudioIniParser`) both treat
`{ … }` expressions as opaque strings and never evaluate them, so neither is
a usable port source either.

**Decision:** `ini::expr` is written fresh. rusEFI's evaluator was consulted
only as a *structural* reference — to confirm the real-world operator set
(arithmetic, comparisons, boolean ops, bare-symbol variables, and the
unsupported `bitStringValue`/`table` function-call forms) enumerated from
real `speeduino.ini` files — not as a line-by-line port. No code or algorithm
was copied. See the module doc comment in `expr.rs` for the same rationale
recorded alongside the implementation.

### Note (2026-07-03) — write-fresh exception: `[GaugeConfigurations]` / `[FrontPage]` parser

Task 3 of M3 (`src-tauri/crates/ini/src/gauges_parser.rs`) is a second
**deliberate, recorded exception** to the port-first default.

`hyper-tuner/ini`'s section switch ends at `[Datalog]` (`src/ini.ts`,
~lines 146-190) — it parses neither `[GaugeConfigurations]` nor
`[FrontPage]`, so there is nothing to port. The grammar truth-source is
the real `speeduino.ini` (noisymime/speeduino @ 63fd68e9, GPL-3),
consulted as a **reference corpus only** (no parser code exists there to
copy); the exact lines exercised are preserved, trimmed, in the
`tests/fixtures/speeduino-gauges.ini` golden fixture. The same rationale
is recorded in the module doc of `gauges_parser.rs` and the test header
of `tests/gauges.rs`.

The bitwise `&`/`<<` extension to `ini::expr` (same task) inherits the
Task 2 write-fresh decision above — the operator set was again confirmed
against real INI usage (`{ sd_status & 1 }`,
`{ halfSync + (sync << 1) }`), with standard C precedence.
