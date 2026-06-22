// SPDX-License-Identifier: GPL-3.0-or-later
mod commands;
pub mod events;

use specta_typescript::Typescript;
use tauri_specta::{collect_commands, collect_events, Builder, Event as _};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = Builder::<tauri::Wry>::new()
        .commands(collect_commands![commands::app_info])
        .events(collect_events![
            events::Heartbeat,
            events::ConnectionStateEvent
        ]);

    #[cfg(debug_assertions)]
    builder
        .export(Typescript::default(), "../src/ipc/bindings.ts")
        .expect("failed to export typescript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);

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

    fn make_builder() -> Builder<tauri::Wry> {
        Builder::<tauri::Wry>::new()
            .commands(collect_commands![commands::app_info])
            .events(collect_events![
                events::Heartbeat,
                events::ConnectionStateEvent
            ])
    }

    #[test]
    fn export_typescript_bindings() {
        make_builder()
            .export(Typescript::default(), "../src/ipc/bindings.ts")
            .expect("failed to export typescript bindings");
    }

    #[test]
    fn export_typescript_bindings_includes_heartbeat() {
        make_builder()
            .export(Typescript::default(), "../src/ipc/bindings.ts")
            .expect("failed to export typescript bindings");

        let contents =
            std::fs::read_to_string("../src/ipc/bindings.ts").expect("bindings.ts must exist");
        assert!(
            contents.contains("Heartbeat"),
            "bindings.ts should contain Heartbeat type, got:\n{contents}"
        );
    }

    #[test]
    fn export_typescript_bindings_includes_connection_state_event() {
        make_builder()
            .export(Typescript::default(), "../src/ipc/bindings.ts")
            .expect("failed to export typescript bindings");

        let contents =
            std::fs::read_to_string("../src/ipc/bindings.ts").expect("bindings.ts must exist");
        assert!(
            contents.contains("ConnectionStateEvent"),
            "bindings.ts should contain ConnectionStateEvent type, got:\n{contents}"
        );
        // Verify the key variants are present so the frontend can pattern-match.
        assert!(
            contents.contains("Reconnecting"),
            "bindings.ts should contain Reconnecting variant, got:\n{contents}"
        );
    }
}
