// SPDX-License-Identifier: GPL-3.0-or-later
//! Dashboard layout persistence — minimal JSON file in the app config dir
//! (plan Decision 6), not the `project` crate.
//!
//! These commands are pure file I/O: they never touch the wire or the
//! session, so they do **not** route through the §9 owner task — plain
//! `#[tauri::command]`s resolving the path via `app.path().app_config_dir()`.
//! The JSON is an opaque blob owned by the frontend; it is validated (shape,
//! known gauge names) on the frontend when loaded, never interpreted here.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use tauri::Manager as _;

/// File name inside the app config dir.
const LAYOUT_FILE: &str = "dashboard-layout.json";

/// Write `json` to `<dir>/dashboard-layout.json`, creating `dir` if missing.
fn save_layout_in(dir: &Path, json: &str) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("failed to create config dir: {e}"))?;
    fs::write(dir.join(LAYOUT_FILE), json).map_err(|e| format!("failed to write layout: {e}"))
}

/// Read `<dir>/dashboard-layout.json` back; `Ok(None)` when never saved.
fn load_layout_in(dir: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(dir.join(LAYOUT_FILE)) {
        Ok(json) => Ok(Some(json)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("failed to read layout: {e}")),
    }
}

fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|e| format!("failed to resolve app config dir: {e}"))
}

/// Persist the dashboard layout JSON to the app config dir.
#[tauri::command]
#[specta::specta]
pub async fn save_layout(app: tauri::AppHandle, json: String) -> Result<(), String> {
    save_layout_in(&config_dir(&app)?, &json)
}

/// Load the persisted dashboard layout JSON; `None` when never saved.
#[tauri::command]
#[specta::specta]
pub async fn load_layout(app: tauri::AppHandle) -> Result<Option<String>, String> {
    load_layout_in(&config_dir(&app)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A per-test scratch dir (std-only — no tempfile dev-dependency),
    /// removed on drop so failures don't leak files across runs.
    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "opentune-layout-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&dir);
            Self(dir)
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn round_trips_layout_json() {
        let scratch = ScratchDir::new("roundtrip");
        let json = r#"{"version":1,"slots":[{"gauge":"rpmGauge","kind":"round"}]}"#;
        save_layout_in(&scratch.0, json).expect("save succeeds");
        assert_eq!(
            load_layout_in(&scratch.0)
                .expect("load succeeds")
                .as_deref(),
            Some(json)
        );
    }

    #[test]
    fn load_returns_none_when_never_saved() {
        let scratch = ScratchDir::new("missing");
        assert_eq!(load_layout_in(&scratch.0), Ok(None));
    }

    #[test]
    fn save_creates_nested_missing_config_dir() {
        let scratch = ScratchDir::new("nested");
        let nested = scratch.0.join("a").join("b");
        save_layout_in(&nested, "{}").expect("save creates dirs");
        assert_eq!(
            load_layout_in(&nested).expect("load succeeds").as_deref(),
            Some("{}")
        );
    }

    #[test]
    fn save_overwrites_previous_layout() {
        let scratch = ScratchDir::new("overwrite");
        save_layout_in(&scratch.0, "old").expect("first save");
        save_layout_in(&scratch.0, "new").expect("second save");
        assert_eq!(
            load_layout_in(&scratch.0).expect("load").as_deref(),
            Some("new")
        );
    }
}
