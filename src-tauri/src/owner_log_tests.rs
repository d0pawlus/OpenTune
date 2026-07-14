// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// Owner-task fixture with real comms + page-1 constants, shaped like
/// `realtime-owner.ini`, but with **no** `[OutputChannels]` section at all —
/// `ochBlockSize` therefore defaults to 0 (M5 review M2: `start_log` must
/// reject this instead of silently recording zero-column rows).
const NO_OCH_INI: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/no-och-owner.ini"
);

fn test_owner() -> OwnerHandle {
    spawn_owner_with_emitter(Arc::new(|_| {}))
}

async fn send<T>(owner: &OwnerHandle, make: impl FnOnce(Reply<T>) -> Command) -> Result<T, String> {
    request(owner, make).await
}

fn temp_path(extension: &str) -> String {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir()
        .join(format!(
            "opentune-m5-{}-{unique}.{extension}",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

#[tokio::test]
async fn owner_records_opens_and_serves_columnar_log() {
    let owner = test_owner();
    send(&owner, |reply| Command::Connect {
        source: ConnectSource::Simulator { ini_path: None },
        reply,
    })
    .await
    .unwrap();
    let path = temp_path("mlg");
    let started = send(&owner, |reply| Command::StartLog {
        path: path.clone(),
        format: LogFormatDto::MlgV1,
        reply,
    })
    .await
    .unwrap();
    assert!(started.active);

    for _ in 0..100 {
        let status = send(&owner, |reply| Command::LogStatus { reply })
            .await
            .unwrap();
        if status.record_count >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let before_stop_live = send(&owner, |reply| Command::LogStatus { reply })
        .await
        .unwrap()
        .record_count;
    send(&owner, |reply| Command::StopRealtime { reply })
        .await
        .unwrap();
    for _ in 0..100 {
        let count = send(&owner, |reply| Command::LogStatus { reply })
            .await
            .unwrap()
            .record_count;
        if count > before_stop_live {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        send(&owner, |reply| Command::LogStatus { reply })
            .await
            .unwrap()
            .record_count
            > before_stop_live,
        "stopping dashboard realtime must not stop active log acquisition"
    );
    send(&owner, |reply| Command::AddLogMarker {
        text: "pull".into(),
        reply,
    })
    .await
    .unwrap();
    let summary = send(&owner, |reply| Command::StopLog { reply })
        .await
        .unwrap();
    assert!(summary.record_count >= 3);
    assert_eq!(summary.marker_count, 1);

    let data = send(&owner, |reply| Command::GetLogData {
        offset: 0,
        limit: 100,
        reply,
    })
    .await
    .unwrap();
    assert_eq!(data.total_records, summary.record_count);
    assert_eq!(data.columns.len(), summary.fields.len());
    assert_eq!(data.t_ms.len(), summary.record_count as usize);

    let reopened = send(&owner, |reply| Command::OpenLog {
        path: path.clone(),
        format: LogFormatDto::MlgV1,
        reply,
    })
    .await
    .unwrap();
    assert_eq!(reopened.record_count, summary.record_count);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn start_log_requires_connection_and_slice_is_bounded() {
    let owner = test_owner();
    let error = send(&owner, |reply| Command::StartLog {
        path: temp_path("csv"),
        format: LogFormatDto::Csv,
        reply,
    })
    .await
    .unwrap_err();
    assert_eq!(error, NOT_CONNECTED);

    let error = send(&owner, |reply| Command::GetLogData {
        offset: 0,
        limit: crate::log_bridge::MAX_LOG_SLICE + 1,
        reply,
    })
    .await
    .unwrap_err();
    assert!(error.contains("no log opened"));
}

// ── 1b: open_log rejects a file over the size cap ───────────────────────────

#[test]
fn read_log_path_rejects_oversized_file() {
    let path = std::env::temp_dir().join(format!(
        "opentune-m5-oversized-{}-{}.mlg",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        // A sparse file: `set_len` reports the target size in metadata
        // without actually writing (or allocating) 256 MiB of real data.
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_LOG_FILE_BYTES + 1).unwrap();
    }

    let error = read_log_path(path.to_str().unwrap(), LogFormatDto::MlgV1).unwrap_err();
    assert!(
        error.contains(&MAX_LOG_FILE_BYTES.to_string()),
        "error should name the byte limit: {error}"
    );

    let _ = std::fs::remove_file(path);
}

// ── 1c: start_log fails without a realtime source ───────────────────────────

#[tokio::test]
async fn start_log_fails_without_och_block() {
    let owner = test_owner();
    send(&owner, |reply| Command::Connect {
        source: ConnectSource::Simulator {
            ini_path: Some(NO_OCH_INI.to_owned()),
        },
        reply,
    })
    .await
    .expect("simulator connects even without an OutputChannels section");

    let error = send(&owner, |reply| Command::StartLog {
        path: temp_path("csv"),
        format: LogFormatDto::Csv,
        reply,
    })
    .await
    .unwrap_err();
    assert!(
        error.contains("ochBlockSize"),
        "error should name the missing och block: {error}"
    );

    let status = send(&owner, |reply| Command::LogStatus { reply })
        .await
        .unwrap();
    assert!(
        !status.active,
        "start_log must not create an active log without a realtime source"
    );
}

// ── 1e: atomic log save ──────────────────────────────────────────────────────

/// Names in `dir` that look like a leftover `write_log_path` temp file
/// (`<name>.<ext>.tmp-<pid>`).
fn temp_leftovers(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp-"))
        .collect()
}

#[test]
fn write_log_path_leaves_no_temp_file_and_writes_the_destination() {
    let dir = std::env::temp_dir().join(format!(
        "opentune-m5-atomic-ok-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("log.csv");

    let log = Log::new(vec![Field::float("rpm", "RPM")]);
    write_log_path(path.to_str().unwrap(), LogFormatDto::Csv, &log).unwrap();

    assert!(path.is_file(), "destination file must exist after a save");
    assert!(
        temp_leftovers(&dir).is_empty(),
        "no `.tmp-<pid>` file should remain after a successful save: {:?}",
        temp_leftovers(&dir)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn write_log_path_removes_temp_file_and_preserves_destination_on_error() {
    let dir = std::env::temp_dir().join(format!(
        "opentune-m5-atomic-err-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("log.mlg");
    std::fs::write(&path, b"old contents").unwrap();

    // An MLG field with a zero scale fails `write_mlg_v1`'s validation
    // before any bytes reach the temp file's destination rename — a
    // deterministic write failure with no filesystem trickery required.
    let bad_field = Field {
        name: "bad".into(),
        units: String::new(),
        field_type: opentune_datalog::FieldType::F32,
        display_style: 0,
        scale: 0.0,
        transform: 0.0,
        digits: 3,
    };
    let log = Log::new(vec![bad_field]);

    let error = write_log_path(path.to_str().unwrap(), LogFormatDto::MlgV1, &log).unwrap_err();
    assert!(!error.is_empty());

    assert!(
        temp_leftovers(&dir).is_empty(),
        "a failed save must remove its temp file: {:?}",
        temp_leftovers(&dir)
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"old contents",
        "a failed save must not touch the existing destination file"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
