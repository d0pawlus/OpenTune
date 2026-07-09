// SPDX-License-Identifier: GPL-3.0-or-later
mod analysis_commands;
mod capture;
mod commands;
pub mod connection;
pub mod dto;
pub mod events;
mod layout;
pub mod owner;
mod realtime_commands;
pub mod session;
mod session_diff;
mod tune_commands;

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
            tune_commands::get_definition,
            tune_commands::load_tune,
            tune_commands::get_values,
            tune_commands::set_value,
            tune_commands::set_cells,
            tune_commands::burn_tune,
            tune_commands::undo_tune,
            tune_commands::redo_tune,
            tune_commands::eval_conditions,
            tune_commands::snapshot_tune,
            tune_commands::diff_tune,
            tune_commands::merge_tune,
            realtime_commands::start_realtime,
            realtime_commands::stop_realtime,
            analysis_commands::start_capture,
            analysis_commands::stop_capture,
            analysis_commands::capture_status,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = build_specta();

    #[cfg(debug_assertions)]
    builder
        .export(Typescript::default(), "../src/ipc/bindings.ts")
        .expect("failed to export typescript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);

            // §9: spawn the single wire-owner task; commands talk to it
            // through the managed sender.
            app.manage(owner::spawn_owner(app.handle().clone()));

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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
            "setValue",
            "setCells",
            "CellEditDto",
            "burnTune",
            "undoTune",
            "redoTune",
            "evalConditions",
            "TuneDirtyEvent",
            "DefinitionDto",
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
}
