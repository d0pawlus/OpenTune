// SPDX-License-Identifier: GPL-3.0-or-later
//! M2 tune commands — thin async senders into the §9 owner task.
//!
//! Each command builds a oneshot reply channel, sends the matching
//! [`Command`] to the owner (the single owner of connection + definition +
//! tune), and awaits the result. The owner itself emits [`TuneDirtyEvent`]
//! after every mutating op, so the frontend badge reflects the backend's
//! single source of truth; no command touches the transport or any state.
//!
//! [`TuneDirtyEvent`]: crate::events::TuneDirtyEvent

use opentune_model::Value;
use tauri::State;

use crate::dto::{CellEditDto, DefinitionDto, FieldDiffDto, MergePickDto, ResolvedGaugeBoundsDto};
use crate::owner::{request, Command, OwnerHandle};

/// Return the parsed firmware definition (menus, dialogs, constants, …) for
/// the frontend to render the data-driven UI against.
#[tauri::command]
#[specta::specta]
pub async fn get_definition(owner: State<'_, OwnerHandle>) -> Result<DefinitionDto, String> {
    request(&owner, |reply| Command::GetDefinition { reply }).await
}

/// Read all declared pages from the ECU into a fresh tune. The owner emits
/// the (clean) dirty state.
#[tauri::command]
#[specta::specta]
pub async fn load_tune(owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::LoadTune { reply })
        .await
        .map(|_| ())
}

/// Read the current physical values of the named constants (for field render).
#[tauri::command]
#[specta::specta]
pub async fn get_values(
    names: Vec<String>,
    owner: State<'_, OwnerHandle>,
) -> Result<Vec<Value>, String> {
    request(&owner, |reply| Command::GetValues { names, reply }).await
}

/// Resolve all gauge bounds against the currently loaded tune.
#[tauri::command]
#[specta::specta]
pub async fn resolve_gauge_bounds(
    owner: State<'_, OwnerHandle>,
) -> Result<Vec<ResolvedGaugeBoundsDto>, String> {
    request(&owner, |reply| Command::ResolveGaugeBounds { reply }).await
}

/// Set a constant and write the changed bytes live to the ECU. The owner
/// emits the new dirty state on success.
#[tauri::command]
#[specta::specta]
pub async fn set_value(
    name: String,
    value: Value,
    owner: State<'_, OwnerHandle>,
) -> Result<(), String> {
    request(&owner, |reply| Command::SetValue { name, value, reply })
        .await
        .map(|_| ())
}

/// Write individual cells of an array constant (a table-editor gesture).
#[tauri::command]
#[specta::specta]
pub async fn set_cells(
    name: String,
    cells: Vec<CellEditDto>,
    owner: State<'_, OwnerHandle>,
) -> Result<(), String> {
    request(&owner, |reply| Command::SetCells { name, cells, reply })
        .await
        .map(|_| ())
}

/// Burn every dirty page to flash. The owner emits the cleared dirty state.
#[tauri::command]
#[specta::specta]
pub async fn burn_tune(owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::Burn { reply })
        .await
        .map(|_| ())
}

/// Undo the most recent edit, writing the reverted bytes to the ECU.
#[tauri::command]
#[specta::specta]
pub async fn undo_tune(owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::Undo { reply })
        .await
        .map(|_| ())
}

/// Redo the most recently undone edit, writing the re-applied bytes to the ECU.
#[tauri::command]
#[specta::specta]
pub async fn redo_tune(owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::Redo { reply })
        .await
        .map(|_| ())
}

/// Evaluate `visible`/`enable` expressions against the current tune values.
/// Fails open (a broken expression yields `true`).
#[tauri::command]
#[specta::specta]
pub async fn eval_conditions(
    exprs: Vec<String>,
    owner: State<'_, OwnerHandle>,
) -> Result<Vec<bool>, String> {
    request(&owner, |reply| Command::EvalConditions { exprs, reply }).await
}

/// Snapshot the current tune as the diff/merge baseline (the "other" side).
#[tauri::command]
#[specta::specta]
pub async fn snapshot_tune(owner: State<'_, OwnerHandle>) -> Result<(), String> {
    request(&owner, |reply| Command::SnapshotTune { reply }).await
}

/// Diff the current tune against the snapshot baseline taken by
/// `snapshot_tune`.
#[tauri::command]
#[specta::specta]
pub async fn diff_tune(owner: State<'_, OwnerHandle>) -> Result<Vec<FieldDiffDto>, String> {
    request(&owner, |reply| Command::DiffTune { reply }).await
}

/// Merge the picked constants from the snapshot baseline into the current
/// tune, writing each accepted pick live to the ECU.
///
/// The owner emits the tune's *actual* dirty state after the merge attempt —
/// regardless of `Ok`/`Err` — because a merge can abort mid-batch after
/// earlier picks already committed (M2 behavior, preserved).
#[tauri::command]
#[specta::specta]
pub async fn merge_tune(
    picks: Vec<MergePickDto>,
    owner: State<'_, OwnerHandle>,
) -> Result<(), String> {
    request(&owner, |reply| Command::MergeTune { picks, reply })
        .await
        .map(|_| ())
}
