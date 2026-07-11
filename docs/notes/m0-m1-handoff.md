# Hand-off: M0/M1 review remediation

**Date:** 2026-07-11  
**Branch:** `review/m0-m1`  
**Worktree:** `.worktrees/m0-m1-review` (isolated from `m4-table-editors` in main checkout)  
**Base commit:** `aab60f7`  
**Review artifact:** [m0-m1-review.md](m0-m1-review.md)  
**Canvas summary:** [m0-M1-review.canvas.tsx](/Users/dopawlus/.cursor/projects/Users-dopawlus-Projects-private-TuningSoftware/canvases/m0-M1-review.canvas.tsx)

## Context

The user requested a review of milestones **M0** and **M1**, then asked to fix all findings from that review. Work was done on a dedicated branch in a git worktree so the main checkout (where another agent runs on `m4-table-editors`) was not disturbed.

## What was done

### M1 — blocking fixes

1. **Automatic reconnect on real link failures**
   - `ConnectionManager` now owns the live `MsProtocol` (`reconnect.rs`).
   - `check_link()` probes the wire via `read_secl()`; backwards non-wrap `secl` triggers recovery (reboot path).
   - Owner: `poll_tick` treats `PollFrameError::Link` as reconnect; idle `health_tick` (1 Hz) when realtime is off.
   - Shared recovery: `reconnect_session()` in `owner_ops.rs`; simulator demo uses retry hook (link stays down through first attempt).
   - Test: `owner::tests::realtime_transport_failure_automatically_reconnects_and_resumes`.

2. **Signature enforcement on connect and reconnect**
   - `ConnectionManager::connect` and reconnect loop validate `identity.matches(&comms)`.
   - Mismatch → `ProtocolError::SignatureMismatch` or terminal `Failed` (no retry on mismatch).
   - `connect_simulator` / `connect_serial` emit `Failed` after handshake failure (UI not stuck on `Connecting`).
   - Tests in `protocol/tests/reconnect.rs` and `connection::tests::simulator_signature_mismatch_emits_failed_*`.

### M1 — major fixes

3. **Serial transport**
   - `serial_config_from_comms()` applies INI `blockReadTimeout` to read timeout.
   - Writes apply `write_timeout` per operation.
   - I/O errors classified: timeout, disconnect (unplug-like), preserved Io.
   - Disconnected port clears `inner` handle.
   - Unit tests in `transport/src/serial.rs`.

4. **`read_secl` CRC**
   - Single-byte `msEnvelope_1.0` path uses `envelope_read_bytes` (CRC verified).
   - Tests in `protocol/tests/output_channels.rs`.

### M0 — CI and docs

5. **CI**
   - Added `npm run rust:test` → `cargo test --workspace`.
   - Clippy uses `--all-targets`.
   - Restored `windows-latest` in matrix.
   - Added `.gitattributes` (`* text=auto eol=lf`).

6. **Documentation**
   - `README.md` — project status reflects M0–M4 implementation.
   - `CONTRIBUTING.md` — `libudev-dev`, `npm ci`, `rust:test`, no stale test count.
   - `docs/ROADMAP.md` — M0 CI bullet marked complete (3 OS + Rust tests).

7. **Formatting**
   - Prettier fixed: `src/App.integration.test.tsx`, `src/components/offline/OfflinePanel.tsx`.

## Files touched (17 + 2 new)

| Area | Files |
| --- | --- |
| Protocol / reconnect | `src-tauri/crates/protocol/src/reconnect.rs`, `engine.rs`, tests |
| Transport | `src-tauri/crates/transport/src/serial.rs` |
| App wiring | `src-tauri/src/owner.rs`, `owner_ops.rs`, `owner_tests.rs`, `session.rs`, `connection.rs` |
| CI / tooling | `.github/workflows/ci.yml`, `package.json`, `.gitattributes` |
| Docs | `README.md`, `CONTRIBUTING.md`, `docs/ROADMAP.md`, `docs/notes/m0-m1-review.md` |
| Frontend format | `src/App.integration.test.tsx`, `OfflinePanel.tsx` |

## Verification status (local, 2026-07-11)

Last full gate run **before user interrupted** a repeat:

| Gate | Result |
| --- | --- |
| `npm run lint` | pass |
| `npm run format:check` | pass |
| `npm run test` | pass (208 tests) |
| `npm run build` | pass |
| `npm run rust:test` | pass (85 opentune lib tests after new cases) |
| `npm run rust:fmt` | pass |
| `npm run rust:clippy --all-targets` | pass |

**Not run / not verified:**

- Remote GitHub Actions (especially **Windows** runner after `.gitattributes`).
- `npm run tauri dev` GUI smoke.
- Real serial ECU hardware (Speeduino etc.).

## Suggested next steps for the next agent

1. **Re-run full gates** in the worktree (commands below) and fix anything that regressed after the final `connection.rs` signature-emit change.
2. **Push branch** and open PR targeting `main` or `m4-table-editors` (confirm with user which base).
3. **Watch CI** on Windows — if Prettier still fails, run `git add --renormalize .` once and recommit.
4. **Optional hardening** (not blocking M0/M1 closure):
   - Wire `SerialTransport` through injectable port for hardware-free serial I/O tests (review M4 finding — partially addressed via error-mapping unit tests only).
   - Add owner test for fresh **serial** connect signature mismatch (currently covered at `ConnectionManager` + attach path tests).
5. **Manual demo:** connect simulator → Start live → unplug/sim drop → confirm UI shows Reconnecting → Connected without app restart.

## Worktree commands

```sh
# Use the isolated worktree (not main checkout)
cd /Users/dopawlus/Projects/private/TuningSoftware/.worktrees/m0-m1-review
git checkout review/m0-m1

# Full local gates
npm ci
npm run lint && npm run format:check && npm run test && npm run build
CARGO_TARGET_DIR="$PWD/src-tauri/target" npm run rust:test
CARGO_TARGET_DIR="$PWD/src-tauri/target" npm run rust:fmt
CARGO_TARGET_DIR="$PWD/src-tauri/target" npm run rust:clippy
```

## Architecture notes for reviewers

- **Owner task** (`owner.rs`) is the single wire owner; reconnect must stay serialized through it.
- **Glitch vs reboot:** glitch reconnect preserves tune; reboot (`last_reconnect_caused_reidentify`) triggers `load_tune()` in `finish_reconnect`.
- **`load_ini` IPC:** still folded into `connect(source)` — intentional deviation from original M1 plan; no separate command.
- **Simulator reconnect delays:** `connect_simulator` uses 10–100 ms backoff (not 500 ms–30 s) so tests and dev stay fast.

## Merge guidance

This branch is **review remediation**, not a feature milestone. Prefer merging into the active integration branch after CI green and user approval. Do not merge into main checkout worktree while another agent has uncommitted work there — use this worktree or cherry-pick.
