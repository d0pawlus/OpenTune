// SPDX-License-Identifier: GPL-3.0-or-later
//! Offline tuning: create / open / save a tune with no live ECU link.
use tauri::State;

use crate::dto::DefinitionDto;
use crate::owner::{request, Command, OwnerHandle};

#[tauri::command]
#[specta::specta]
pub async fn new_tune(
    ini_path: String,
    owner: State<'_, OwnerHandle>,
) -> Result<DefinitionDto, String> {
    request(&owner, |reply| Command::NewTune { ini_path, reply }).await
}

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

#[tauri::command]
#[specta::specta]
pub async fn save_tune(path: String, owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::SaveTune { path, reply }).await
}
