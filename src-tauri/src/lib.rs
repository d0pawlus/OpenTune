// SPDX-License-Identifier: GPL-3.0-or-later
pub mod ai_anthropic;
pub mod ai_commands;
pub mod ai_provider;
pub mod ai_settings;
pub mod ai_tools;
mod analysis_bridge;
mod analysis_commands;
mod capture;
mod commands;
pub mod connection;
pub mod dto;
pub mod events;
mod layout;
mod log_bridge;
mod log_commands;
mod log_paths;
mod offline_commands;
pub mod owner;
mod realtime_commands;
pub mod session;
mod session_diff;
mod tune_commands;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[cfg(debug_assertions)]
use specta_typescript::Typescript;
use tauri::Manager as _;
use tauri_specta::{collect_commands, collect_events, Builder, Event as _};

/// Assemble the tauri-specta builder. Single source for the command/event
/// registration lists so `run()` and the `binding_gen` tests can never drift.
fn build_specta() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::app_info,
            commands::list_ports,
            commands::connect,
            commands::disconnect,
            commands::simulate_link_drop,
            ai_commands::get_ai_settings,
            ai_commands::set_ai_settings,
            ai_commands::set_ai_key,
            ai_commands::clear_ai_key,
            ai_commands::ai_key_present,
            tune_commands::get_definition,
            tune_commands::load_tune,
            tune_commands::get_values,
            tune_commands::resolve_gauge_bounds,
            tune_commands::set_value,
            tune_commands::set_cells,
            tune_commands::burn_tune,
            tune_commands::undo_tune,
            tune_commands::redo_tune,
            tune_commands::eval_conditions,
            tune_commands::snapshot_tune,
            tune_commands::diff_tune,
            tune_commands::merge_tune,
            offline_commands::new_tune,
            offline_commands::open_tune,
            offline_commands::save_tune,
            offline_commands::write_tune_to_ecu,
            realtime_commands::start_realtime,
            realtime_commands::stop_realtime,
            analysis_commands::start_capture,
            analysis_commands::stop_capture,
            analysis_commands::capture_status,
            analysis_commands::run_ve_analyze,
            log_commands::start_log,
            log_commands::stop_log,
            log_commands::add_log_marker,
            log_commands::log_status,
            log_commands::open_log,
            log_commands::get_log_data,
            log_commands::save_log,
            log_commands::log_stats,
            log_commands::detect_anomaly,
            log_commands::virtual_dyno,
            layout::save_layout,
            layout::load_layout,
        ])
        .events(collect_events![
            events::Heartbeat,
            events::ConnectionStateEvent,
            events::TuneDirtyEvent,
            events::RealtimeFrameEvent,
        ])
}

/// M5 review CRITICAL (C3): hard cap on the best-effort log flush run on app
/// exit. The app must never hang on close, so a stuck `StopLog` (owner
/// wedged, wire stalled) is abandoned after this long and exit proceeds
/// anyway.
const EXIT_FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

/// Pure decision for one `RunEvent::ExitRequested`: `flush_done` is the
/// "exit flush already ran" guard, read at the top of
/// [`handle_exit_requested`]. The first pass (guard unset) must be
/// deferred — `prevent_exit()`, then flush. The second, self-triggered pass
/// — from the flush task's own `app_handle.exit(0)`, by which point the
/// guard is set — must be let through unconditionally, or the app could
/// never actually close.
fn should_defer_exit(flush_done: bool) -> bool {
    !flush_done
}

/// Fold a `StopLog` reply into the exit-flush outcome. Success and "nothing
/// was recording" ([`owner::NO_ACTIVE_LOG`]) both need no report; any other
/// error is handed back for the caller to log — the flush is best-effort
/// and must never block exit.
fn exit_flush_outcome<T>(result: Result<T, String>) -> Result<(), String> {
    match result {
        Ok(_) => Ok(()),
        Err(error) if error == owner::NO_ACTIVE_LOG => Ok(()),
        Err(error) => Err(error),
    }
}

