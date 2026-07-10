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
