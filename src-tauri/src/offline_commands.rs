// SPDX-License-Identifier: GPL-3.0-or-later
//! Offline tuning: create / open / save a tune with no live ECU link.
use tauri::State;

use crate::dto::DefinitionDto;
use crate::owner::{request, Command, OwnerHandle};

/// Start a fresh offline session with a blank tune built from the INI at
/// `ini_path` (no ECU link). Returns the parsed definition for the frontend
/// to render against. Replaces any current session only if the INI parses.
#[tauri::command]
#[specta::specta]
pub async fn new_tune(
    ini_path: String,
    owner: State<'_, OwnerHandle>,
) -> Result<DefinitionDto, String> {
    request(&owner, |reply| Command::NewTune { ini_path, reply }).await
}

/// Open a `.msq` tune file offline: build a session from `ini_path`, then
/// load `msq_path` into it (signature-checked). Returns the parsed
/// definition. Replaces any current session only if the INI and `.msq` load.
#[tauri::command]
#[specta::specta]
pub async fn open_tune(
    ini_path: String,
    msq_path: String,
    owner: State<'_, OwnerHandle>,
) -> Result<DefinitionDto, String> {
    request(&owner, |reply| Command::OpenTune {
        ini_path,
        msq_path,
        reply,
    })
    .await
}

/// Save the current tune to `path` as a `.msq` file. Errors if no tune is
/// loaded or the file cannot be written.
#[tauri::command]
#[specta::specta]
pub async fn save_tune(path: String, owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::SaveTune { path, reply }).await
}
