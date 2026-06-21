# 0001 — Use Tauri (Rust + web) for the application shell

- **Status:** Accepted
- **Date:** 2026-06-21

## Context

OpenTune must be a fast, cross-platform desktop application with first-class
support for modern macOS (Apple Silicon, strict notarization), plus Windows and
Linux. The most important runtime concerns are:

- **Reliable serial/USB communication** with ECUs.
- **High-frequency real-time data** (tens of Hz) driving gauges and datalogs.
- **Heavy rendering**: 3D tables and large datalog charts.
- **Small, easy-to-distribute, signable binaries.**

It must also be **easy to develop** (to attract contributors) and **genuinely
open source**. The incumbent, TunerStudio, is a Java/Swing app whose future on
modern macOS is uncertain and which is not open source.

## Decision

Build the application on **Tauri v2**: a **Rust backend** ("core") plus a **web
frontend** rendered in the OS WebView, communicating over Tauri's typed IPC.

## Consequences

**Positive**

- **Native performance & tiny binaries.** No bundled browser engine (unlike
  Electron); the OS WebView is used. Binaries are typically a few MB.
- **Strong macOS / Apple Silicon story.** Native arm64 builds, code signing,
  notarization, and an auto-updater are well supported — directly addressing the
  reason for replacing TunerStudio.
- **Rust where it counts.** Serial I/O, protocol handling, INI parsing, and the
  real-time loop are in a fast, memory-safe systems language with a great
  concurrency story (Tokio) and a mature serial crate (`serialport`).
- **Modern, approachable UI.** The frontend is standard web tech, with a huge
  contributor pool.
- **Clean separation.** The IPC boundary enforces a tidy split between
  hardware/domain logic (Rust) and presentation (web).

**Negative / costs**

- **Two languages** (Rust + TypeScript) raise the contribution bar versus a
  single-language stack. Mitigated by clear module boundaries, generated TS types
  from Rust, and good docs.
- **WebView differences** across platforms can cause minor rendering quirks.
  Mitigated by sticking to well-supported web features and testing on all OSes.
- **Rust learning curve** for some contributors. Mitigated by keeping domain
  crates small, well-documented, and decoupled from Tauri.

## Alternatives considered

- **Electron (Node + web).** Easiest for web developers and a huge ecosystem, but
  heavy (~150 MB bundles), higher memory use, and a less ideal real-time/serial
  path (JS + node-serialport) for our performance goals. Rejected on
  footprint/performance.
- **Flutter (Dart).** Good performance and a single codebase incl. mobile, but a
  smaller ecosystem for serial/embedded work and Dart is less common among the
  systems/embedded contributors this project will attract. Rejected for ecosystem
  fit.
- **Qt (C++/PySide).** Truly native and performant, but heavier, more ceremony to
  build/distribute, and a steeper contribution curve. Rejected for developer
  velocity/openness.
- **Pure web app + WebSerial.** Zero install and very accessible, but limited to
  Chromium browsers, weaker offline/file integration, and not a credible full
  desktop replacement for TunerStudio. Rejected as the primary form (could still
  exist as a companion later).

## Notes

This decision interacts with [ADR-0003](0003-frontend-stack.md) (frontend
framework) and [ADR-0004](0004-ecu-simulator.md) (simulator, which reduces the
hardware burden on contributors).
