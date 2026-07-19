# M6 Slice 1 — Baseline and Interoperability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore a clean frontend test exit and commit reproducible Speeduino, rusEFI, MegaSquirt, and real TunerStudio `.msq` interoperability evidence.

**Architecture:** Keep the existing INI and project parsers unchanged unless a real input exposes a concrete defect. Isolate the `DatalogPanel` unit suite from the independently tested uPlot renderer, reuse the existing `dump` and `msq_dump` examples, and record source hashes plus acceptance results in one Markdown report. No compatibility framework or new parser facade is introduced.

**Tech Stack:** Vitest, Rust workspace examples/tests, SHA-256, OpenTune desktop app, TunerStudio MS.

## Global Constraints

- Work in `.worktrees/m6-completion` on `feat/m6-completion`.
- Do not copy non-redistributable third-party INI/MSQ files into the repository.
- Do not change `src/ipc/bindings.ts` manually.
- Treat any parser change as a focused bugfix: reproduce it with a small derived fixture and test first.
- Record exact input hashes and commands; do not turn prior PR notes into unverified current results.

---

### Task 1: Eliminate the jsdom/uPlot unhandled-error baseline

**Files:**
- Modify: `src/components/datalog/DatalogPanel.test.tsx`

**Interfaces:**
- `DatalogPanel` still renders its chart boundary in production.
- The panel suite tests panel state and IPC only; `facetedPlotConfig.test.ts` remains the chart-specific unit boundary.

- [ ] **Step 1: Reproduce the failure**

Run: `npm test -- src/components/datalog/DatalogPanel.test.tsx`

Expected before the fix: assertions pass, but Vitest exits non-zero with deferred uPlot `clearRect` errors from jsdom's missing canvas implementation.

- [ ] **Step 2: Add the smallest renderer-boundary mock**

Add this hoisted mock beside the existing module mocks:

```tsx
vi.mock("./DatalogCharts", () => ({
  DatalogCharts: () => <div data-testid="datalog-charts" />,
  DynoChart: () => <div data-testid="dyno-chart" />,
}));
```

Do not install a canvas implementation and do not weaken Vitest's unhandled-error behavior.

- [ ] **Step 3: Verify the focused and full frontend suites**

Run:

```bash
npm test -- src/components/datalog/DatalogPanel.test.tsx
npm test
```

Expected: both commands exit 0; the full run reports no unhandled errors.

- [ ] **Step 4: Commit the baseline fix**

```bash
git add src/components/datalog/DatalogPanel.test.tsx
git commit -m "test(datalog): isolate panel tests from uPlot canvas"
```

### Task 2: Produce current compatibility diagnostics

**Files:**
- Create: `docs/compatibility/m6.md`

**Interfaces:**
- Consumes the existing examples:
  - `cargo run -p opentune-ini --example dump -- <ini> [symbols]`
  - `cargo run -p opentune-project --example msq_dump -- <ini> <msq> [symbols]`
- Produces one report row each for Speeduino, rusEFI, and MegaSquirt MS3.

- [ ] **Step 1: Select and hash the tested inputs**

Use:

- Speeduino: `src-tauri/crates/ini/tests/fixtures/speeduino-real-0832dc1d.ini`.
- rusEFI: `~/TunerStudioProjects/MazdaMiata_DominikP_RusEfi_ZgrywusK8/projectCfg/mainController.ini` plus its `CurrentTune.msq`.
- MegaSquirt: `~/TunerStudioProjects/MS3-Example_Project/projectCfg/mainController.ini` plus its `CurrentTune.msq`.

Run `shasum -a 256` for every selected file and capture the hashes without modifying the source projects.

- [ ] **Step 2: Run the INI and MSQ diagnostics**

Run `dump` for all three definitions and `msq_dump` for the rusEFI and MS3 pairs. Supply the comma-separated `PcVariables` from each project's `project.properties` when the project uses conditional symbols. Save stdout in a temporary directory, not the repository.

Expected gate: every definition parses; each real `.msq` has `failed: 0`. Record applied, skipped, clamped, failed, definition counts, project origin/version, file hashes, and the literal reproduction commands.

- [ ] **Step 3: Verify the existing format round-trip regression**

Run:

```bash
cargo test -p opentune-project --test msq scalar_array_bits_text_round_trip \
  --manifest-path src-tauri/Cargo.toml -- --exact
```

Expected: the scalar, array/table, bits, and text serialization test passes.

- [ ] **Step 4: Write the compatibility report**

Create `docs/compatibility/m6.md` with:

- scope and limitations;
- one table row per firmware family;
- exact hashes and result counts;
- a reproduction section with copy-paste commands;
- a separate TunerStudio acceptance section initially marked as not yet executed;
- explicit non-claims for Honda OBD1, MS4x, and physical ECU hardware.

### Task 3: Complete the real TunerStudio round trip

**Files:**
- Modify: `docs/compatibility/m6.md`
- Temporary only: `/private/tmp/opentune-m6-ms3`

**Interfaces:**
- OpenTune consumes the copied MS3 project and writes a new `.msq`.
- TunerStudio reopens that output and displays the deliberately changed scalar, bits option, and table cell.

- [ ] **Step 1: Protect the source project**

Copy `~/TunerStudioProjects/MS3-Example_Project` to `/private/tmp/opentune-m6-ms3`. All GUI edits must target only the copy.

- [ ] **Step 2: Establish three observable source values**

Open the copied project in TunerStudio and note one scalar, one bits/choice setting, and one table cell that OpenTune exposes. Record their names and original physical values.

- [ ] **Step 3: Edit and save through OpenTune**

Launch `npm run tauri dev`, open the copied project, change only the three recorded values, and save to `/private/tmp/opentune-m6-ms3/OpenTune-M6.msq`. Reopen the output in OpenTune and verify the same values.

- [ ] **Step 4: Reopen through TunerStudio**

Open `OpenTune-M6.msq` in the copied TunerStudio project. Verify the three changed values are present and that TunerStudio reports no file-format error. Save once from TunerStudio as `TunerStudio-M6-resaved.msq`, then load that file in OpenTune.

- [ ] **Step 5: Record acceptance evidence and commit**

Update the report with date, app versions, named values before/after, both generated-file hashes, and PASS/FAIL for both directions.

```bash
git add docs/compatibility/m6.md
git commit -m "docs(m6): record firmware and msq compatibility"
```

### Task 4: Slice verification

- [ ] **Step 1: Run all relevant suites**

```bash
npm test
npm run rust:test
git status --short
```

Expected: both suites exit 0 and only intentional later-slice changes, if any, remain uncommitted.
