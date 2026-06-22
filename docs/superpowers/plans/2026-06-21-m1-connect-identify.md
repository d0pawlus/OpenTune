# M1 — Connect & Identify Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Parse the comms slice of a real firmware INI, connect to a (simulated)
ECU over a trait-based transport, complete the MS/TS handshake, and **confirm
identity** — with **reliable auto-reconnect** (research pain point #1) as a
first-class feature, not polish.

**Architecture:** Three decoupled crates layered per
[ARCHITECTURE.md §5](../../ARCHITECTURE.md#5-backend-rust-modules):
`transport` (raw bytes) ← `protocol` (the conversation, data-driven from the
INI) ← the app/`realtime`. `ini` supplies the comms settings the protocol reads.
The shared seams between these crates are **already frozen as typed contracts**
(see "Shared contracts" below) so the four component agents can build in
parallel without interface drift. The `simulator` provides a hardware-free ECU
so the whole flow — including a *simulated dropped link* — runs in CI.

**Tech Stack:** Rust (stable), `serialport` (serial enumeration + I/O),
`thiserror` (typed crate errors), `tokio` (async owner task + backoff),
`tauri-specta` (generated IPC types — never hand-write TS), React + TS frontend.

## Global Constraints

These apply to **every task**. Values from
[ARCHITECTURE.md](../../ARCHITECTURE.md), [ROADMAP.md §M1](../../ROADMAP.md),
[protocol.md](../../protocol.md), [ini-format.md](../../ini-format.md), and the
ADRs.

- **Port, don't re-derive.** Per
  [ADR-0006](../../adr/0006-reuse-existing-parsers.md): the INI parser ports from
  [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini) (MIT — GPL-3
  compatible); protocol command bytes/quirks come from the open Speeduino
  `comms.cpp` / rusEFI `tunerstudio.cpp` sources; the simulator ports from
  [`askrejans/speeduino-serial-sim`](https://github.com/askrejans/speeduino-serial-sim).
  Record each source's license. **Confirm command bytes with tests against the
  simulator** — do not trust memory.
- **TDD is mandatory.** Failing test first, minimal code to pass, then refactor.
  No implementation without a failing test. Contract tests already exist (one
  `tests/contract.rs` per crate) and must stay green.
- **Single conversation.** All hardware access is serialized through one owner
  task; never interleave two operations on the wire.
  [ARCHITECTURE.md §9](../../ARCHITECTURE.md#9-concurrency--performance-model)
- **Fail safe.** Every hardware op has a timeout + bounded retry; a failure never
  leaves the ECU or the in-memory state half-written.
- **IPC types are generated from Rust** via `tauri-specta` — never hand-write
  `src/ipc/bindings.ts`.
- **License header** on every new source file: `// SPDX-License-Identifier:
  GPL-3.0-or-later` (Rust) / the TS equivalent.
- **Offline-first; no network telemetry.** Small focused files (<400 lines);
  immutable patterns.
- **i18n:** new UI strings go through the PL/EN dictionaries from M0.
- **Commits:** conventional-commit format, scoped per component.

---

## Shared contracts (already landed — build against these)

These are the **fixed seams** the component agents implement against. They live
as real typed definitions with `todo!()` bodies + doc comments, each pinned by a
`tests/contract.rs`. **Do not change a contract's shape unilaterally** — if a
field is genuinely missing, update the contract *and* its test in one commit and
flag it to the other agents.

| Crate | Contract surface | File |
| --- | --- | --- |
| `transport` | `Transport` trait, `TransportError`, `PortInfo`, `SerialConfig`, `enumerate_ports()` | `crates/transport/src/lib.rs` |
| `ini` | `CommsSettings` (+ `Endianness`, `EnvelopeFormat`, `IniError`), `parse_comms()` | `crates/ini/src/lib.rs` |
| `protocol` | `EcuIdentity`, `ConnectionState` (incl. `Reconnecting { attempt }`), `ProtocolError`, `Protocol` trait | `crates/protocol/src/lib.rs` |

`CommsSettings` field names mirror the **verified** Speeduino INI keywords
(`signature`, `queryCommand`/`query_command`, `versionInfo`/`version_info`,
`pageActivationDelay`, `blockReadTimeout`, `interWriteDelay`, `blockingFactor`,
`endianness`, `messageEnvelopeFormat`, `pageReadCommand`, `pageValueWrite`,
`burnCommand`, `ochGetCommand`). Command templates are stored **verbatim** by
`ini`; expanding `%2i`/`%2o`/`%2c`/`%v` is the `protocol` crate's job.

---

## Ordered tasks (mapped to the 6 M1 roadmap bullets)

### Task 1 — `ini`: parse the comms slice  →  *roadmap bullet 1*

**Files:** `crates/ini/src/{lib,parser}.rs`, golden fixtures under
`crates/ini/tests/fixtures/`, `crates/ini/tests/contract.rs` (extend).

**Interfaces:** Produces a populated [`CommsSettings`] from real INI text.
Consumes nothing else; first in the chain.

- [ ] **1.1** Add a **golden fixture**: a real (trimmed) `speeduino.ini`
  comms/`[MegaTune]`/`[Constants]` block under `tests/fixtures/`. Record its
  source + license in the test module.
- [ ] **1.2** Write a failing test: `parse_comms(fixture)` returns the expected
  `signature`, `query_command`, `version_info`, command templates, endianness,
  and envelope. Run it — RED (`parse_comms` is `todo!()`).
- [ ] **1.3** Port the tokenizer + `key = value` extraction from
  [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini) (ADR-0006): minimal
  `#if/#define/#set` preprocessing, section awareness, graceful skip of unknown
  sections. Implement `parse_comms` to GREEN.
- [ ] **1.4** Add tests for: missing required key → `IniError::MissingKey`; bad
  number/enum → `IniError::InvalidValue`; an INI with `endianness = big` and the
  legacy (plain) envelope. Keep the existing contract test green.
- [ ] **1.5** Commit: `feat(ini): parse comms settings + signature from real INI`.

### Task 2 — `transport`: serial enumerate / open / close + `SimTransport`  →  *roadmap bullet 2*

**Files:** `crates/transport/src/{lib,serial,sim}.rs`, `crates/transport/Cargo.toml`
(add `serialport`), `crates/transport/tests/contract.rs` (extend).

**Interfaces:** Implements the `Transport` trait twice; implements
`enumerate_ports()`. Consumes nothing from sibling crates.

- [ ] **2.1** Add `serialport` dep. Write a failing test that `enumerate_ports()`
  returns `Ok(_)` (possibly empty) and never panics. RED → implement over
  `serialport`, mapping VID/PID into `PortInfo`. GREEN.
- [ ] **2.2** `SerialTransport`: implement `Transport` over a `serialport`
  handle. `read_exact` honors `SerialConfig::read_timeout` and maps a timeout to
  `TransportError::Timeout`, a vanished device to `TransportError::Disconnected`.
  Test the timeout mapping with a loopback or a fake.
- [ ] **2.3** `SimTransport`: an in-process `Transport` backed by a byte queue,
  later wired to the `simulator` (Task 4). Test open/write/read_exact/close +
  the object-safe path (the existing `NullTransport` contract test already
  guards object-safety).
- [ ] **2.4** Commit: `feat(transport): serial enumerate/open + SimTransport`.

### Task 3 — `protocol`: signature/version handshake (generic MS/TS)  →  *roadmap bullet 3*

**Files:** `crates/protocol/src/{lib,engine,framing}.rs`,
`crates/protocol/tests/contract.rs` (extend) + simulator-backed integration test.

**Interfaces:** Implements the `Protocol` trait over a `Transport` +
`CommsSettings`. This is M1's headline operation.

- [ ] **3.1** Failing test: a `GenericProtocol::new(transport, comms)` whose
  `identify()` issues `queryCommand`, reads the signature, issues `versionInfo`,
  reads the version, and returns `EcuIdentity`. Drive it with a scripted
  `SimTransport`. RED.
- [ ] **3.2** Implement command-template expansion (`%2i`/`%2o`/`%2c`/`%v`) and
  the **two framings** selected by `CommsSettings::envelope`: plain (legacy) and
  `msEnvelope_1.0` (length prefix + CRC32). Port CRC + framing from the open
  firmware sources (ADR-0006). GREEN.
- [ ] **3.3** Signature matching: `identify()` (or the connect orchestrator)
  returns `ProtocolError::SignatureMismatch` when `EcuIdentity::matches` is
  false. Test both match and mismatch.
- [ ] **3.4** Implement `read_secl()` (the firmware second counter) — needed by
  reconnect resync in Task 5. Test against the simulator.
- [ ] **3.5** Commit: `feat(protocol): generic MS/TS signature+version handshake`.

### Task 4 — `simulator`: minimal virtual ECU + simulated drop  →  *roadmap bullet 5*

**Files:** `crates/simulator/src/lib.rs`, `crates/simulator/Cargo.toml`
(dep on `ini`, `transport`), `crates/simulator/tests/`.

**Interfaces:** Provides a `Transport` (and/or `Protocol`) that answers
signature/version from a real INI and can be told to **drop the link**.

- [ ] **4.1** Failing test: a `Simulator::from_ini(comms)` answers `queryCommand`
  with the INI signature and `versionInfo` with a version string, exposed as a
  `SimTransport`. RED → port the response logic from
  [`speeduino-serial-sim`](https://github.com/askrejans/speeduino-serial-sim)
  (ADR-0006; record license). GREEN.
- [ ] **4.2** Add a `drop_link()` / `set_connected(false)` control so a test can
  simulate a USB power-save glitch: subsequent reads yield
  `TransportError::Disconnected`, then recover. Test the drop→recover transition.
- [ ] **4.3** Make the simulator answer `read_secl()` with a monotonically
  increasing counter that **resets on a simulated reboot**, so Task 5 can test
  resync-vs-reboot detection.
- [ ] **4.4** Commit: `feat(simulator): virtual ECU with signature + simulated drop`.

### Task 5 — Reliable reconnect with backoff + `secl` resync  →  *roadmap bullet 4*

**Files:** `crates/protocol/src/reconnect.rs` (or a small `realtime`/app
orchestrator), integration tests driving the simulator's drop control.

**Interfaces:** Consumes `Protocol` + `ConnectionState`; owns the retry loop.
This is the headline reliability feature (pain point #1).

- [ ] **5.1** Failing test: given a connected link, when the transport returns
  `Disconnected`, the orchestrator transitions
  `Connected → Reconnecting { attempt: 1 }` and retries with **exponential
  backoff** (bounded), emitting each state. Drive with `Simulator::drop_link()`.
  RED → implement. GREEN.
- [ ] **5.2** `secl` resync: on reconnect, read `secl`; if it went *backwards*
  the ECU rebooted → re-identify + (in M2) re-read pages; otherwise resume
  seamlessly. Test both the glitch (secl advanced) and reboot (secl reset) paths.
- [ ] **5.3** Terminal failure: after N exhausted attempts, transition to
  `Failed { reason }`. Signature mismatch on reconnect → `Failed`, never a silent
  wrong-INI link. Test.
- [ ] **5.4** Commit: `feat(protocol): auto-reconnect with backoff + secl resync`.

### Task 6 — UI: pick port + INI, Connect/Disconnect, show identity & state  →  *roadmap bullet 6*

**Files:** Tauri commands in `src-tauri/src/commands.rs`
(`list_ports`, `load_ini`, `connect`, `disconnect`); a `connection_state` event
in `src-tauri/src/events.rs`; frontend `src/stores/connection.ts` (extend),
`src/components/connect/` + i18n strings. Bindings **regenerated**, not written.

**Interfaces:** Wires the backend chain to the WebView through generated IPC.
Backend stays the single source of truth for connection state.

- [ ] **6.1** Backend: add `list_ports() -> Vec<PortInfo>`, `load_ini(path)`,
  `connect(port, ini) -> EcuIdentity`, `disconnect()`. Hold the transport in a
  single owner task (Tokio) per ARCHITECTURE §9. Unit-test the command logic;
  add the synthetic "Simulator" port option.
- [ ] **6.2** Emit a typed `ConnectionState` event on every transition (incl.
  `Reconnecting`). Regenerate bindings (`cargo run` debug build); confirm
  `src/ipc/bindings.ts` gains the new types — never hand-edit it.
- [ ] **6.3** Frontend: a connect panel — port picker + INI picker, Connect /
  Disconnect, and a status area showing the signature when connected and a clear
  **"reconnecting…"** indicator when `Reconnecting`. Vitest the store reducer
  for each state; PL/EN strings for all labels.
- [ ] **6.4** E2E (simulator, no hardware): connect → see signature → trigger a
  simulated drop → observe the UI recover silently. This is the **M1 demo**.
- [ ] **6.5** Commit: `feat(app): connect/disconnect UI with live connection state`.

---

## Self-review

**Spec coverage (M1 roadmap bullets):**
- `ini` comms + signature (port per ADR-0006) → Task 1. ✅
- `transport` enumerate/open/close + `SimTransport` → Task 2. ✅
- `protocol` signature/version, generic MS/TS handshake → Task 3. ✅
- Reliable reconnect: detect drops, backoff, `secl` resync → Task 5 (+ simulator
  drop control in Task 4). ✅
- `simulator` virtual ECU answering signature/version + simulated drop → Task 4. ✅
- UI: pick port + INI, Connect/Disconnect, show signature & (reconnecting) state
  → Task 6. ✅
- **Demo** (connect to simulator, see signature, recover from a simulated drop)
  → satisfied at Task 6.4. ✅

**Parallelism:** Tasks 1–4 are independent (the shared contracts are frozen and
already compile + test green), so four agents can start at once. Task 3 needs
Task 1's `CommsSettings` (a type, already defined) and Task 2/4's transport (the
trait, already defined) — agents code against the contract, integrate against the
real impls. Task 5 depends on 3+4; Task 6 depends on 1–5.

**Contract stability:** Each seam is pinned by a `tests/contract.rs` that
constructs the types and exercises the traits with in-test fakes — so an
accidental shape change breaks a fast test immediately, before any integration.

**Ported-source ledger (ADR-0006):** Task 1 → `hyper-tuner/ini` (MIT). Task 3 →
Speeduino `comms.cpp` / rusEFI `tunerstudio.cpp` (firmware sources). Task 4 →
`speeduino-serial-sim`. Each task's first commit records the source + license.

**Known risk to watch:** the `ochGetCommand`/realtime command character and the
exact CRC32 polynomial/seed for `msEnvelope_1.0` must be **confirmed against the
firmware source and the simulator with tests** (ADR-0006) — not assumed. The
`CommsSettings` contract reserves the field; Task 3.2 verifies the bytes.
