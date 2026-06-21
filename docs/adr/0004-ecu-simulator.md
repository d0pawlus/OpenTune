# 0004 — A first-class ECU simulator

- **Status:** Accepted
- **Date:** 2026-06-21

## Context

Tuning software is inherently coupled to hardware: normally you need a physical
ECU (and ideally an engine) to exercise connect, read/write, burn, and real-time
data. That is a serious barrier to:

- **Contributors** without hardware,
- **Automated testing/CI**, which has no ECU attached, and
- **Reproducing bugs** deterministically.

"Easy to develop" is a stated project goal, so we must remove the hardware
dependency from the everyday development loop.

## Decision

Build an **ECU simulator** as a **first-class component** (its own crate; see
[ARCHITECTURE.md §10](../ARCHITECTURE.md#10-the-ecu-simulator)), not a test mock.
It implements the same `Transport`/`Protocol` surface as real hardware:

- loads any real INI and reports a matching signature/version,
- serves page reads/writes/burns against a backing memory image, and
- synthesizes plausible, animated real-time channels (correlated RPM/MAP/TPS,
  warming temperatures, AFR, etc.).

It is selectable through the normal connect flow (a "Simulator" device) and used by
CI for end-to-end tests.

## Consequences

**Positive**

- **Hardware-free development.** Anyone can run and improve the full app.
- **Deterministic CI/E2E tests** of connect/read/write/burn/realtime without a
  board.
- **Better demos and onboarding** — the app does something interesting on first
  launch.
- **Forces good abstractions.** Making real hardware and the simulator
  interchangeable validates the transport/protocol boundary.

**Negative / costs**

- **Maintenance**: the simulator must track protocol/INI features to stay useful;
  it can drift from real behavior. Mitigated by keeping it on the same trait
  surface and periodically validating against real hardware.
- **False confidence risk**: passing against the simulator is not proof against
  real firmware. Mitigated by a hardware-in-the-loop test lane and a community
  testing guide.

## Alternatives considered

- **Plain unit-test mocks only.** Cheaper, but can't exercise end-to-end flows or
  give contributors a runnable app. Rejected as insufficient.
- **Record/replay of real serial traffic.** Useful for regression fidelity, but
  rigid (no interactive writes) and hardware-dependent to capture. Kept as a
  possible *complement* to the simulator, not a replacement.
