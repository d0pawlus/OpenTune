# Hand-off: M2/M3 review remediation

**Date:** 2026-07-11  
**Branch:** `review/m2-m3`  
**Worktree:** `.worktrees/review-m2-m3` (isolated from main checkout)  
**Base commit:** `e0586c3` (`Merge pull request #7 — m3-realtime-dashboard`)  
**Scope:** All High/Medium/Low findings from the M2/M3 code review, plus recommended missing tests.

## Context

The user requested a review of milestones **M2** (read/edit/burn) and **M3** (real-time dashboard), then asked to fix **all** actionable findings (High/Medium/Low) and add recommended tests. Work runs on a dedicated git worktree so the main checkout is not disturbed.

Review was performed at `e0586c3` with full gates green (102 frontend tests, full Rust workspace). Remediation is **in progress** — the branch does **not** compile or pass tests yet.

## Review findings (original)

| Severity | Finding |
| --- | --- |
| **High** | Diff/merge lacks per-cell table selection (roadmap requirement). |
| **High** | Partial merge error is immediately cleared by automatic re-diff. |
| **Medium** | `enable`/`visible` positional parsing inverted in UI dialog parser. |
| **Medium** | Dirty state stays true after load → edit → undo (no flash baseline). |
| **Medium** | Stale tune values / snapshot after merge and ECU reboot. |
| **Medium** | Malformed INI can panic (undeclared page, offset overflow). |
| **Medium** | Owner panic leaves frontend falsely “Connected”. |
| **Medium** | Expression-bound gauge ranges fall back to misleading 0–100. |
| **Low** | Various parser/preprocessor/protocol edge cases and missing unit tests. |

## What was done (uncommitted WIP)

### 1. Core hardening (`harden-core`) — largely complete

**INI crate** (`src-tauri/crates/ini/`):

- Fixed **visible/enable positional parsing** in `ui_dialog_parser.rs` (3rd token = enable, 4th = visible per TunerStudio grammar).
- **Constants parser**: reject undeclared pages; checked arithmetic for offsets/shapes/sizes; pointed errors for invalid `page`/`pageSize`; empty numeric fields use defaults instead of `Number::Expr("")`.
- **Output channels**: offset+width validation against `ochBlockSize` with checked arithmetic.
- **Expression parser**: exponent operator and unary `+` support.
- **Preprocessor**: improved diagnostics.
- Extensive new/updated tests in `ini/tests/{constants,expr,output_channels,preprocessor,ui}.rs`.

**Protocol crate** (`src-tauri/crates/protocol/`):

- `pages.rs`: controlled errors instead of truncating size/page-id conversions.
- New tests in `protocol/tests/pages.rs`.

**Model crate** (`src-tauri/crates/model/`):

- New **`MergePick`** enum and **`merge_picks`** in `diff.rs` (cell-level merge).
- **`resolve_number`** on `Tune` for backend gauge-bound resolution.
- New diff tests: cell-level merge, invalid indices; tune tests for Text/Enum diff and bits immutability on rejected set.

**Simulator** (`och_codec.rs`): alignment with output-channel validation changes.

### 2. Cell-level diff/merge (`cell-merge`) — partial

**Done:**

- Model: `MergePick`, `merge_picks`, tests.
- DTOs: `MergePickDto`, extended `FieldDiffDto`/`CellDiffDto` usage.
- Session: `session_diff.rs` → `merge_picks`; `merge_tune` wraps names as `MergePick::All`.
- Frontend: `TuneDiff.tsx` redesigned for per-cell selection, partial-error preservation, `onAfterMerge` callback; tests updated.
- `tune_commands.rs`: `merge_tune(picks: Vec<MergePickDto>)`, new `resolve_gauge_bounds` command stub.
- `lib.rs`: commands and specta type exports registered.

**Not wired (blocks compile):**

