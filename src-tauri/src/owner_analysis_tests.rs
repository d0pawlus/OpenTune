// SPDX-License-Identifier: GPL-3.0-or-later
//! Owner-level tests for `Command::RunVeAnalyze` (M4 Task 11) — split from
//! `owner_tests.rs` for file-size cohesion (that file was already at the
//! project's soft line cap). Mirrors its helper shapes; each helper is
//! duplicated locally rather than shared, since Rust module privacy doesn't
//! let sibling `#[path]` test modules reach into each other's private items.
//!
//! The bridge's own resolution-rule/report-shape coverage lives in
//! `analysis_bridge.rs`'s unit tests; these tests only prove the owner arm's
//! wiring (connection/capture/tune preconditions + the full command → bridge
//! → DTO round trip over the async channel).

use super::*;

/// Spawn an owner whose events land in a shared Vec (mirrors `owner_tests`).
fn test_owner() -> OwnerHandle {
    let emit: Emitter = Arc::new(|_ev| {});
    spawn_owner_with_emitter(emit)
}

/// Send one command and await its oneshot reply (the production `request` path).
async fn send<T>(tx: &OwnerHandle, make: impl FnOnce(Reply<T>) -> Command) -> Result<T, String> {
    request(tx, make).await
}

/// Connect to the bundled-INI simulator (optionally loading the tune too).
async fn connect(tx: &OwnerHandle, load_tune: bool) {
    send(tx, |reply| Command::Connect {
        source: ConnectSource::Simulator { ini_path: None },
        reply,
    })
    .await
    .expect("simulator connects");
    if load_tune {
        send(tx, |reply| Command::LoadTune { reply })
            .await
            .expect("tune loads");
    }
}

/// Write one array constant via `SetValue` (owner-level equivalent of the
/// bridge test's direct `tune.set` seeding).
async fn set_array(tx: &OwnerHandle, name: &str, values: Vec<f64>) {
    send(tx, |reply| Command::SetValue {
        name: name.to_string(),
        value: Value::Array(values),
        reply,
    })
    .await
    .unwrap_or_else(|e| panic!("seeding `{name}` must succeed: {e}"));
}

