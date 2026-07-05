// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests: M2 Task 6 — the simulator's backing memory image,
//! exercised through the **real** `opentune_protocol::MsProtocol` (not
//! scripted bytes, per `tests/pages.rs`'s note that Task 6 is what grows the
//! sim to speak the page protocol). Proves the sim and the Task 5 protocol
//! layer agree on wire format end-to-end, in both framings.
//!
//! See `crates/simulator/src/memory.rs` for the port-note / license record
//! (`speeduino-serial-sim`, MIT — confirmed fresh, not GPL-3 as the task
//! brief assumed; it has no page-write or RAM/flash logic to port, so the
//! state logic here is written fresh per ADR-0006, corroborated only for
//! wire *shape* against that reference and against Speeduino `comms.cpp`
//! directly, as `opentune-protocol`'s Task 5 already did).

use opentune_ini::{CommsSettings, Definition, Endianness, EnvelopeFormat, FrontPageDef, PageDef};
use opentune_protocol::{MsProtocol, Protocol};
use opentune_simulator::EcuSimulator;

fn plain_comms() -> CommsSettings {
    CommsSettings {
        signature: EcuSimulator::SIGNATURE.to_owned(),
        query_command: "Q".to_owned(),
        version_info: "S".to_owned(),
        och_get_command: "A".to_owned(),
        page_read_command: "p%2i%2o%2c".to_owned(),
        page_value_write: "M%2i%2o%2c%v".to_owned(),
        burn_command: "b%2i".to_owned(),
        blocking_factor: 121,
        page_activation_delay_ms: 0,
        block_read_timeout_ms: 2_000,
        inter_write_delay_ms: 0,
        endianness: Endianness::Little,
        envelope: EnvelopeFormat::Plain,
        och_block_size: 0,
    }
}

fn crc_comms() -> CommsSettings {
    CommsSettings {
        envelope: EnvelopeFormat::MsEnvelope10,
        ..plain_comms()
    }
}

fn definition_with_pages(pages: Vec<PageDef>) -> Definition {
    Definition {
        comms: plain_comms(),
        pages,
        constants: Vec::new(),
        pc_variables: Vec::new(),
        menus: Vec::new(),
        dialogs: Vec::new(),
        tables: Vec::new(),
        curves: Vec::new(),
        diagnostics: Vec::new(),
        output_channels: Vec::new(),
        gauges: Vec::new(),
        frontpage: FrontPageDef {
            gauge_slots: Vec::new(),
            indicators: Vec::new(),
        },
    }
}

// ── 6.1: write_read_roundtrip ──────────────────────────────────────────

#[test]
fn write_read_roundtrip_plain() {
    let def = definition_with_pages(vec![PageDef { number: 0, size: 4 }]);
    let sim = EcuSimulator::from_definition(&def);
    let mut proto = MsProtocol::new(plain_comms(), sim.client_transport());

    let before = proto
        .read_page(PageDef { number: 0, size: 4 })
        .expect("initial read must succeed");
    assert_eq!(before, vec![0, 0, 0, 0], "fresh page must be zero-filled");

    proto
        .write(0, 1, &[0xAA, 0xBB])
        .expect("write must succeed");

    let after = proto
        .read_page(PageDef { number: 0, size: 4 })
        .expect("post-write read must succeed");
    assert_eq!(
        after,
        vec![0, 0xAA, 0xBB, 0],
        "read after write must reflect the mutated RAM"
    );
}

#[test]
fn write_read_roundtrip_crc() {
    let def = definition_with_pages(vec![PageDef { number: 0, size: 4 }]);
    let sim = EcuSimulator::from_definition(&def);
    let mut proto = MsProtocol::new(crc_comms(), sim.client_transport());

    proto
        .write(0, 0, &[0x11, 0x22, 0x33, 0x44])
        .expect("CRC write must succeed");
    let after = proto
        .read_page(PageDef { number: 0, size: 4 })
        .expect("CRC read must succeed");
    assert_eq!(after, vec![0x11, 0x22, 0x33, 0x44]);
}

#[test]
fn write_read_roundtrip_multiple_pages_are_independent() {
    let def = definition_with_pages(vec![
        PageDef { number: 0, size: 2 },
        PageDef { number: 1, size: 2 },
    ]);
    let sim = EcuSimulator::from_definition(&def);
    let mut proto = MsProtocol::new(plain_comms(), sim.client_transport());

    proto.write(0, 0, &[0xAA, 0xAA]).unwrap();
    proto.write(1, 0, &[0xBB, 0xBB]).unwrap();

    assert_eq!(
        proto.read_page(PageDef { number: 0, size: 2 }).unwrap(),
        vec![0xAA, 0xAA]
    );
    assert_eq!(
        proto.read_page(PageDef { number: 1, size: 2 }).unwrap(),
        vec![0xBB, 0xBB]
    );
}

