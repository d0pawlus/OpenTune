// SPDX-License-Identifier: GPL-3.0-or-later
use serde::Serialize;
use specta::Type;
use tauri_specta::Event as _;

use crate::connection::{
    connect_serial, connect_simulator, load_comms_from_path, load_comms_from_str,
    simulate_link_drop_async, ActiveConnection, ConnectSource, ConnectionStore,
};
use crate::events::ConnectionStateEvent;

// ── Bundled INI (plain protocol, matches EcuSimulator::new()) ─────────────────

const BUNDLED_INI: &str = include_str!("../resources/speeduino.sample.ini");

// ── Existing commands ─────────────────────────────────────────────────────────

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

// ── M1 connection commands ────────────────────────────────────────────────────

/// Connect to an ECU (simulator or serial).
///
/// Emits `ConnectionStateEvent` transitions over IPC as the connection
/// progresses. Returns `Ok(())` once the initial handshake succeeds.
#[tauri::command]
#[specta::specta]
pub fn connect(
    source: ConnectSource,
    state: tauri::State<'_, ConnectionStore>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    // Drop any existing connection before opening a new one.
    {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        *guard = None;
    }

    let emit_app = app.clone();
    let emit = move |cs: opentune_protocol::ConnectionState| {
        let _ = ConnectionStateEvent::from(cs).emit(&emit_app);
    };

    let active = match source {
        ConnectSource::Simulator { ini_path } => {
            let comms = match ini_path {
                Some(ref path) => load_comms_from_path(path)?,
                None => load_comms_from_str(BUNDLED_INI)?,
            };
            connect_simulator(comms, &emit)?
        }
        ConnectSource::Serial {
            ref port_name,
            ref ini_path,
        } => {
            let comms = load_comms_from_path(ini_path)?;
            connect_serial(port_name.clone(), comms, &emit)?
        }
    };

    let mut guard = state.lock().map_err(|e| e.to_string())?;
    *guard = Some(active);
    Ok(())
}

/// Disconnect from the ECU and emit `Disconnected`.
#[tauri::command]
#[specta::specta]
pub fn disconnect(
    state: tauri::State<'_, ConnectionStore>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    *guard = None;
    let _ = ConnectionStateEvent::Disconnected.emit(&app);
    Ok(())
}

/// Simulator-only: drop the link and drive the reconnect loop.
///
/// Returns immediately; the reconnect runs on a background thread, emitting
/// `Reconnecting{attempt}` states until `Connected` or `Failed`.
#[tauri::command]
#[specta::specta]
pub fn simulate_link_drop(
    state: tauri::State<'_, ConnectionStore>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let active = {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        guard.take()
    };
    let active = active.ok_or_else(|| "not connected".to_string())?;

    match &active {
        ActiveConnection::Serial { .. } => {
            return Err("simulate_link_drop is only available in simulator mode".to_string());
        }
        ActiveConnection::Sim { .. } => {}
    }

    let store = std::sync::Arc::clone(&state);
    let emit = move |cs: opentune_protocol::ConnectionState| {
        let _ = ConnectionStateEvent::from(cs).emit(&app);
    };
    simulate_link_drop_async(active, store, emit);
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

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
        let result = list_ports();
        assert!(result.is_ok());
    }
}
