// SPDX-License-Identifier: GPL-3.0-or-later
//! Owner-task tests for the M7 AI additions: realtime snapshot retention
//! and constant-bounds lookup.

use std::sync::{Arc, Mutex};

use super::*;

fn test_owner() -> OwnerHandle {
    let emit: Emitter = Arc::new({
        let sink: Arc<Mutex<Vec<OwnerEvent>>> = Arc::default();
        move |ev| sink.lock().unwrap().push(ev)
    });
    spawn_owner_with_emitter(emit)
}

async fn send<T>(tx: &OwnerHandle, make: impl FnOnce(Reply<T>) -> Command) -> Result<T, String> {
    request(tx, make).await
}

async fn connect_and_load(tx: &OwnerHandle) {
    send(tx, |reply| Command::Connect {
        source: ConnectSource::Simulator { ini_path: None },
        reply,
    })
    .await
    .expect("simulator connects");
    send(tx, |reply| Command::LoadTune { reply })
        .await
        .expect("tune loads");
}

#[tokio::test]
async fn constant_bounds_resolves_ini_low_high() {
    let tx = test_owner();
    connect_and_load(&tx).await;
    let (low, high) = send(&tx, |reply| Command::ConstantBounds {
        name: "reqFuel".into(),
        reply,
    })
    .await
    .expect("bounds resolve for a known constant");
    assert!(low < high, "resolved bounds must be a real interval");
    // 12.5 is the canonical in-range test value for reqFuel (session.rs tests).
    assert!((low..=high).contains(&12.5));
}

#[tokio::test]
async fn constant_bounds_unknown_name_errors() {
    let tx = test_owner();
    connect_and_load(&tx).await;
    let err = send(&tx, |reply| Command::ConstantBounds {
        name: "definitelyNotAConstant".into(),
        reply,
    })
    .await
    .unwrap_err();
    assert!(!err.is_empty());
}

#[tokio::test]
async fn realtime_snapshot_is_none_before_any_frame() {
    let tx = test_owner();
    connect_and_load(&tx).await;
    let snap = send(&tx, |reply| Command::RealtimeSnapshot { reply })
        .await
        .expect("snapshot query succeeds");
    assert!(snap.is_none(), "no polling yet, so no frame retained");
}

#[tokio::test]
async fn realtime_snapshot_returns_latest_frame_after_polling() {
    let tx = test_owner();
    connect_and_load(&tx).await;
    send(&tx, |reply| Command::StartRealtime { reply })
        .await
        .expect("realtime starts");
    // Poll interval is 40 ms; wait deterministically for the first frame.
    let mut snap = None;
    for _ in 0..100 {
        snap = send(&tx, |reply| Command::RealtimeSnapshot { reply })
            .await
            .expect("snapshot query succeeds");
        if snap.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let snap = snap.expect("a frame arrives within 1 s of polling");
    assert!(!snap.channels.is_empty(), "frame carries channels");
    send(&tx, |reply| Command::StopRealtime { reply })
        .await
        .expect("realtime stops");
}
