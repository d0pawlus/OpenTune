# Glossary

Domain terms used throughout OpenTune's docs and code. Aimed at contributors who
are strong developers but new to engine tuning (or vice versa).

## ECU / EMS

**Engine Control Unit** (a.k.a. Engine Management System). The computer that runs
the engine — reads sensors, computes fuel and ignition, and drives injectors and
coils. OpenTune connects to, configures, and tunes ECUs.

## Tuning

Adjusting the ECU's configuration (mostly tables) so the engine runs correctly and
optimally across all conditions — the core activity OpenTune supports.

## INI / firmware definition

A text file shipped by the firmware that describes the ECU's memory layout, real-
time channels, UI, and communication settings. The key to OpenTune's universality.
See [ini-format.md](ini-format.md).

## Signature

A string the ECU reports identifying its firmware/version. Used to match the right
INI to the connected ECU.

## Page

A contiguous region of ECU configuration memory, read/written/burned as a unit.
Constants live within pages. See [protocol.md](protocol.md).

## Constant

A single named configuration value (scalar, array, or bit field) stored at a known
page+offset with a type and scaling. Editing constants is how you change the tune.

## Scaling (scale / translate)

The transform between the raw stored value and the human-readable physical value:
`physical = raw * scale + translate`. The inverse applies when writing.

## Tune (.msq)

The complete set of configuration values for an engine — what you save, load, and
share. `.msq` is TunerStudio's XML tune format; OpenTune reads/writes it for
interoperability.

## Output channel

A single real-time telemetry value streamed by the ECU (e.g., RPM, MAP, coolant
temp, AFR). The dashboard and datalogger consume these.

## Real-time data

The live stream of output channels read continuously while connected, driving the
gauges and datalogs.

## Burn

Persisting the current configuration from the ECU's volatile RAM to non-volatile
flash so it survives a power cycle. Live edits hit RAM immediately; **burn** makes
them permanent.

## Live tuning

Changing constants while the engine runs, with effects applied immediately to RAM —
the everyday tuning workflow. Followed by a **burn** to keep the changes.

## Table (map) — VE, ignition, AFR target

A 2D/3D lookup the ECU interpolates at runtime. Common ones:

- **VE table** — Volumetric Efficiency; the primary fueling table (RPM × load).
- **Ignition/spark table** — spark advance (RPM × load).
- **AFR target table** — desired Air-Fuel Ratio (RPM × load).

Editing these is the heart of tuning; OpenTune offers 2D heatmap and 3D surface
editors.

## Curve

A 1D lookup (e.g., warm-up enrichment vs. coolant temperature). Edited in a curve
editor.

## AFR / Lambda

**Air-Fuel Ratio** (and its normalized form, **Lambda**) — how rich/lean the
mixture is. Measured by a wideband O2 sensor and central to fueling decisions.

## MAP / TPS / RPM / CLT / IAT

Common sensor channels: **MAP** (Manifold Absolute Pressure / load), **TPS**
(Throttle Position), **RPM** (engine speed), **CLT** (Coolant Temperature), **IAT**
(Intake Air Temperature).

## Datalog (.mlg / CSV)

A recording of output channels over time, replayed and analyzed to guide tuning.
`.mlg` is TunerStudio's binary log format; CSV is the portable alternative.

## Auto-tune / VE Analyze

Tooling that compares logged AFR against the AFR target and suggests corrections to
the VE table, accelerating tuning.

## Speeduino / MegaSquirt / rusEFI

The open and semi-open ECU platforms OpenTune targets first. They share a broadly
compatible serial protocol and the INI definition format, which is what makes a
single universal application practical.

## TunerStudio

The incumbent, closed-source tuning application OpenTune aims to replace. It
defined the INI/.msq/.mlg formats this ecosystem relies on; we stay interoperable
with them.

## Tauri

The framework OpenTune is built on: a Rust backend ("core") plus a web frontend in
a native WebView. See [ADR-0001](adr/0001-tauri-stack.md).

## IPC

**Inter-Process Communication** — here, the typed boundary between the Rust backend
and the web frontend (Tauri commands and events). See
[ARCHITECTURE.md §7](ARCHITECTURE.md#7-the-ipc-contract).
