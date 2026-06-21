// SPDX-License-Identifier: GPL-3.0-or-later
mod commands;

use specta_typescript::Typescript;
use tauri_specta::{collect_commands, Builder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = Builder::<tauri::Wry>::new()
        .commands(collect_commands![commands::app_info]);

    #[cfg(debug_assertions)]
    builder
        .export(
            Typescript::default(),
            "../src/ipc/bindings.ts",
        )
        .expect("failed to export typescript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod binding_gen {
    use super::*;
    use specta_typescript::Typescript;
    use tauri_specta::{collect_commands, Builder};

    #[test]
    fn export_typescript_bindings() {
        let builder = Builder::<tauri::Wry>::new()
            .commands(collect_commands![commands::app_info]);

        builder
            .export(
                Typescript::default(),
                "../src/ipc/bindings.ts",
            )
            .expect("failed to export typescript bindings");
    }
}