// ── 6.2: burn_persists ─────────────────────────────────────────────────

#[test]
fn burn_persists_across_reboot_unburned_lost_plain() {
    let def = definition_with_pages(vec![PageDef { number: 0, size: 2 }]);
    let sim = EcuSimulator::from_definition(&def);
    let mut proto = MsProtocol::new(plain_comms(), sim.client_transport());

    // Burned write.
    proto.write(0, 0, &[0xAA, 0xBB]).unwrap();
    proto.burn(0).expect("burn must succeed");

    // Un-burned write on top of the burned bytes.
    proto.write(0, 0, &[0xCC, 0xDD]).unwrap();
    let before_reboot = proto.read_page(PageDef { number: 0, size: 2 }).unwrap();
    assert_eq!(before_reboot, vec![0xCC, 0xDD]);

    sim.reboot();

    let after_reboot = proto.read_page(PageDef { number: 0, size: 2 }).unwrap();
    assert_eq!(
        after_reboot,
        vec![0xAA, 0xBB],
        "burned bytes must survive reboot; the un-burned write must be lost"
    );
}

#[test]
fn burn_persists_across_reboot_crc() {
    let def = definition_with_pages(vec![PageDef { number: 2, size: 3 }]);
    let sim = EcuSimulator::from_definition(&def);
    let mut proto = MsProtocol::new(crc_comms(), sim.client_transport());

    proto.write(2, 0, &[0x01, 0x02, 0x03]).unwrap();
    proto.burn(2).unwrap();
    proto.write(2, 0, &[0x99, 0x99, 0x99]).unwrap(); // not burned

    sim.reboot();

    let after = proto.read_page(PageDef { number: 2, size: 3 }).unwrap();
    assert_eq!(after, vec![0x01, 0x02, 0x03]);
}

#[test]
fn reboot_without_any_burn_resets_to_zero() {
    let def = definition_with_pages(vec![PageDef { number: 0, size: 2 }]);
    let sim = EcuSimulator::from_definition(&def);
    let mut proto = MsProtocol::new(plain_comms(), sim.client_transport());

    proto.write(0, 0, &[0xFF, 0xFF]).unwrap();
    sim.reboot(); // never burned

    let after = proto.read_page(PageDef { number: 0, size: 2 }).unwrap();
    assert_eq!(after, vec![0, 0]);
}

// ── Fail-safe: malformed / out-of-range page ops never panic the sim ──────

#[test]
fn write_to_page_outside_definition_does_not_panic() {
    let def = definition_with_pages(vec![PageDef { number: 0, size: 2 }]);
    let sim = EcuSimulator::from_definition(&def);
    let mut proto = MsProtocol::new(plain_comms(), sim.client_transport());

    // Page 9 was never declared — a real ECU's setPageValue-style tolerance
    // is a no-op, not a crash; the wire exchange itself must still succeed.
    proto
        .write(9, 0, &[0xAA])
        .expect("write to an unknown page must not error or panic the sim");
    proto
        .burn(9)
        .expect("burn of an unknown page must not error or panic the sim");
}

#[test]
fn out_of_range_offset_write_is_ignored_not_panic() {
    let def = definition_with_pages(vec![PageDef { number: 0, size: 2 }]);
    let sim = EcuSimulator::from_definition(&def);
    let mut proto = MsProtocol::new(plain_comms(), sim.client_transport());

    proto
        .write(0, 10, &[0xAA, 0xBB]) // offset 10 is past the 2-byte page
        .expect("out-of-range write must not error or panic the sim");

    let after = proto.read_page(PageDef { number: 0, size: 2 }).unwrap();
    assert_eq!(after, vec![0, 0], "out-of-range write must be a no-op");
}

// ── M1 regression: handshake + drop control still work on a paged sim ─────

#[test]
fn handshake_and_drop_control_still_work_with_pages_declared() {
    let def = definition_with_pages(vec![PageDef { number: 0, size: 4 }]);
    let sim = EcuSimulator::from_definition(&def);
    let mut proto = MsProtocol::new(plain_comms(), sim.client_transport());

    let identity = proto.identify().expect("identify must still succeed");
    assert_eq!(identity.signature, EcuSimulator::SIGNATURE);
    assert_eq!(identity.version, EcuSimulator::VERSION);

    sim.set_link_dropped(true);
    assert!(proto.read_secl().is_err(), "dropped link must fail reads");
    sim.set_link_dropped(false);
    assert!(
        proto.read_secl().is_ok(),
        "restored link must succeed again"
    );
}