- `owner.rs` still has `Command::MergeTune { picks: Vec<String> }` — must become `Vec<MergePickDto>` (or internal `MergePick`) and call `Session::merge_picks`.
- `owner.rs` missing `Command::ResolveGaugeBounds` handler → call `Session::resolve_gauge_bounds`.
- `owner.merge_tune()` helper still takes `Vec<String>`.

### 3. Owner / reconnect / panic (`session-reconnect`) — largely complete

**Done:**

- `lose_session_after_panic()`: clears session, disarms realtime, emits `Disconnected` (fixes false “Connected” after panic).
- Test-only debug commands: `DebugPanicSessionOperation`, `DebugHoldSessionOperation`, `DebugFailNextRebootTuneRead`, `DebugState`.
- `owner_ops.rs`: on reboot reconnect, **clear snapshot** before tune re-read; keep link on failed re-read but invalidate tune+snapshot.
- Many new owner integration tests in `owner_tests.rs` (panic disconnect, failed reboot reread, polling/command serialization, secl baseline during polling, link-drop recovery).

**Still open:**

- **Dirty-after-undo / flash baseline** — not implemented; `Tune::is_dirty` still sticky after undo back to loaded values.
- Confirm frontend connection store reacts to panic-induced `Disconnected` (likely OK via existing event path — verify manually).

### 4. Dashboard / gauges (`dashboard`) — largely complete (frontend side)

**Done:**

- Backend: `Session::resolve_gauge_bounds`, `ResolvedGaugeBoundsDto`.
- Store: `tune.ts` → `gaugeBounds` + `setGaugeBounds`.
- `TunePanel.tsx`: calls `resolveGaugeBounds` on refresh; resets store on failed `loadTune`.
- New `useResolvedGauge.ts`; `RoundGauge`, `BarGauge`, `DigitalGauge` use resolved bounds (no hardcoded 0–100 fallback when range unknown).
- `GaugeCanvas.tsx`: minor canvas hardening.
- `Dashboard.tsx`: async pending guards (`realtimePending`, `savePending`), layout loading state, error handling.
- `layout.rs` / `layout.ts`: persistence improvements and tests.
- `Field.tsx`: minor fix; new/updated tests across dashboard, gauges, Field.

**Not done:**

- TypeScript bindings **not regenerated** → `commands.resolveGaugeBounds` missing in `src/ipc/bindings.ts`.
- Integration test fails: `commands.resolveGaugeBounds is not a function`.

### 5. Integration tests & bindings (`integration-tests`) — not started

- Run `npm run tauri build` or the project's specta binding generator after owner wiring compiles.
- Re-run full gates (see below).
- Optional: add missing integration scenarios called out in review (partial merge over wire, gauge bounds with expression INI fixture).

### 6. Documentation (`verify-document`) — not started

- Update `docs/notes/m2-decisions.md` and `docs/notes/m3-decisions.md` with remediation decisions.
- Mark ROADMAP items if appropriate (only after gates green + user approval).

## Known compile / test failures (2026-07-11)

```
error[E0599]: no variant `ResolveGaugeBounds` on `owner::Command`
  --> src/tune_commands.rs:52

error[E0308]: MergeTune expects `Vec<String>`, found `Vec<MergePickDto>`
  --> src/tune_commands.rs:134
```

Frontend:

```
TypeError: commands.resolveGaugeBounds is not a function
  --> TunePanel.tsx:75
```

**109 frontend tests:** 108 pass, 1 fails (+ 3 unhandled rejections from missing mock).

## Suggested next steps (priority order)

1. **Wire owner commands** (unblocks everything):
   - Add `ResolveGaugeBounds { reply: Reply<Vec<ResolvedGaugeBoundsDto>> }` to `Command`.
   - Change `MergeTune` to `picks: Vec<MergePickDto>`; convert via `MergePick::from` in handler.
   - Update `Owner::merge_tune` to call `Session::merge_picks`.
   - Handle new command in `dispatch` match arm.

