// SPDX-License-Identifier: GPL-3.0-or-later
//! M3 realtime commands — thin async senders into the §9 owner task
//! (Task 1 pattern, mirroring `tune_commands.rs`).
//!
//! Realtime is **explicit-start only**: the owner never polls until the
//! frontend invokes `start_realtime`, and `stop_realtime`/disconnect always
//! stop it. While polling, the owner emits
//! [`RealtimeFrameEvent`](crate::events::RealtimeFrameEvent) at ≤30 Hz.

use tauri::State;

use crate::owner::{request, Command, OwnerHandle};

/// Start the 25 Hz realtime poll loop (frames are emitted coalesced to
/// ≤30 Hz as `RealtimeFrameEvent`s).
#[tauri::command]
#[specta::specta]
pub async fn start_realtime(owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::StartRealtime { reply }).await
}

/// Stop the realtime poll loop.
#[tauri::command]
#[specta::specta]
pub async fn stop_realtime(owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::StopRealtime { reply }).await
}
