// SPDX-License-Identifier: GPL-3.0-or-later
use serde::Serialize;
use specta::Type;

#[derive(Serialize, Type, Clone, Debug, PartialEq)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

pub fn app_info_impl() -> AppInfo {
    AppInfo {
        name: "OpenTune".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

#[tauri::command]
#[specta::specta]
pub fn app_info() -> AppInfo {
    app_info_impl()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_reports_name_and_nonempty_version() {
        let info = app_info_impl();
        assert_eq!(info.name, "OpenTune");
        assert!(!info.version.is_empty());
    }
}
