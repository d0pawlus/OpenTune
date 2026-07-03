// SPDX-License-Identifier: GPL-3.0-or-later
//! App + connection IPC commands.
//!
//! The connection commands are thin async senders into the §9 owner task
//! (see [`crate::owner`]): each builds a oneshot reply channel, sends the
//! matching [`Command`], and awaits the result. No command touches the
//! transport or holds any state — the owner emits every event.

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::connection::ConnectSource;
use crate::owner::{request, Command, OwnerHandle};

// ── App info / port enumeration (no session involved) ────────────────────────

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

// ── M1 connection commands (thin senders into the owner task) ────────────────

/// Connect to an ECU (simulator or serial).
///
/// The owner emits `ConnectionStateEvent` transitions over IPC as the
/// connection progresses. Resolves `Ok(())` once the handshake succeeds.
#[tauri::command]
#[specta::specta]
pub async fn connect(source: ConnectSource, owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::Connect { source, reply }).await
}

/// Disconnect from the ECU; the owner emits `Disconnected`.
#[tauri::command]
#[specta::specta]
pub async fn disconnect(owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::Disconnect { reply }).await
}

/// Simulator-only: drop the link and drive the reconnect loop. The owner
/// emits `Reconnecting{attempt}` states until `Connected` or `Failed`, and
/// re-reads the tune when the reconnect detected an ECU reboot.
#[tauri::command]
#[specta::specta]
pub async fn simulate_link_drop(owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::SimulateLinkDrop { reply }).await
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
