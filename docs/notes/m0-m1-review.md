# Review M0 and M1

Review date: 2026-07-11  
Reviewed branch: `review/m0-m1`  
Baseline: `aab60f7` (`m4-table-editors`)

## Verdict

- **M0 — accepted locally.** All declared frontend and Rust gates pass. CI now
  includes Cargo tests, all-target clippy, and macOS/Linux/Windows runners with
  repository-wide LF normalization.
- **M1 — accepted at code and simulator-test level.** Real owner/session link
  failures enter reconnect automatically, every handshake validates the ECU
  signature, and reconnect keeps the tune across glitches while re-reading it
  after a detected reboot.

Remote Windows CI, a native GUI walkthrough, and real-ECU hardware validation
remain external verification steps rather than known code defects.

## Resolved blocking findings

### B1 — Real link failures now trigger reconnect

- `ConnectionManager` retains the live protocol and exposes an idle health
  check.
- The owner uses realtime polling as the active health probe and a one-second
  check while polling is idle.
- Poll/health failures enter one shared reconnect operation which emits
  `Reconnecting` and terminal `Connected`/`Failed` states.
- Successful glitch recovery keeps polling and the tune intact. A backwards
  non-wrap `secl` value enters the same reconnect/re-identify path and reloads
  the tune after reboot.
- `realtime_transport_failure_automatically_reconnects_and_resumes` proves
  recovery without invoking the simulator-only drop command.

### B2 — Signature mismatch now fails connect and reconnect

- `ConnectionManager` checks `identity.matches(&self.comms)` after every
  `identify()`.
- Initial mismatch returns `ProtocolError::SignatureMismatch`.
- Reconnect mismatch stops immediately in `Failed` rather than retrying.
- Focused tests cover initial and reconnect-only mismatches; attach and
  pre-write checks remain as defense in depth.

## Resolved major findings

- CI runs `cargo test --workspace`; clippy now includes `--all-targets`.
- `.gitattributes` normalizes text to LF and Windows is restored to the CI
  matrix.
- The two Prettier failures are formatted.
- `blockReadTimeout` from the INI seeds `SerialConfig::read_timeout`.
- Serial writes temporarily apply `write_timeout`; timeout/unplug-like errors
  are classified and a disconnected port clears its live handle.
- The simulator demo holds the link down through a failed retry before
  restoring it.
- Single-byte `msEnvelope_1.0` `read_secl` responses use the shared
  CRC-validating envelope reader, with success and corrupt-CRC tests.
- README and CONTRIBUTING reflect the implemented application, Linux
  `libudev-dev`, current commands, and non-hard-coded test counts.

## Verified strengths retained

- The Tauri v2, React, TypeScript, and Vite skeleton remains buildable.
- Rust domain crates remain decoupled from Tauri through clear
  transport/protocol/INI seams.
- Rust remains the source of truth for generated TypeScript IPC bindings.
- PL/EN i18n and theme scaffolding remain test-pinned.
- INI comms parsing is exercised against realistic Speeduino input.
- Plain and `msEnvelope_1.0` framing, CRC32, simulator identity/drop behavior,
  reconnect backoff, `secl` glitch/reboot handling, and terminal failure all
  have focused tests.
- The owner task serializes hardware access and resumes realtime frames after
  automatic reconnect.

## Verification performed after remediation

- `npm run lint` — passed.
- `npm run format:check` — passed.
- `npm run test` — passed: 26 files, 208 tests.
- `npm run build` — passed; Vite emitted only the existing lazy 3D chunk-size
  warning.
- `npm run rust:test` — passed for the complete workspace.
- `npm run rust:fmt` — passed.
- `npm run rust:clippy` — passed with `--all-targets -D warnings`.
- `npm ci` reported one low-severity dependency advisory.

No native `tauri dev` GUI walkthrough or real-ECU hardware test was performed.
