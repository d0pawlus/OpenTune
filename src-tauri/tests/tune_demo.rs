// SPDX-License-Identifier: GPL-3.0-or-later
//! M2 demo (7.4) as a command-level integration test: simulator, no hardware.
//!
//! Drives the exact demo narrative through the public `Session` owner API —
//! the same code path the Tauri commands call — and asserts each beat against
//! the ECU (via a read-back protocol handle) so "reached the wire" is proven,
//! not assumed:
//!
//!   open tune → edit a constant → live write → "modified, not burned" badge
//!   → burn → badge clears → undo/redo round-trips.
//!
//! Full GUI E2E (Playwright/tauri-driver) is deferred: no such harness exists
//! in the repo yet, and the brief says compose the Rust command-level test with
//! vitest component tests as the demo evidence rather than build E2E infra from
//! scratch. See `src/components/dialogs/*.test.tsx` and `src/stores/tune.test.ts`
//! for the UI half.

use opentune_lib::connection::{
    connect_simulator, load_definition_from_str, ActiveConnection, Session,
};
use opentune_model::Value;
use opentune_protocol::{MsProtocol, Protocol};
use opentune_transport::Transport;
use std::sync::Arc;

const BUNDLED_INI: &str = include_str!("../resources/speeduino.sample.ini");

/// Build a live simulator-backed session from the bundled INI.
fn open_session() -> Session {
    let def = Arc::new(load_definition_from_str(BUNDLED_INI).expect("bundled INI parses"));
    let conn = connect_simulator(def.as_ref(), &|_| {}).expect("simulator connects");
    Session {
        conn,
        def,
        tune: None,
        snapshot: None,
    }
}

/// Read `reqFuel`'s raw little-endian U16 straight off the ECU, bypassing the
/// tune — the independent oracle that a write actually reached the wire.
fn ecu_req_fuel_raw(session: &Session) -> u16 {
    let ActiveConnection::Sim { simulator, .. } = &session.conn else {
        panic!("demo runs on the simulator");
    };
    let page = *session.def.pages.iter().find(|p| p.number == 1).unwrap();
    let mut transport = simulator.client_transport();
    transport.open().unwrap();
    let mut proto = MsProtocol::new(session.def.comms.clone(), transport);
    let bytes = proto.read_page(page).expect("read page 1");
    u16::from_le_bytes([bytes[0], bytes[1]])
}

#[test]
fn m2_demo_open_edit_live_write_burn_undo_redo() {
    let mut s = open_session();

    // ── Beat 1: open the tune (read pages from the ECU) ──────────────────────
    let ev = s.load_tune().expect("load_tune");
    assert!(!ev.dirty, "a freshly opened tune is clean (no badge)");
    assert_eq!(
        s.definition().dialogs[0].name,
        "engine_dialog",
        "the data-driven dialog is available to render"
    );

    // ── Beat 2: edit a constant through the dialog → live write ──────────────
    // reqFuel scale is 0.1, so physical 12.5 ms encodes to raw 125.
    let ev = s
        .set_value("reqFuel", Value::Scalar(12.5))
        .expect("set_value writes live");
    assert!(ev.dirty, "editing raises the 'modified, not burned' badge");
    assert_eq!(ev.dirty_pages, vec![1]);
    assert_eq!(ecu_req_fuel_raw(&s), 125, "the write reached ECU RAM");

    // ── Beat 3: burn → badge clears, value persists to flash ─────────────────
    let ev = s.burn().expect("burn");
    assert!(!ev.dirty, "burning clears the badge");
    // Reboot restores RAM from flash; a burned value survives.
    if let ActiveConnection::Sim { simulator, .. } = &s.conn {
        simulator.reboot();
    }
    s.load_tune().expect("reload after reboot");
    assert_eq!(
        s.read_values(&["reqFuel".into()]).unwrap(),
        vec![Value::Scalar(12.5)],
        "the burned value survived a reboot"
    );

    // ── Beat 4: undo / redo round-trip, each reaching the wire ───────────────
    s.set_value("reqFuel", Value::Scalar(20.0)).unwrap();
    assert_eq!(ecu_req_fuel_raw(&s), 200);

    s.undo().expect("undo");
    assert_eq!(
        ecu_req_fuel_raw(&s),
        125,
        "undo reverts the ECU, not just UI"
    );

    s.redo().expect("redo");
    assert_eq!(ecu_req_fuel_raw(&s), 200, "redo re-applies to the ECU");

    // ── Beat 5: visibility is data-driven off live values ────────────────────
    // `crankRPM` is gated by `{ injLayout != 0 }`. injLayout starts 0 → hidden.
    let gate = vec!["injLayout != 0".to_string()];
    assert_eq!(s.eval_conditions(&gate).unwrap(), vec![false]);
    s.set_value("injLayout", Value::Enum(3)).unwrap(); // "Sequential"
    assert_eq!(
        s.eval_conditions(&gate).unwrap(),
        vec![true],
        "selecting a layout reveals the gated field"
    );
}