/// Poll `CaptureStatus` (deterministic wait, ~2 s cap) until `sample_count`
/// reaches `min` (mirrors `owner_tests::wait_for_sample_count`).
async fn wait_for_sample_count(tx: &OwnerHandle, min: u32) {
    for _ in 0..200 {
        let status = send(tx, |reply| Command::CaptureStatus { reply })
            .await
            .expect("capture_status");
        if status.sample_count >= min {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("sample_count never reached {min} within 2 s");
}

#[tokio::test]
async fn run_ve_analyze_errs_without_connection() {
    let tx = test_owner();
    let err = send(&tx, |reply| Command::RunVeAnalyze {
        table: "veTable1Tbl".to_string(),
        reply,
    })
    .await
    .expect_err("no session yet");
    assert!(err.contains("not connected"), "got: {err}");
}

#[tokio::test]
async fn run_ve_analyze_errs_without_a_tune_loaded() {
    let tx = test_owner();
    connect(&tx, false).await;
    // Capture doesn't require a loaded tune — only a session.
    send(&tx, |reply| Command::StartCapture { reply })
        .await
        .expect("start capture without a loaded tune");

    let err = send(&tx, |reply| Command::RunVeAnalyze {
        table: "veTable1Tbl".to_string(),
        reply,
    })
    .await
    .expect_err("tune never loaded");
    assert!(err.contains("no tune loaded"), "got: {err}");
}

#[tokio::test]
async fn run_ve_analyze_errs_without_a_capture() {
    let tx = test_owner();
    connect(&tx, true).await;

    let err = send(&tx, |reply| Command::RunVeAnalyze {
        table: "veTable1Tbl".to_string(),
        reply,
    })
    .await
    .expect_err("capture never started");
    assert!(err.contains("no capture"), "got: {err}");
}

#[tokio::test]
async fn run_ve_analyze_errs_on_an_unknown_table_id() {
    let tx = test_owner();
    connect(&tx, true).await;
    send(&tx, |reply| Command::StartCapture { reply })
        .await
        .expect("start capture");

    let err = send(&tx, |reply| Command::RunVeAnalyze {
        table: "notATable".to_string(),
        reply,
    })
    .await
    .expect_err("no [VeAnalyze] map for an undeclared table");
    assert!(err.contains("no [VeAnalyze] map"), "got: {err}");
}

/// The full command → bridge → DTO round trip over the async channel,
/// against the real bundled INI: seed the VE/AFR-target tables and bins
/// (never written otherwise — see `analysis_bridge.rs`'s target-grid
/// provenance doc), capture a few live poll ticks, then analyze. Only the
/// wiring is asserted here (table echo + shape); the engine's numeric
/// behavior is the bridge unit tests' job.
#[tokio::test]
async fn run_ve_analyze_wires_through_the_owner_to_a_report() {
    let tx = test_owner();
    connect(&tx, true).await;

    let rpm_bins: Vec<f64> = (0..16).map(|i| 500.0 + i as f64 * 500.0).collect();
    let fuel_load_bins: Vec<f64> = (0..16).map(|i| 20.0 + i as f64 * 5.0).collect();
    set_array(&tx, "rpmBins", rpm_bins.clone()).await;
    set_array(&tx, "fuelLoadBins", fuel_load_bins.clone()).await;
    set_array(&tx, "veTable", vec![50.0; 256]).await;
    set_array(&tx, "rpmBinsAFR", rpm_bins).await;
    set_array(&tx, "loadBinsAFR", fuel_load_bins).await;
    set_array(&tx, "afrTable", vec![14.7; 256]).await;

    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start realtime");
    send(&tx, |reply| Command::StartCapture { reply })
        .await
        .expect("start capture");
    wait_for_sample_count(&tx, 10).await;
    send(&tx, |reply| Command::StopCapture { reply })
        .await
        .expect("stop capture");

    let report = send(&tx, |reply| Command::RunVeAnalyze {
        table: "veTable1Tbl".to_string(),
        reply,
    })
    .await
    .expect("veTable1Tbl has a [VeAnalyze] map");

    assert_eq!(report.table, "veTable1Tbl");
    assert_eq!(report.x_len, 16);
    assert_eq!(report.y_len, 16);
    assert_eq!(report.cells.len(), 256);
    assert!(report.total_samples >= 10);

    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("stop realtime");
}

// ── Task 12: the M4 demo — capture, analyze, apply, error flattens ─────────
//
// The live simulator behind the owner's session (mirrors `owner_tests`'s
// `simulator` helper — duplicated per this file's module-privacy note above).
async fn simulator(tx: &OwnerHandle) -> Arc<opentune_simulator::EcuSimulator> {
    send(tx, |reply| Command::DebugSimulator { reply })
        .await
        .expect("simulator connection present")
}

/// Drive the sim's engine `windows` steps of 50 ms simulated time each, with
/// a real 40 ms sleep between so the owner's realtime poller (25 Hz,
/// `POLL_INTERVAL`) gets its own wall-clock cadence to acquire and capture
/// frames. `tick_engine` is the sim's own clock for engine time — calling it
/// even once permanently disables the sim's wall-clock auto-tick (see its
/// doc comment), so the engine only ever advances exactly as far as this
/// loop drives it; the wall-clock sleep only paces the owner's async poller,
/// never the engine.
async fn drive_engine(sim: &opentune_simulator::EcuSimulator, windows: u32) {
    for _ in 0..windows {
        sim.tick_engine(std::time::Duration::from_millis(50));
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    }
}

/// Windows per capture pass (50 ms simulated engine time each): 210 * 50 ms
/// = 10.5 s of cumulative engine time, comfortably past the ~8 s Task 9 found
/// necessary to clear STARTUP/WARMUP_IDLE into a real running-load operating
/// point (`sim_measured_afr_reflects_ve_error`,
/// `crates/simulator/tests/realtime.rs`) — see the m4-decisions entry for the
/// empirical margin.
///
/// **Raised from 200 → 210 as a review fast-follow (Task 12 review MEDIUM):**
/// this is NOT simply "deeper capture = more confidence, so keep raising it."
/// Measured: past ~245-250 windows the engine's Idle-mode state machine
/// (`EngineMode::Idle`'s `STATE_TRANSITION_MS = 5_000 ms` roll,
/// `crates/simulator/src/engine/physics.rs`) has enough deterministic
/// sim-time to fire its first random transition roll, pulling the trajectory
/// into a fresh, thinly-sampled cell — confirmed by a jump in
/// `report1`'s confident-cell set (a new index appears) and the ratio
/// spiking well past the gate (measured ~0.50-0.59 at 250/260/270 windows,
/// test-failing). 210 sits inside the pre-transition Idle plateau with a
/// ~2 s buffer before that cliff — see the m4-decisions entry for the full
/// sweep.
const CAPTURE_WINDOWS: u32 = 210;

/// Mean `|delta_pct|` over cells confident enough to matter (mirrors the
/// brief's `mean_abs`) — 0.0 when no cell clears the confidence bar (an
/// already-flat table, or a report with no usable samples).
fn mean_abs_confident_delta(report: &VeAnalysisReportDto) -> f64 {
    let confident: Vec<f64> = report
        .cells
        .iter()
        .filter(|c| c.confidence >= 0.3)
        .map(|c| c.delta_pct.abs())
        .collect();
    if confident.is_empty() {
        0.0
    } else {
        confident.iter().sum::<f64>() / confident.len() as f64
    }
}

/// **The M4 demo.** Connect to the simulator, seed a deliberately-wrong flat
/// VE table against the sim's hidden `true_ve` surface (Task 9), capture a
/// live session, analyze, apply the confident corrections via `SetCells`,
/// reconnect and retrace the identical engine trajectory, re-analyze, and
/// prove the seeded error got flatter — the exact loop the AutoTune UI
/// (Task 11) drives by hand.
///
/// Real wall-clock time, not `tokio::time::pause` (no `test-util` feature on
/// this crate's dev-deps — every timing test in this suite already runs on
/// the real clock): two `drive_engine` passes of `CAPTURE_WINDOWS` each, ~40
/// ms real sleep per window, so this test runs ~20 real seconds — longer
/// than the M3 E2E's few seconds, because Task 9's running-load operating
/// point needs several real seconds of simulated engine time to reach, not
/// just a couple of poll ticks. Kept in the normal suite regardless, same as
/// the other real-clock owner E2E tests.
#[tokio::test]
async fn ve_analyze_flattens_the_sim_ve_error() {
    let tx = test_owner();

    // Phase 0 — connect + seed a deliberately-wrong (flat) VE table against
    // the bundled INI's `veTable1Tbl`/`afrTable1Tbl` [VeAnalyze] binding. The
    // sim's hidden `true_ve(rpm, load) = 40 + 25*(load/100) + 15*(rpm/6000)`
    // (clamped 20..110, `crates/simulator/src/ve_model.rs`) disagrees with
    // this flat 50 almost everywhere — the "wrong" table the demo corrects.
    connect(&tx, true).await;

    let rpm_bins: Vec<f64> = (0..16).map(|i| 500.0 + i as f64 * 500.0).collect();
    let fuel_load_bins: Vec<f64> = (0..16).map(|i| 20.0 + i as f64 * 5.0).collect();
    set_array(&tx, "rpmBins", rpm_bins.clone()).await;
    set_array(&tx, "fuelLoadBins", fuel_load_bins.clone()).await;
    set_array(&tx, "veTable", vec![50.0; 256]).await;
    set_array(&tx, "afrTable", vec![14.7; 256]).await;
    set_array(&tx, "rpmBinsAFR", rpm_bins.clone()).await;
    set_array(&tx, "loadBinsAFR", fuel_load_bins.clone()).await;
    send(&tx, |reply| Command::Burn { reply })
        .await
        .expect("burn the seeded tune");

    // Phase 1 — capture a live session: the engine runs from cold START
    // through STARTUP/WARMUP_IDLE into a real operating point while the
    // owner's realtime poller decodes frames into the capture ring (Task 8).
    let sim = simulator(&tx).await;
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start realtime");
    send(&tx, |reply| Command::StartCapture { reply })
        .await
        .expect("start capture");
    drive_engine(&sim, CAPTURE_WINDOWS).await;
    let stopped1 = send(&tx, |reply| Command::StopCapture { reply })
        .await
        .expect("stop capture");
    assert!(
        stopped1.sample_count >= 80,
        "expected >= 80 captured rows, got {}",
        stopped1.sample_count
    );

    // Phase 2 — analyze: the flat-50 table must read lean wherever the true
    // surface exceeds it (and rich where it doesn't), for every cell the
    // engine actually visited with enough weight to matter.
    let report1 = send(&tx, |reply| Command::RunVeAnalyze {
        table: "veTable1Tbl".to_string(),
        reply,
    })
    .await
    .expect("veTable1Tbl has a [VeAnalyze] map");
    assert!(
        report1.used_samples > 0,
        "analysis must use captured samples"
    );

    let confident_count = report1.cells.iter().filter(|c| c.confidence >= 0.3).count();
    assert!(
        confident_count > 0,
        "expected at least one confident cell after phase 1 capture"
    );
    for (i, cell) in report1.cells.iter().enumerate() {
        if cell.confidence < 0.3 {
            continue;
        }
        let x = i % 16;
        let y = i / 16;
        let want = (40.0 + 25.0 * (fuel_load_bins[y] / 100.0) + 15.0 * (rpm_bins[x] / 6000.0))
            .clamp(20.0, 110.0);
        if (want - 50.0).abs() > 1.0 {
            assert_eq!(
                cell.delta_pct > 0.0,
                want > 50.0,
                "correction must point toward the sim's true-VE surface at cell {i}"
            );
        }
    }

    // Phase 3 — apply the confident corrections, re-measure, and prove the
    // seeded error got flatter.
    let edits: Vec<CellEditDto> = report1
        .cells
        .iter()
        .enumerate()
        .filter(|(_, c)| c.confidence >= 0.3 && c.proposed != c.current)
        .map(|(i, c)| CellEditDto {
            index: i as u32,
            value: c.proposed,
        })
        .collect();
    assert!(
        !edits.is_empty(),
        "expected at least one cell edit to apply"
    );
    // Re-measuring means reproducing a comparable operating-point spread —
    // continuing to drive the SAME running simulator forward instead walks
    // the trajectory into brand-new, never-corrected cells (confirmed
    // empirically: a second `drive_engine` pass here made `report2` barely
    // overlap `report1`'s touched cells, and the seeded error looked WORSE,
    // not flatter). Freezing the operating point (one small tick, then just
    // re-sampling it) fixes the overlap but narrows `report2` down to
    // whichever single cell the trajectory happened to end on — fragile,
    // since a moderate-confidence cell's first-pass correction is only
    // partial by design (`cell_change_resistance` damps every cell's delta
    // by 20%, even at full confidence — see `ve_analyze.rs::finalize`).
    // Reconnecting instead spawns a FRESH simulator (a brand-new `SimEngine`,
    // fixed-seed RNG restarted from the same cold-start state — `reboot()`
    // only resets ECU memory, never engine physics, per its own doc
    // comment) and re-seeding + re-applying the SAME `edits` retraces the
    // *identical* deterministic engine trajectory against the corrected
    // table: the same cells get hit with comparable weight/confidence, so
    // `report2` is a fair apples-to-apples measurement of the same spread
    // `report1` analyzed.
    connect(&tx, true).await;
    set_array(&tx, "rpmBins", rpm_bins.clone()).await;
    set_array(&tx, "fuelLoadBins", fuel_load_bins.clone()).await;
    set_array(&tx, "veTable", vec![50.0; 256]).await;
    set_array(&tx, "afrTable", vec![14.7; 256]).await;
    set_array(&tx, "rpmBinsAFR", rpm_bins.clone()).await;
    set_array(&tx, "loadBinsAFR", fuel_load_bins.clone()).await;
    send(&tx, |reply| Command::SetCells {
        name: "veTable".to_string(),
        cells: edits,
        reply,
    })
    .await
    .expect("apply the corrections");
    send(&tx, |reply| Command::Burn { reply })
        .await
        .expect("burn the corrected tune");

    let sim2 = simulator(&tx).await;
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("start realtime after reconnect");
    send(&tx, |reply| Command::StartCapture { reply })
        .await
        .expect("start a fresh capture");
    drive_engine(&sim2, CAPTURE_WINDOWS).await;
    let stopped2 = send(&tx, |reply| Command::StopCapture { reply })
        .await
        .expect("stop capture");
    assert!(
        stopped2.sample_count >= 80,
        "expected >= 80 re-captured rows, got {}",
        stopped2.sample_count
    );

    let report2 = send(&tx, |reply| Command::RunVeAnalyze {
        table: "veTable1Tbl".to_string(),
        reply,
    })
    .await
    .expect("veTable1Tbl has a [VeAnalyze] map");

    let mean1 = mean_abs_confident_delta(&report1);
    let mean2 = mean_abs_confident_delta(&report2);
    assert!(
        mean2 < 0.5 * mean1,
        "applying the analysis must flatten the seeded VE error: {mean1} -> {mean2}"
    );

    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("stop realtime");
}