/// Handle one `RunEvent::ExitRequested`. On the first pass, defers the exit
/// and spawns a best-effort flush of any active recording — hard-capped at
/// [`EXIT_FLUSH_TIMEOUT`] so the app can never hang on close — then sets
/// `flush_done` and calls `app_handle.exit(0)`, which re-raises
/// `ExitRequested`; [`should_defer_exit`] now reads the guard as set and lets
/// that second pass straight through.
fn handle_exit_requested(
    app_handle: &tauri::AppHandle<tauri::Wry>,
    api: &tauri::ExitRequestApi,
    code: Option<i32>,
    flush_done: &Arc<AtomicBool>,
) {
    if !should_defer_exit(flush_done.load(Ordering::SeqCst)) {
        return;
    }
    api.prevent_exit();

    let owner_handle = app_handle.state::<owner::OwnerHandle>().inner().clone();
    let app_handle = app_handle.clone();
    let flush_done = Arc::clone(flush_done);
    tauri::async_runtime::spawn(async move {
        let reply = tokio::time::timeout(
            EXIT_FLUSH_TIMEOUT,
            owner::request(&owner_handle, |reply| owner::Command::StopLog { reply }),
        )
        .await;
        match reply {
            Ok(result) => {
                if let Err(error) = exit_flush_outcome(result) {
                    eprintln!("exit: log flush failed, exiting anyway: {error}");
                }
            }
            Err(_) => {
                eprintln!("exit: log flush timed out after {EXIT_FLUSH_TIMEOUT:?}, exiting anyway")
            }
        }
        flush_done.store(true, Ordering::SeqCst);
        // Preserve the exit code the original request carried (a window close
        // is `None`) rather than always forcing 0.
        app_handle.exit(code.unwrap_or(0));
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = build_specta();

    #[cfg(debug_assertions)]
    builder
        .export(Typescript::default(), "../src/ipc/bindings.ts")
        .expect("failed to export typescript bindings");

    // M5 review CRITICAL (C3): guards `handle_exit_requested` against its
    // own `app_handle.exit(0)` re-raising `ExitRequested`.
    let exit_flush_done = Arc::new(AtomicBool::new(false));

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);

            // §9: spawn the single wire-owner task; commands talk to it
            // through the managed sender.
            app.manage(owner::spawn_owner(app.handle().clone()));

            // M7 slice 2: manage the AI key store (OsKeyStore in production).
            app.manage(crate::ai_commands::AiKeyStoreState(std::sync::Arc::new(
                crate::ai_settings::OsKeyStore,
            )));

            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut seq = 0u32;
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    seq += 1;
                    let _ = events::Heartbeat { seq }.emit(&handle);
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(move |app_handle, event| {
        if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
            handle_exit_requested(app_handle, &api, code, &exit_flush_done);
        }
    });
}

#[cfg(test)]
mod exit_flush_tests {
    use super::*;

    // M5 review CRITICAL (C3): pure decision logic extracted out of
    // `handle_exit_requested` so the defer/allow behaviour is unit-testable
    // without a running Tauri app.

    #[test]
    fn first_exit_request_with_guard_unset_is_deferred() {
        assert!(should_defer_exit(false));
    }

    #[test]
    fn second_exit_request_with_guard_set_is_allowed_through() {
        assert!(!should_defer_exit(true));
    }

    #[test]
    fn exit_flush_outcome_treats_success_as_nothing_to_report() {
        assert_eq!(exit_flush_outcome(Ok(())), Ok(()));
    }

    #[test]
    fn exit_flush_outcome_treats_no_active_log_as_nothing_to_report() {
        let reply: Result<(), String> = Err(owner::NO_ACTIVE_LOG.to_string());
        assert_eq!(exit_flush_outcome(reply), Ok(()));
    }

    #[test]
    fn exit_flush_outcome_surfaces_a_real_flush_error() {
        let reply: Result<(), String> = Err("disk full".to_string());
        assert_eq!(exit_flush_outcome(reply), Err("disk full".to_string()));
    }
}

#[cfg(test)]
mod binding_gen {
    use super::*;
    use std::sync::Mutex;

    /// These tests all export to and read back the same `bindings.ts` path.
    /// Under the default parallel test runner that races (pre-existing flake);
    /// this mutex serializes them so each export→read pair is atomic. Recovers
    /// from poisoning so one panicking test does not cascade.
    static BINDINGS_LOCK: Mutex<()> = Mutex::new(());

    /// Export the bindings under the shared lock and return their contents.
    fn export_and_read() -> String {
        let _guard = BINDINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        build_specta()
            .export(Typescript::default(), "../src/ipc/bindings.ts")
            .expect("failed to export typescript bindings");
        std::fs::read_to_string("../src/ipc/bindings.ts").expect("bindings.ts must exist")
    }

    #[test]
    fn export_typescript_bindings() {
        let _ = export_and_read();
    }

    #[test]
    fn export_typescript_bindings_includes_heartbeat() {
        let contents = export_and_read();
        assert!(
            contents.contains("Heartbeat"),
            "bindings.ts should contain Heartbeat type, got:\n{contents}"
        );
    }

