// SPDX-License-Identifier: GPL-3.0-or-later
//! M2 tune commands — thin IPC wrappers over [`Session`] operations.
//!
//! Each command locks the single session mutex, delegates to the co-located
//! [`Session`] (which owns the connection, definition, and tune), and — for
//! mutating ops — emits a [`TuneDirtyEvent`] so the frontend badge reflects the
//! backend's single source of truth. No command touches the transport directly.

use opentune_model::Value;
use tauri::{AppHandle, State};
use tauri_specta::Event as _;

use crate::connection::SessionStore;
use crate::dto::DefinitionDto;

const NOT_CONNECTED: &str = "not connected";

/// Return the parsed firmware definition (menus, dialogs, constants, …) for
/// the frontend to render the data-driven UI against.
#[tauri::command]
#[specta::specta]
pub fn get_definition(state: State<'_, SessionStore>) -> Result<DefinitionDto, String> {
    let guard = state.lock().map_err(|e| e.to_string())?;
    let session = guard.as_ref().ok_or_else(|| NOT_CONNECTED.to_string())?;
    Ok(session.definition())
}

/// Read all declared pages from the ECU into a fresh tune. Emits the (clean)
/// dirty state.
#[tauri::command]
#[specta::specta]
pub fn load_tune(state: State<'_, SessionStore>, app: AppHandle) -> Result<(), String> {
    let event = {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        let session = guard.as_mut().ok_or_else(|| NOT_CONNECTED.to_string())?;
        session.load_tune()?
    };
    let _ = event.emit(&app);
    Ok(())
}

/// Read the current physical values of the named constants (for field render).
#[tauri::command]
#[specta::specta]
pub fn get_values(
    names: Vec<String>,
    state: State<'_, SessionStore>,
) -> Result<Vec<Value>, String> {
    let guard = state.lock().map_err(|e| e.to_string())?;
    let session = guard.as_ref().ok_or_else(|| NOT_CONNECTED.to_string())?;
    session.read_values(&names)
}

/// Set a constant and write the changed bytes live to the ECU. Emits the new
/// dirty state on success.
#[tauri::command]
#[specta::specta]
pub fn set_value(
    name: String,
    value: Value,
    state: State<'_, SessionStore>,
    app: AppHandle,
) -> Result<(), String> {
    let event = {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        let session = guard.as_mut().ok_or_else(|| NOT_CONNECTED.to_string())?;
        session.set_value(&name, value)?
    };
    let _ = event.emit(&app);
    Ok(())
}

/// Burn every dirty page to flash. Emits the cleared dirty state.
#[tauri::command]
#[specta::specta]
pub fn burn_tune(state: State<'_, SessionStore>, app: AppHandle) -> Result<(), String> {
    let event = {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        let session = guard.as_mut().ok_or_else(|| NOT_CONNECTED.to_string())?;
        session.burn()?
    };
    let _ = event.emit(&app);
    Ok(())
}

/// Undo the most recent edit, writing the reverted bytes to the ECU.
#[tauri::command]
#[specta::specta]
pub fn undo_tune(state: State<'_, SessionStore>, app: AppHandle) -> Result<(), String> {
    let event = {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        let session = guard.as_mut().ok_or_else(|| NOT_CONNECTED.to_string())?;
        session.undo()?
    };
    let _ = event.emit(&app);
    Ok(())
}

/// Redo the most recently undone edit, writing the re-applied bytes to the ECU.
#[tauri::command]
#[specta::specta]
pub fn redo_tune(state: State<'_, SessionStore>, app: AppHandle) -> Result<(), String> {
    let event = {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        let session = guard.as_mut().ok_or_else(|| NOT_CONNECTED.to_string())?;
        session.redo()?
    };
    let _ = event.emit(&app);
    Ok(())
}

/// Evaluate `visible`/`enable` expressions against the current tune values.
/// Fails open (a broken expression yields `true`).
#[tauri::command]
#[specta::specta]
pub fn eval_conditions(
    exprs: Vec<String>,
    state: State<'_, SessionStore>,
) -> Result<Vec<bool>, String> {
    let guard = state.lock().map_err(|e| e.to_string())?;
    let session = guard.as_ref().ok_or_else(|| NOT_CONNECTED.to_string())?;
    session.eval_conditions(&exprs)
}