2. **Regenerate bindings** — run the project's specta/typescript export (see `src-tauri/src/lib.rs` `export_typescript_bindings_*` test or `npm run` script). Verify `MergePickDto`, `ResolvedGaugeBoundsDto`, `resolveGaugeBounds`, updated `mergeTune` signature appear in `src/ipc/bindings.ts`.

3. **Fix frontend mocks** — update test mocks / `TunePanel` integration stubs to include `resolveGaugeBounds`.

4. **Dirty / flash baseline** — implement minimal safe design from M2 review:
   - Record flash baseline at `load_tune` (or first successful read).
   - `is_dirty` = any page bytes differ from flash baseline (not merely “had an edit”).
   - Add model tests in `tune_state.rs`.

5. **Run full gates** in the worktree:

   ```sh
   cd /Users/dopawlus/Projects/private/TuningSoftware/.worktrees/review-m2-m3
   npm ci
   npm run lint && npm run format:check && npm run test && npm run build
   CARGO_TARGET_DIR="$PWD/src-tauri/target" cargo test --manifest-path src-tauri/Cargo.toml --workspace
   CARGO_TARGET_DIR="$PWD/src-tauri/target" cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
   CARGO_TARGET_DIR="$PWD/src-tauri/target" cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
   ```

6. **Update decision notes** and optionally open PR after user confirms base branch.

## Files touched (45 modified + 1 new)

| Area | Key files |
| --- | --- |
| INI | `constants_parser.rs`, `constants_fields.rs`, `ui_dialog_parser.rs`, `expr_parser.rs`, `output_channels_parser.rs`, `preprocessor.rs`, tests |
| Model | `diff.rs`, `tune.rs`, `lib.rs`, `tests/diff.rs`, `tests/tune.rs` |
| Protocol | `pages.rs`, `tests/pages.rs` |
| Simulator | `och_codec.rs` |
| Owner/session | `owner.rs`, `owner_ops.rs`, `owner_tests.rs`, `session.rs`, `session_diff.rs` |
| IPC | `dto.rs`, `tune_commands.rs`, `lib.rs`, `layout.rs` |
| Frontend tune/diff | `TunePanel.tsx`, `TuneDiff.tsx`, `stores/tune.ts`, tests |
| Frontend dashboard | `Dashboard.tsx`, `layout.ts`, `Field.tsx`, tests |
| Frontend gauges | `useResolvedGauge.ts` (new), `RoundGauge.tsx`, `BarGauge.tsx`, `DigitalGauge.tsx`, `GaugeCanvas.tsx`, tests |

## Architecture notes for the next agent

- **Owner task** remains the single wire owner; all new commands follow the existing `request()` + oneshot pattern.
- **Cell merge** must stay **per-pick wire commit** (see `session_diff.rs` module doc) — do not batch multi-page deltas.
- **Gauge bounds** are intentionally resolved in the backend (`Tune::resolve_number`) and cached in the tune store; realtime frames still bypass React.
- **Partial merge errors**: frontend must not auto-clear error on re-diff; `TuneDiff` was updated — verify behavior end-to-end after owner wiring.
- **Reboot reconnect**: snapshot is cleared in `owner_ops.rs`; tune re-read failure keeps link but drops tune — frontend should show error and empty tune panel.
- **Do not edit** the superpowers plan file (`docs/superpowers/plans/…`); update `docs/notes/m2-decisions.md` / `m3-decisions.md` instead.

## Worktree commands

```sh
cd /Users/dopawlus/Projects/private/TuningSoftware/.worktrees/review-m2-m3
git checkout review/m2-m3
git status   # expect WIP commit or dirty tree until remediation finishes
```

## Merge guidance

This branch is **review remediation**, not a feature milestone. Do not merge until compile + full gates are green. Prefer merging into the active integration branch after CI and user approval. Keep using this worktree — do not mix with uncommitted work in the main checkout.