    #[test]
    fn export_typescript_bindings_includes_connection_state_event() {
        let contents = export_and_read();
        assert!(
            contents.contains("ConnectionStateEvent"),
            "bindings.ts should contain ConnectionStateEvent type, got:\n{contents}"
        );
        assert!(
            contents.contains("Reconnecting"),
            "bindings.ts should contain Reconnecting variant, got:\n{contents}"
        );
    }

    #[test]
    fn export_typescript_bindings_includes_list_ports_command() {
        let contents = export_and_read();
        assert!(
            contents.contains("listPorts"),
            "bindings.ts should contain listPorts command, got:\n{contents}"
        );
        assert!(
            contents.contains("PortInfoDto"),
            "bindings.ts should contain PortInfoDto type, got:\n{contents}"
        );
    }

    #[test]
    fn export_typescript_bindings_includes_connect_commands() {
        let contents = export_and_read();
        assert!(
            contents.contains("connect"),
            "bindings.ts should contain connect command"
        );
        assert!(
            contents.contains("disconnect"),
            "bindings.ts should contain disconnect command"
        );
        assert!(
            contents.contains("simulateLinkDrop"),
            "bindings.ts should contain simulateLinkDrop command"
        );
        assert!(
            contents.contains("ConnectSource"),
            "bindings.ts should contain ConnectSource type"
        );
    }

    #[test]
    fn export_typescript_bindings_includes_tune_commands_and_event() {
        let contents = export_and_read();
        for needle in [
            "getDefinition",
            "loadTune",
            "resolveGaugeBounds",
            "setValue",
            "setCells",
            "CellEditDto",
            "burnTune",
            "undoTune",
            "redoTune",
            "evalConditions",
            "TuneDirtyEvent",
            "DefinitionDto",
            "ResolvedGaugeBoundsDto",
            "MergePickDto",
            "DialogDto",
            "ConstantKindDto",
            "CurveDto",
            "AxisDto",
            "x_channel",
        ] {
            assert!(
                contents.contains(needle),
                "bindings.ts should contain `{needle}`, got:\n{contents}"
            );
        }
    }

    #[test]
    fn export_typescript_bindings_includes_diff_merge_commands() {
        let contents = export_and_read();
        for needle in [
            "snapshotTune",
            "diffTune",
            "mergeTune",
            "FieldDiffDto",
            "CellDiffDto",
        ] {
            assert!(
                contents.contains(needle),
                "bindings.ts should contain `{needle}`, got:\n{contents}"
            );
        }
    }

    #[test]
    fn export_typescript_bindings_includes_realtime_commands_and_event() {
        let contents = export_and_read();
        for needle in ["startRealtime", "stopRealtime", "RealtimeFrameEvent"] {
            assert!(
                contents.contains(needle),
                "bindings.ts should contain `{needle}`, got:\n{contents}"
            );
        }
    }

    #[test]
    fn export_typescript_bindings_includes_capture_commands_and_dto() {
        let contents = export_and_read();
        for needle in [
            "startCapture",
            "stopCapture",
            "captureStatus",
            "CaptureStatusDto",
        ] {
            assert!(
                contents.contains(needle),
                "bindings.ts should contain `{needle}`, got:\n{contents}"
            );
        }
    }

    #[test]
    fn export_typescript_bindings_includes_run_ve_analyze_command_and_dtos() {
        let contents = export_and_read();
        for needle in [
            "runVeAnalyze",
            "VeAnalysisReportDto",
            "CellResultDto",
            "FilterCountDto",
            "analyze_tables",
        ] {
            assert!(
                contents.contains(needle),
                "bindings.ts should contain `{needle}`, got:\n{contents}"
            );
        }
    }

    #[test]
    fn export_typescript_bindings_includes_layout_commands_and_gauge_dtos() {
        let contents = export_and_read();
        for needle in [
            "saveLayout",
            "loadLayout",
            "GaugeDto",
            "FrontPageDto",
            "IndicatorDto",
        ] {
            assert!(
                contents.contains(needle),
                "bindings.ts should contain `{needle}`, got:\n{contents}"
            );
        }
    }

    #[test]
    fn export_typescript_bindings_includes_m5_log_and_analysis_api() {
        let contents = export_and_read();
        for needle in [
            "startLog",
            "stopLog",
            "addLogMarker",
            "logStatus",
            "openLog",
            "getLogData",
            "saveLog",
            "logStats",
            "detectAnomaly",
            "virtualDyno",
            "LogDataDto",
            "LogStatsReportDto",
            "AnomalyReportDto",
            "VirtualDynoReportDto",
        ] {
            assert!(
                contents.contains(needle),
                "bindings.ts should contain `{needle}`, got:\n{contents}"
            );
        }
    }
}
