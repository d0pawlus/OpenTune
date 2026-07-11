// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

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
