// SPDX-License-Identifier: GPL-3.0-or-later
//! Log-file path validation shared by the owner's `start_log`/`save_log`
//! (write) and `open_log` (read) commands (M5 review H1).
//!
//! Every path crossing the webview boundary is validated here before it
//! ever reaches `std::fs::File::create`/`open`: `start_log` previously only
//! rejected an empty string, and `open_log`/`save_log` validated nothing at
//! all, so a bad path surfaced as a raw OS error (or, for `start_log`, no
//! error at all — the log just silently recorded nothing to an
//! unwritable/nonexistent destination until the flush at `stop_log`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::connection::expand_tilde;

/// Extensions this app accepts for log files, compared case-insensitively.
const LOG_EXTENSIONS: [&str; 2] = ["csv", "mlg"];

/// Validate a path a log will be **written** to (`start_log`/`save_log`):
/// trims the input, expands a leading `~`, requires a `.csv`/`.mlg`
/// extension, requires the parent directory to exist, and rejects a
/// destination that is itself an existing directory.
///
/// Returns the canonicalized parent joined with the file name — the file
/// itself need not exist yet, since this validates a *write* target.
pub fn validate_log_write_path(path: &str) -> Result<PathBuf, String> {
    let (expanded, file_name) = expanded_path_and_file_name(path)?;
    require_log_extension(&expanded)?;

    let parent = expanded
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let canonical_parent = parent
        .canonicalize()
        .map_err(|_| "parent directory does not exist".to_string())?;

    let target = canonical_parent.join(&file_name);
    if target.is_dir() {
        return Err(format!(
            "log path `{}` is a directory, not a file",
            target.display()
        ));
    }
    Ok(target)
}

/// Validate a path a log will be **read** from (`open_log`): trims the
/// input, expands a leading `~`, requires a `.csv`/`.mlg` extension, and
/// requires the file to exist as a regular file. Returns the canonicalized
/// path.
pub fn validate_log_read_path(path: &str) -> Result<PathBuf, String> {
    let (expanded, _file_name) = expanded_path_and_file_name(path)?;
    require_log_extension(&expanded)?;

    let metadata = std::fs::metadata(&expanded)
        .map_err(|error| format!("log file `{}` does not exist: {error}", expanded.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "log path `{}` is not a regular file",
            expanded.display()
        ));
    }
    expanded
        .canonicalize()
        .map_err(|error| format!("cannot resolve log file `{}`: {error}", expanded.display()))
}

/// Trim + expand a leading `~`, rejecting an empty path; splits off the
/// file-name component both validators need.
fn expanded_path_and_file_name(path: &str) -> Result<(PathBuf, OsString), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("log path must not be empty".to_string());
    }
    let expanded = PathBuf::from(expand_tilde(trimmed));
    let file_name = expanded
        .file_name()
        .ok_or_else(|| "log path has no file name".to_string())?
        .to_owned();
    Ok((expanded, file_name))
}

fn require_log_extension(path: &Path) -> Result<(), String> {
    let has_valid_extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            LOG_EXTENSIONS
                .iter()
                .any(|allowed| ext.eq_ignore_ascii_case(allowed))
        });
    if has_valid_extension {
        Ok(())
    } else {
        Err("log files must end in .csv or .mlg".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_path_rejects_empty() {
        let error = validate_log_write_path("   ").unwrap_err();
        assert!(error.contains("must not be empty"), "{error}");
    }

    #[test]
    fn write_path_rejects_missing_parent() {
        let error =
            validate_log_write_path("/definitely/not/a/real/opentune-dir/log.csv").unwrap_err();
        assert!(error.contains("parent directory does not exist"), "{error}");
    }

    #[test]
    fn write_path_rejects_bad_extension() {
        let path = std::env::temp_dir().join("opentune-log-path-test.txt");
        let error = validate_log_write_path(path.to_str().unwrap()).unwrap_err();
        assert!(error.contains("must end in .csv or .mlg"), "{error}");
    }

    #[test]
    fn write_path_rejects_directory_target() {
        let dir =
            std::env::temp_dir().join(format!("opentune-log-path-test-dir-{}", std::process::id()));
        let target = dir.join("sub.csv");
        std::fs::create_dir_all(&target).unwrap();

        let error = validate_log_write_path(target.to_str().unwrap()).unwrap_err();
        assert!(error.contains("is a directory"), "{error}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_path_accepts_valid_target_and_canonicalizes_parent() {
        let dir =
            std::env::temp_dir().join(format!("opentune-log-path-test-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("log.mlg");

        let result = validate_log_write_path(target.to_str().unwrap()).unwrap();
        assert_eq!(result, dir.canonicalize().unwrap().join("log.mlg"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_path_expands_leading_tilde_to_home() {
        // `HOME` is a process-wide env var and `connection::tests` has its
        // own test that transiently overwrites it — share that test's lock
        // so the two can never interleave (see `HOME_ENV_TEST_LOCK`'s doc).
        let _guard = crate::connection::HOME_ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let home = std::env::var("HOME").expect("HOME must be set to run this test");
        let canonical_home =
            std::fs::canonicalize(&home).expect("HOME must be a real, existing directory");

        let result = validate_log_write_path("~/opentune-log-path-tilde-test-target.csv").unwrap();
        assert_eq!(
            result,
            canonical_home.join("opentune-log-path-tilde-test-target.csv")
        );
    }

    #[test]
    fn read_path_rejects_missing_file() {
        let path = std::env::temp_dir().join(format!(
            "opentune-log-path-test-missing-{}.csv",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let error = validate_log_read_path(path.to_str().unwrap()).unwrap_err();
        assert!(error.contains("does not exist"), "{error}");
    }

    #[test]
    fn read_path_rejects_non_regular_file() {
        let dir = std::env::temp_dir().join(format!(
            "opentune-log-path-test-read-dir-{}.csv",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let error = validate_log_read_path(dir.to_str().unwrap()).unwrap_err();
        assert!(error.contains("not a regular file"), "{error}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_path_rejects_bad_extension() {
        let path =
            std::env::temp_dir().join(format!("opentune-log-path-test-{}.bin", std::process::id()));
        std::fs::write(&path, b"data").unwrap();

        let error = validate_log_read_path(path.to_str().unwrap()).unwrap_err();
        assert!(error.contains("must end in .csv or .mlg"), "{error}");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_path_accepts_existing_file() {
        let path = std::env::temp_dir().join(format!(
            "opentune-log-path-test-read-ok-{}.csv",
            std::process::id()
        ));
        std::fs::write(&path, b"Time\n").unwrap();

        let result = validate_log_read_path(path.to_str().unwrap()).unwrap();
        assert_eq!(result, path.canonicalize().unwrap());

        let _ = std::fs::remove_file(&path);
    }
}
