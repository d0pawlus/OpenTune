// SPDX-License-Identifier: GPL-3.0-or-later
//! M5 datalog IPC. Hardware/session state remains on the owner; disk and
//! analysis work is dispatched to the blocking pool by owner handlers.

use tauri::State;

use crate::dto::{
    AnomalyReportDto, AnomalyThresholdsDto, LogDataDto, LogFormatDto, LogStatsParamsDto,
    LogStatsReportDto, LogStatusDto, LogSummaryDto, VirtualDynoParamsDto, VirtualDynoReportDto,
};
use crate::owner::{request, Command, OwnerHandle};

#[tauri::command]
#[specta::specta]
pub async fn start_log(
    path: String,
    format: LogFormatDto,
    owner: State<'_, OwnerHandle>,
) -> Result<LogStatusDto, String> {
    request(&owner, |reply| Command::StartLog {
        path,
        format,
        reply,
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn stop_log(owner: State<'_, OwnerHandle>) -> Result<LogSummaryDto, String> {
    request(&owner, |reply| Command::StopLog { reply }).await
}

#[tauri::command]
#[specta::specta]
pub async fn add_log_marker(text: String, owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::AddLogMarker { text, reply }).await
}

#[tauri::command]
#[specta::specta]
pub async fn log_status(owner: State<'_, OwnerHandle>) -> Result<LogStatusDto, String> {
    request(&owner, |reply| Command::LogStatus { reply }).await
}

#[tauri::command]
#[specta::specta]
pub async fn open_log(
    path: String,
    format: LogFormatDto,
    owner: State<'_, OwnerHandle>,
) -> Result<LogSummaryDto, String> {
    request(&owner, |reply| Command::OpenLog {
        path,
        format,
        reply,
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn get_log_data(
    offset: u32,
    limit: u32,
    owner: State<'_, OwnerHandle>,
) -> Result<LogDataDto, String> {
    request(&owner, |reply| Command::GetLogData {
        offset,
        limit,
        reply,
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn save_log(
    path: String,
    format: LogFormatDto,
    owner: State<'_, OwnerHandle>,
) -> Result<(), String> {
    request(&owner, |reply| Command::SaveLog {
        path,
        format,
        reply,
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn log_stats(
    params: LogStatsParamsDto,
    owner: State<'_, OwnerHandle>,
) -> Result<LogStatsReportDto, String> {
    request(&owner, |reply| Command::LogStats { params, reply }).await
}

#[tauri::command]
#[specta::specta]
pub async fn detect_anomaly(
    thresholds: AnomalyThresholdsDto,
    owner: State<'_, OwnerHandle>,
) -> Result<AnomalyReportDto, String> {
    request(&owner, |reply| Command::DetectAnomaly { thresholds, reply }).await
}

#[tauri::command]
#[specta::specta]
pub async fn virtual_dyno(
    params: VirtualDynoParamsDto,
    owner: State<'_, OwnerHandle>,
) -> Result<VirtualDynoReportDto, String> {
    request(&owner, |reply| Command::VirtualDyno { params, reply }).await
}
