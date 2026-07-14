# M5 review fixes — remediation plan

Date: 2026-07-14. Branch: `fix/m5-review-fixes` off main @ 3d088d1.
Source: M5 review of PR #11 (datalog recording, analysis, viewer UI) —
3 CRITICAL, 4 HIGH, 6 MEDIUM, 5 LOW, all verified in code. This plan
addresses every finding. One LOW (MLG bitfield field types unsupported)
is accepted as-is: the codec fails safe with a typed error; documented in
the PR body, no code change.

## Global Constraints

These bind every task and every reviewer:

- TDD: write the failing test first (RED), then the fix (GREEN). Every
  behavioral change lands with a test that fails without it.
- Gates per task, all green before commit:
  - Rust (run from `src-tauri/`, prefix with `. "$HOME/.cargo/env" && `):
    `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
    `cargo test --workspace`
  - Frontend (repo root): `npm run lint` (max-warnings 0),
    `npm run format:check`, `npx tsc --noEmit -p tsconfig.json`, `npm test`
- Immutability in TS: never mutate existing objects/arrays in stores or
  props; return new copies (Zustand `set` with fresh objects).
- All user-facing strings go through i18n with BOTH `en` and `pl` keys
  (parity is type-enforced by `src/i18n`). No hardcoded UI strings.
- Never edit `src/ipc/bindings.ts` by hand — it is generated. Regenerate by
  running `cargo test binding_gen` in `src-tauri/` after changing commands,
  events, or DTOs.
- Error handling: no silently swallowed errors; user-facing failures get a
  clear message; backend errors keep detail.
- File caps: no file may end a task over 800 lines unless the task existed
  to split it (Tasks are ordered so splits come last).
- Commit format: `<type>(<scope>): <description>` (conventional commits).
  NO attribution footers of any kind (no Co-Authored-By, no "Generated
  with" lines) — attribution is disabled for this repository.
- Scope discipline: fix exactly what the task says. No drive-by refactors,
  no new dependencies.

## Task 1: Backend log-file hardening (H1, M1, M2, M3, M5, LOW-csv, LOW-marker)

**Files:** `src-tauri/src/owner.rs` (log commands around lines 1220–1420),
`src-tauri/crates/datalog/src/csv.rs`, tests co-located.

All changes are backend (Rust) only. Seven sub-fixes:

**1a. H1 — validate file paths from the webview.**
`start_log` (line ~1220) validates only `path.is_empty()`; `open_log`
(~1315) and `save_log` (~1328) validate nothing. Add two helpers (in
`owner.rs` or a small new module):

- `validate_log_write_path(path: &str) -> Result<PathBuf, String>`:
  trim; reject empty; expand a leading `~` (reuse the existing
  tilde-expansion helper used by the INI path picker — find it with
  `grep -rn "tilde\|expand_home\|shellexpand" src-tauri/src src-tauri/crates`);
  require extension `csv` or `mlg` (case-insensitive); the parent
  directory must exist and canonicalize successfully; the final path must
  not itself be an existing directory. Return the canonicalized-parent +
  filename PathBuf.
- `validate_log_read_path(path: &str) -> Result<PathBuf, String>`:
  same trim/expand; file must exist, be a regular file, extension
  `csv`/`mlg`; canonicalize.

Wire into `start_log`, `save_log` (write) and `open_log` (read). Error
messages must name the problem ("parent directory does not exist", "log
files must end in .csv or .mlg"), not just "invalid path".

**1b. M1 — cap `open_log` file size.** `open_log` currently reads the whole
file unbounded (`read_to_end`). Check `std::fs::metadata` length first;
reject files larger than a named constant
`MAX_LOG_FILE_BYTES: u64 = 256 * 1024 * 1024` with a clear error.

**1c. M2 — `start_log` must fail without a realtime source.** Today a log
started when the och (output-channels) block size is zero/unknown silently
records 0 rows. `start_log` must return a clear error when there is no
active ECU session or the realtime definition has no output channels
(och block size == 0). Locate where realtime frames feed
`ActiveLog` to find the right predicate.

**1d. M3 — disconnect must not mask flush failures.** During Disconnect the
owner calls `stop_log()`; today a flush failure surfaces as a disconnect
error (disconnect appears failed although it succeeded). Change: always
complete the disconnect; if the log flush failed, report it distinctly
(log with context + surface a message that says the log flush failed but
the device was disconnected).

**1e. M5 — atomic log save.** `write_log_path` uses `File::create` directly
on the destination (truncates; a crash mid-write corrupts an existing
file). Write to a temp file in the same directory
(`<name>.<ext>.tmp-<pid>` or similar), then `std::fs::rename` over the
destination. Remove the temp file on error.

**1f. LOW — CSV non-finite values round-trip consistently.** In
`crates/datalog/src/csv.rs` only the time column guards `is_finite`
(line ~71). Decide one policy for data cells: non-finite values (NaN,
±inf) serialize as an empty cell, and the CSV reader parses empty cells
back to the same "missing" representation the MLG path uses. Add a
round-trip test with NaN and ±inf values.

**1g. LOW — marker duplication at page boundaries.** `get_log_data` paging
can emit the same marker on two adjacent pages (currently masked by a
frontend dedup). Fix at the source: each marker is emitted on exactly one
page. Keep the frontend dedup as belt-and-suspenders. Add a paging test
with a marker exactly on a page boundary.

**Verification:** Rust gates green. New tests: path validation (accept ~,
reject missing parent/bad ext/dir target), size cap, start-without-och,
atomic-save (temp gone, dest correct), CSV non-finite round-trip, marker
boundary paging.

## Task 2: C2 — log identity token through the IPC (backend + store)

**Files:** `src-tauri/src/owner.rs`, `src-tauri/src/dto.rs`,
`src/ipc/bindings.ts` (regenerated only), `src/stores/datalog.ts`,
`src/components/datalog/DatalogPanel.tsx` (small: disable Open while busy).

**Problem:** the backend holds a single `opened_log: Option<Log>` slot. The
frontend pages `get_log_data` in a multi-round-trip loop guarded only by
`offset`. Opening log B while log A is still paging silently splices B's
rows into A's dataset (A/B compare, stats, export all corrupt silently).

**Fix — generation token:**

- Owner state gains `log_generation: u32`, incremented every time
  `opened_log` is assigned (in `open_log` AND in `stop_log`'s auto-open of
  the just-recorded log).
- `open_log` (and the `stop_log` summary, if it exposes the opened log)
  returns the new generation in its DTO (`log_id: u32`).
- `get_log_data` and `save_log` take a required `log_id: u32` parameter;
  on mismatch with the current generation return a typed error string
  (e.g. `"log changed since it was opened"`) — never stale data.
- The analysis commands that read `opened_log` (`log_stats`,
  `detect_anomaly`, `virtual_dyno` — confirm names in `owner.rs`) take the
  same `log_id` parameter with the same mismatch behavior.
- Regenerate bindings: `cargo test binding_gen` in `src-tauri/`.

**Frontend threading (`src/stores/datalog.ts`):**

- `openLog` captures `log_id` from the open response and passes it to every
  `getLogData` page call; on a mismatch error, abort the paging loop and
  surface the store's normal error state (no partial dataset may be kept).
- Store each slot's `log_id` with its dataset; `exportLog` and the analysis
  calls send the `log_id` of the dataset they operate on. `activate()`
  (re-open for analysis) updates the stored id.
- Add `loadingLog: boolean` (or per-slot) state; `DatalogPanel` disables the
  Open/Load buttons while a load is in flight.

**Tests:** Rust — mismatch on `get_log_data`/`save_log`/one analysis command
returns the typed error; generation increments on both assignment sites.
FE — store test: paging loop aborts and clears when a page returns the
mismatch error; Open button disabled while loading (component test).

## Task 3: C3 — flush active recording on app exit

**Files:** `src-tauri/src/lib.rs` (tauri `run` / builder), `src-tauri/src/owner.rs`
(small helper if needed).

**Problem:** only StopLog and Disconnect flush an active recording. Closing
the app window mid-recording loses the entire session buffer — there is no
`ExitRequested` handling anywhere in `src-tauri`.

**Fix:** in the `.run(...)` event callback handle
`tauri::RunEvent::ExitRequested { api, .. }`:

- Keep a shared `Arc<AtomicBool>` "exit flush done" guard plus a way to ask
  the owner if a log is active (the owner command channel already exists —
  send it a StopLog and inspect the reply; a StopLog with no active log
  must be treated as "nothing to do", not an error, for this path).
- First ExitRequested with a (possibly) active log: `api.prevent_exit()`,
  spawn an async task that sends StopLog to the owner, awaits the reply
  with a hard timeout (5 s, named constant), sets the guard, then calls
  `app_handle.exit(0)`.
- When the guard is already set (second pass), do not prevent exit.
- The flush must be best-effort: a StopLog error or timeout still exits
  (log the error); the app must never hang on close.

**Tests:** extract the decision logic ("should this ExitRequested be
deferred?") into a small pure/unit-testable helper and test: first call
with guard unset → defer; guard set → allow; plus owner-side test that
StopLog with no active log is a no-op success for the exit path (if
behavior changes are needed there). Manual note in the report: full
end-to-end exit flush verified by `cargo test` for logic; GUI exit not
automatable here.

## Task 4: C1 — uPlot mode-2 placeholder in DatalogCharts (+ tests)

**Files:** `src/components/datalog/DatalogCharts.tsx`, new
`src/components/datalog/facetedPlotConfig.ts` + test.

**Problem:** uPlot `mode: 2` (faceted) requires a placeholder at index 0 of
BOTH `series` and `data` (`setDefaults2` forces `series[0] = {}`; the
constructor reads `series[1].facets[0].scale`; draw loops start at i=1).
`useFacetedPlot` passes only real series → with exactly 1 series the
constructor throws a TypeError (default view selects 1 channel:
`channels.slice(0, 1)`), and with ≥2 the FIRST series is silently dropped
(A/B compare shows only B; DynoChart never draws WHP).

**Fix:**

- Extract a pure function `buildFacetedPlot(series, xBounds, yBounds, scatter, width)`
  returning `{ options, data }`, used by `useFacetedPlot`. It must produce
  `series: [{}, ...realSeries]` and `data: [null, ...realSeriesData]`
  (uPlot's documented faceted-data shape), preserving all current options
  (scales with bound overrides, axes, legend, cursor drag).
- `useFacetedPlot` consumes it; behavior otherwise unchanged.

**Tests (vitest, pure — jsdom cannot construct real uPlot canvases):**

- placeholder `{}` at `series[0]` and `null` at `data[0]`;
- 1 input series → `options.series.length === 2` and
  `options.series[1].facets[0].scale === "x"`;
- 2 input series → both present at indices 1..2 with labels/colors intact;
- scatter flag switches paths/points config as today.

## Task 5: H2 + H4 + M4 — playback escape hatch, RAF loop, strict gauge apply

**Files:** `src/stores/datalog.ts` (~lines 276–301), `src/components/datalog/DatalogPanel.tsx`
(playback component, ~lines 215–252), `src/i18n/en.ts` + `pl.ts`,
`src/App.tsx` only if the strict-apply wiring requires it.

**H2 — playback traps the dashboard.** Once a user plays or scrubs,
`replaying: true` gates ALL live frames off the dashboard (`App.tsx:47`)
and nothing in the UI ever resets it (`stopPlayback` exists but no control
calls it). Fix:

- Add a visible Stop/"back to live" button to the playback controls that
  calls `stopPlayback()` (which must reset `playing: false`,
  `replaying: false`).
- Add a visible "replay" indicator near the playback controls (i18n key,
  EN "Replay — live gauges paused" / PL equivalent) shown whenever
  `replaying` is true, so the frozen-live state is never silent.
- Closing/unloading the dataset that is being replayed must call
  `stopPlayback()`.
- Pausing intentionally KEEPS `replaying: true` (a paused replay must not
  be overwritten by live frames) — the escape is the explicit Stop.

**H4 — RAF loop restarts every tick.** The playback `useEffect` lists `row`
in its deps, so every frame tears down and restarts the loop, recomputes
`const finalTime = [...dataset.tMs].reverse().find(...)` (full copy +
reverse of up to 100k elements per frame) and resets the time base
(drift). Fix:

- Extract `lastValidTime(tMs: (number | null)[]): number` (scan backwards,
  no copy) into the store module or a small util, with a unit test.
- Memoize `finalTime` per dataset (`useMemo`).
- Drive the loop from a ref for the current row; effect deps become
  `[playing, speed, dataset]` (or equivalent) — the loop must advance from
  `performance.now()` deltas and MUST NOT restart on row changes.

**M4 — playback must not hide log gaps.** Playback currently routes frames
through the live fail-open `applyFrame`, so a null gap in the log shows a
frozen stale value on the gauges. The replay path from the M3 fixes
already clears gauges on null gaps (`grep -rn "replay" src/stores` to find
it). Route playback row application through that strict path so null cells
clear the corresponding gauges.

**Tests:** store — `stopPlayback` resets both flags; pause keeps
`replaying`; unload-while-replaying resets; `lastValidTime` unit test
(trailing nulls, all-null, empty). Component — Stop button visible during
replay and calls `stopPlayback`; indicator rendered when `replaying`.
Strict-apply — a null cell clears the gauge value (extend the existing
replay-strict tests).

## Task 6: H3 — math channels must not break analysis commands

**Files:** `src/stores/datalog.ts` (`withMathChannels`, ~lines 62–75; analysis
call sites), `src/components/datalog/DatalogPanel.tsx` (analysis section
channel picker), tests.

**Problem:** `withMathChannels` pushes derived channel names into the
dataset's `fields` list. The analysis section builds its channel list from
`fields` and `runStats` sends ALL of them; the backend rejects unknown
names with `MissingChannel`, so defining a single math channel makes Log
Stats (and the other analysis commands) fail entirely.

**Fix:**

- Keep derived channels OUT of `fields`. Track them separately (e.g.
  `mathChannelNames: string[]` on the dataset or store) while still
  merging their columns into `columns` for charting.
- Chart channel pickers keep offering real + math channels.
- Analysis requests (`runStats`, anomaly, dyno config) must only ever send
  real backend field names. If the analysis picker lists channels, math
  channels are either excluded or visibly disabled there (pick one,
  implement consistently, i18n for any new label).

**Tests:** store — dataset with a math channel: `fields` unchanged,
`columns` contains the derived column, the `runStats` payload contains
only real field names (assert on the mocked command call). Existing math
channel chart tests keep passing.

## Task 7: LOW — i18n sweep + native file dialogs for log paths

**Files:** `src/components/datalog/DatalogPanel.tsx`,
`src/components/datalog/DatalogCharts.tsx`, `src/i18n/en.ts`, `src/i18n/pl.ts`.

**7a. i18n sweep.** Replace every hardcoded user-facing string in the
datalog UI with i18n keys (EN + PL): "No chart data", axis labels
"X"/"Y", "WHP"/"Torque Nm" series labels, stats table headers
(Min/Max/Mean/…), dyno config labels (Load/Knock/RPM/Cd …), and any
playback/export/marker strings added by M5. Parity is type-enforced —
`npm run lint` + `tsc` catch missing keys; the i18n test suite must pass.
Series labels like "A: RPM" keep the raw channel name; only the fixed
words get keys.

**7b. Native file dialogs.** The panel takes log file paths via free-text
inputs although `@tauri-apps/plugin-dialog` is installed. Follow the
existing pattern (see `OfflinePanel`'s folder/file pickers) and add
Browse buttons using `open()` for the open-log path and `save()` for the
record/export destinations (filters: `csv`, `mlg`). Keep the text input
as an editable fallback (backend validates since Task 1). Verify the
dialog capability/permission is granted in `src-tauri/capabilities/`
(the OfflinePanel picker already works, so it should be); if missing, add
`dialog:default`.

**Tests:** i18n suite green (parity); component tests for the Browse
buttons (mock the dialog module; assert the chosen path lands in the
input/store). No English literals remain in the datalog components
(spot-check by grep in the report).

## Task 8: MEDIUM — file-size caps: split DatalogPanel.tsx and owner.rs

**Files:** `src/components/datalog/DatalogPanel.tsx` (967+ lines) → split;
`src-tauri/src/owner.rs` (1551 lines) → split. PURE MOVES — zero behavior
change, zero signature change visible to callers.

- `DatalogPanel.tsx`: extract cohesive child components into sibling files
  (suggested seams: `PlaybackControls.tsx`, `AnalysisSection.tsx`,
  `CompareSection.tsx`/`MarkersSection.tsx`, `ExportControls.tsx` — follow
  the natural JSX structure). Every resulting file < 800 lines (target
  < 400). Props stay minimal; state stays in the store as today.
- `owner.rs`: move the log/datalog command handling (start/stop/open/save/
  get_log_data + analysis command impls) into a new module (e.g.
  `src-tauri/src/owner/logging.rs` with `owner.rs` → `owner/mod.rs`, or a
  sibling `owner_logging.rs` with an `impl` block — pick what needs the
  least churn). Every resulting file < 800 lines. `pub(crate)` visibility
  as needed; no API change.

**Verification:** full gates both sides; test counts identical before/after
(report the numbers); `wc -l` for every touched file in the report;
`git diff --stat` sanity (moves, not rewrites — use `git diff --color-moved`
to confirm).

## Final: whole-branch review

Dispatch the final code reviewer (most capable model) with a review package
over `3d088d1..HEAD`, the Minor-findings roll-up from the ledger, and this
plan. Fix wave (single fixer) for anything Critical/Important. Then:
gates, push `-u origin fix/m5-review-fixes`, PR to `main` titled
`fix(m5): address all M5 review findings`, body mapping every finding
(C1–C3, H1–H4, M1–M6, LOWs incl. the accepted MLG-bitfield one) to its
commit.
