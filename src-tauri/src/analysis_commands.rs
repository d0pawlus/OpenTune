// SPDX-License-Identifier: GPL-3.0-or-later
//! M4 Task 8 capture commands — thin async senders into the §9 owner task
//! (mirrors `tune_commands.rs`'s shape).
//!
//! `start_capture`/`stop_capture`/`capture_status` control the owner-side
//! realtime capture ring (`crate::capture::CaptureBuffer`) that feeds the
//! M4 Task 11 VE analyzer. Raw rows never cross IPC — only the status.

use tauri::State;

use crate::dto::CaptureStatusDto;
use crate::owner::{request, Command, OwnerHandle};

/// Start (or restart) the realtime capture ring for the current session.
/// Requires an active session — the ring's pinned columns come from the
/// session's declared output channels.
#[tauri::command]
#[specta::specta]
pub async fn start_capture(owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::StartCapture { reply }).await
}

/// Stop capturing (rows are retained for `run_ve_analyze`) and return the
/// final status.
#[tauri::command]
#[specta::specta]
pub async fn stop_capture(owner: State<'_, OwnerHandle>) -> Result<CaptureStatusDto, String> {
    request(&owner, |reply| Command::StopCapture { reply }).await
}

/// Report the capture ring's current status.
#[tauri::command]
#[specta::specta]
pub async fn capture_status(owner: State<'_, OwnerHandle>) -> Result<CaptureStatusDto, String> {
    request(&owner, |reply| Command::CaptureStatus { reply }).await
}
