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
    log_id: u32,
    offset: u32,
    limit: u32,
    owner: State<'_, OwnerHandle>,
) -> Result<LogDataDto, String> {
    request(&owner, |reply| Command::GetLogData {
        log_id,
        offset,
        limit,
        reply,
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn save_log(
    log_id: u32,
    path: String,
    format: LogFormatDto,
    owner: State<'_, OwnerHandle>,
) -> Result<(), String> {
    request(&owner, |reply| Command::SaveLog {
        log_id,
        path,
        format,
        reply,
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn log_stats(
    log_id: u32,
    params: LogStatsParamsDto,
    owner: State<'_, OwnerHandle>,
) -> Result<LogStatsReportDto, String> {
    request(&owner, |reply| Command::LogStats {
        log_id,
        params,
        reply,
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn detect_anomaly(
    log_id: u32,
    thresholds: AnomalyThresholdsDto,
    owner: State<'_, OwnerHandle>,
) -> Result<AnomalyReportDto, String> {
    request(&owner, |reply| Command::DetectAnomaly {
        log_id,
        thresholds,
        reply,
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn virtual_dyno(
    log_id: u32,
    params: VirtualDynoParamsDto,
    owner: State<'_, OwnerHandle>,
) -> Result<VirtualDynoReportDto, String> {
    request(&owner, |reply| Command::VirtualDyno {
        log_id,
        params,
        reply,
    })
    .await
}
