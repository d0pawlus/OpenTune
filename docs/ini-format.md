---
layout: page
title: Firmware definition format
permalink: /ini-format/
---

# Firmware definition format (the `.ini` file)

This document explains the role of the firmware **INI definition file** in
OpenTune and outlines the parts of the format we need to support. It is a
*working reference for implementers*, not a formal specification — the format is
defined by the broader TunerStudio/MegaSquirt ecosystem, and our authority is the
real INIs shipped by Speeduino, rusEFI, and MegaSquirt.

> **Why this matters:** the INI is what makes OpenTune universal. Parsing it well
> is what lets a single, generic application support many ECUs. See
> [ADR-0002](adr/0002-data-driven-ini.md).

## What an INI file is

An INI definition is a structured text file, supplied by the firmware author, that
describes everything an application needs to talk to and present a given ECU:

1. **Identity** — a signature string the ECU reports, used to match firmware ↔ INI.
2. **Communication settings** — which serial commands and timeouts to use.
3. **Memory layout** — the configuration data, grouped into *pages* of *constants*.
4. **Output channels** — the real-time telemetry the ECU streams back.
5. **UI definition** — menus, dialogs, fields, tables, curves, and gauges.

OpenTune parses this into an immutable in-memory `Definition` (see the `ini` and
`model` crates in [ARCHITECTURE.md](ARCHITECTURE.md)).

## Sections we need to support

The exact set varies by firmware/version; below are the sections that matter, with
their purpose. The parser should handle unknown sections gracefully.

| Section | Purpose |
| --- | --- |
| `[MegaTune]` / `[TunerStudio]` | Versioning, signature, query command, global flags. |
| `[Constants]` | The heart of it: page count/sizes and every configuration constant (scalars, arrays, bit fields) with type, scaling, units, range, and digits. |
| `[PcVariables]` | App-side variables (not stored on the ECU) used in expressions/UI. |
| `[OutputChannels]` | Real-time channels streamed by the ECU, with offsets, types, and scaling. |
| `[Datalog]` | Which channels are logged and how they're labeled/formatted. |
| `[ConstantsExtensions]` | Extra behavior for constants (e.g., default values, special handling). |
| `[Menu]` | The application menu tree linking to dialogs. |
| `[UserDefined]` | Dialogs/panels and their field layout. |
| `[CurveEditor]` | 1D curve editors (e.g., warm-up enrichment vs. temp). |
| `[TableEditor]` | 2D/3D table editors (e.g., VE, ignition, AFR target). |
| `[GaugeConfigurations]` | Predefined gauges (range, warn/danger thresholds). |
| `[ControllerCommands]` | Named action commands (e.g., trigger an output test). |
| `[SettingGroups]` / `[Tools]` / `[WueAE]` etc. | Firmware-specific extras; supported as needed. |

## Constants: the memory model

A *constant* is a named value living at a known **page + offset** with a **type**
and a **scale/translate** transform. Conceptually:

```
name = type, layout, offset/shape, units, scale, translate, low, high, digits
```

Key aspects the parser/model must capture:

- **Types:** unsigned/signed 8/16/32-bit integers, fixed-point, bit fields
  (`bits`), arrays (1D/2D), and strings.
- **Endianness:** fields may be big- or little-endian depending on firmware.
- **Scaling:** `physical = raw * scale + translate`; the inverse is used on write.
- **Bit fields:** packed enums within a byte/word, with named options.
- **Limits & digits:** for validation and display precision in the UI.
- **Pages:** constants are grouped into pages that map to ECU memory regions and
  are read/written/burned as units (see [protocol.md](protocol.md)).

## Expressions

INI files use small expressions in several places:

- **Conditional UI:** `visible`/`enable` clauses (e.g., show a field only when a
  feature is enabled).
- **Computed/derived values** and dynamic scaling.
- **Preprocessing:** `#if/#elif/#else/#endif`, `#define`, and `#set` to tailor the
  definition to firmware build options.

OpenTune includes a **small, sandboxed expression evaluator** (no I/O, no
arbitrary code) supporting arithmetic, comparisons, boolean logic, and references
to constants/PC variables. This is the only "execution" the INI gets.

## UI definition

Menus reference dialogs; dialogs lay out fields, sub-panels, and embedded
tables/curves. The frontend **dialog engine** renders this generically (see
[ARCHITECTURE.md §6.1](ARCHITECTURE.md#61-data-driven-dialog-engine)). Table and
curve editors reference the constants that hold their axes and Z-data.

## Parsing strategy

1. **Preprocess** (`#if`/`#define`/`#set`, includes) into a flat token stream.
2. **Tokenize & parse** sections into typed structures; collect diagnostics for
   anything unrecognized rather than failing hard.
3. **Resolve** cross-references (tables → constants, menus → dialogs, gauges →
   channels) and validate offsets/sizes against declared page sizes.
4. **Freeze** into an immutable `Definition` shared across the app.

### Robustness principles

- **Graceful degradation:** an unknown construct should disable just that feature,
  not the whole file.
- **Golden-file tests:** keep real Speeduino/rusEFI/MS INIs as fixtures and assert
  stable parse results (see [ARCHITECTURE.md §13](ARCHITECTURE.md#13-testing-strategy)).
- **Diagnostics:** surface a parse report (what was understood, what was skipped)
  to help users and contributors.

## Sources of truth

When in doubt, defer to real, current INIs and the firmware projects' own
documentation:

- Speeduino, rusEFI, and MegaSquirt each publish INIs with their firmware.
- The TunerStudio INI format is documented across community wikis and the firmware
  repositories.

> ⚠️ **Implementer note:** exact keyword spellings, field orderings, and optional
> parameters differ across firmwares and versions. Do **not** hardcode against a
> single example; drive behavior from the parsed structure and cover variations
> with fixtures.
