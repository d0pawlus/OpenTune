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
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tauri::Manager as _;

/// File name inside the app config dir.
const LAYOUT_FILE: &str = "dashboard-layout.json";

/// Makes same-process concurrent saves use distinct files in the target
/// directory. `rename` can then atomically publish each complete write.
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Write `json` atomically to `<dir>/dashboard-layout.json`, creating `dir`
/// if missing.
fn save_layout_in(dir: &Path, json: &str) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("failed to create config dir: {e}"))?;

    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp_path = dir.join(format!(
        ".{LAYOUT_FILE}.{}.{}.tmp",
        std::process::id(),
        sequence
    ));
    let target_path = dir.join(LAYOUT_FILE);

    let result = (|| -> Result<(), String> {
        let mut temp = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|e| format!("failed to create temporary layout: {e}"))?;
        temp.write_all(json.as_bytes())
            .map_err(|e| format!("failed to write temporary layout: {e}"))?;
        temp.sync_all()
            .map_err(|e| format!("failed to sync temporary layout: {e}"))?;
        drop(temp);
        fs::rename(&temp_path, &target_path).map_err(|e| format!("failed to publish layout: {e}"))
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
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
    use std::sync::{Arc, Barrier};

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

    #[test]
    fn concurrent_saves_publish_one_complete_layout_and_clean_up_temps() {
        let scratch = ScratchDir::new("concurrent");
        let dir = Arc::new(scratch.0.clone());
        let payloads = (0..8)
            .map(|index| {
                format!(
                    r#"{{"version":1,"writer":{index},"padding":"{}"}}"#,
                    "x".repeat(16_384)
                )
            })
            .collect::<Vec<_>>();
        let barrier = Arc::new(Barrier::new(payloads.len()));

        let handles = payloads
            .iter()
            .cloned()
            .map(|payload| {
                let dir = Arc::clone(&dir);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    save_layout_in(&dir, &payload)
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle
                .join()
                .expect("save thread does not panic")
                .expect("concurrent save succeeds");
        }

        let saved = load_layout_in(&scratch.0)
            .expect("load succeeds")
            .expect("a layout was published");
        assert!(
            payloads.contains(&saved),
            "published layout must be one complete writer payload"
        );
        let temp_files = fs::read_dir(&scratch.0)
            .expect("scratch directory exists")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(temp_files, 0, "successful saves must not leave temp files");
    }
}
