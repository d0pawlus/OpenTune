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

/// Port information for the frontend UI.
#[derive(Serialize, Type, Clone, Debug, PartialEq)]
pub struct PortInfoDto {
    pub name: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub product: Option<String>,
}

/// Enumerate available serial ports (does not connect).
#[tauri::command]
#[specta::specta]
pub fn list_ports() -> Result<Vec<PortInfoDto>, String> {
    opentune_transport::enumerate_ports()
        .map(|ports| {
            ports
                .into_iter()
                .map(|p| PortInfoDto {
                    name: p.name,
                    vid: p.vid,
                    pid: p.pid,
                    product: p.product,
                })
                .collect()
        })
        .map_err(|e| e.to_string())
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

    #[test]
    fn list_ports_returns_ports() {
        // M1: this is a thin IPC wrapper around the transport layer.
        // The transport layer's enumerate_ports is tested in its unit tests.
        // Here we just verify the DTO conversion doesn't panic.
        let result = list_ports();
        // May return Ok([]) or Ok([ports]) depending on hardware; never panic.
        assert!(result.is_ok());
    }
}
