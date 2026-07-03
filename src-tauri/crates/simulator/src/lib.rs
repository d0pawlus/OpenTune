// SPDX-License-Identifier: GPL-3.0-or-later
//! `opentune-simulator` — minimal virtual ECU for hardware-free dev and CI.
//!
//! Per [ADR-0006](../../../docs/adr/0006-reuse-existing-parsers.md), wire
//! semantics are ported from the Speeduino / rusEFI open firmware sources.

pub mod ecu;
pub mod memory;
pub use ecu::{EcuClientTransport, EcuSimulator};
pub use memory::MemoryImage;

#[cfg(test)]
mod tests {
    use opentune_ini::{CommsSettings, Endianness, EnvelopeFormat};
    use opentune_protocol::{MsProtocol, Protocol};
    use opentune_transport::{Transport, TransportError};

    fn speeduino_plain_comms() -> CommsSettings {
        CommsSettings {
            signature: "speeduino 202504-dev".to_owned(),
            query_command: "Q".to_owned(),
            version_info: "S".to_owned(),
            och_get_command: "A".to_owned(),
            page_read_command: "p%2i%2o%2c".to_owned(),
            page_value_write: "M%2i%2o%2c%v".to_owned(),
            burn_command: "b%2i".to_owned(),
            blocking_factor: 121,
            page_activation_delay_ms: 10,
            block_read_timeout_ms: 2_000,
            inter_write_delay_ms: 10,
            endianness: Endianness::Little,
            envelope: EnvelopeFormat::Plain,
        }
    }

    fn speeduino_crc_comms() -> CommsSettings {
        CommsSettings {
            envelope: EnvelopeFormat::MsEnvelope10,
            ..speeduino_plain_comms()
        }
    }

    // Test 1 — plain protocol signature (GREEN).
    #[test]
    fn handshake_plain_returns_correct_signature() {
        let sim = crate::EcuSimulator::new();
        let client = sim.client_transport();
        let comms = speeduino_plain_comms();
        let mut proto = MsProtocol::new(comms, client);
        let sig = proto.signature().expect("signature query must succeed");
        assert_eq!(sig, "speeduino 202504-dev");
        sim.flush();
    }

    // Test 2 — version query (GREEN).
    #[test]
    fn handshake_plain_returns_version() {
        let sim = crate::EcuSimulator::new();
        let client = sim.client_transport();
        let comms = speeduino_plain_comms();
        let mut proto = MsProtocol::new(comms, client);
        let version = proto.version().expect("version query must succeed");
        assert_eq!(version, crate::EcuSimulator::VERSION);
        sim.flush();
    }

    // Test 3 — identify: signature + version together, and matches() succeeds.
    #[test]
    fn handshake_plain_identify_succeeds() {
        let sim = crate::EcuSimulator::new();
        let client = sim.client_transport();
        let comms = speeduino_plain_comms();
        let mut proto = MsProtocol::new(comms.clone(), client);
        let identity = proto.identify().expect("identify must succeed");
        assert_eq!(identity.signature, crate::EcuSimulator::SIGNATURE);
        assert_eq!(identity.version, crate::EcuSimulator::VERSION);
        assert!(identity.matches(&comms));
        sim.flush();
    }

    // Test 4 — CRC (msEnvelope_1.0) signature query.
    #[test]
    fn handshake_crc_returns_correct_signature() {
        let sim = crate::EcuSimulator::new_crc();
        let client = sim.client_transport();
        let comms = speeduino_crc_comms();
        let mut proto = MsProtocol::new(comms, client);
        let sig = proto.signature().expect("CRC signature query must succeed");
        assert_eq!(sig, crate::EcuSimulator::SIGNATURE);
        sim.flush();
    }

    // Test 5 — CRC identify.
    #[test]
    fn handshake_crc_identify_succeeds() {
        let sim = crate::EcuSimulator::new_crc();
        let client = sim.client_transport();
        let comms = speeduino_crc_comms();
        let mut proto = MsProtocol::new(comms.clone(), client);
        let identity = proto.identify().expect("CRC identify must succeed");
        assert_eq!(identity.signature, crate::EcuSimulator::SIGNATURE);
        assert_eq!(identity.version, crate::EcuSimulator::VERSION);
        assert!(identity.matches(&comms));
        sim.flush();
    }

    // Test 6 — read_secl returns a u8.
    #[test]
    fn read_secl_returns_a_byte() {
        let sim = crate::EcuSimulator::new();
        let client = sim.client_transport();
        let comms = speeduino_plain_comms();
        let mut proto = MsProtocol::new(comms, client);
        let _secl = proto.read_secl().expect("read_secl must succeed");
        sim.flush();
    }

    // Test 7 — drop link makes next write fail.
    #[test]
    fn drop_link_makes_next_op_fail() {
        let sim = crate::EcuSimulator::new();
        let mut client = sim.client_transport();
        sim.set_link_dropped(true);
        let err = client.write(b"Q").unwrap_err();
        assert!(
            matches!(
                err,
                TransportError::Disconnected | TransportError::Timeout(_)
            ),
            "expected Disconnected or Timeout after drop, got: {err:?}"
        );
    }

    // Test 8 — restore after drop allows handshake.
    #[test]
    fn drop_link_then_restore_allows_handshake() {
        let sim = crate::EcuSimulator::new();
        let mut client = sim.client_transport();
        // Drop.
        sim.set_link_dropped(true);
        assert!(client.write(b"Q").is_err());
        // Restore.
        sim.set_link_dropped(false);
        // Handshake should now succeed.
        let comms = speeduino_plain_comms();
        let mut proto = MsProtocol::new(comms, client);
        let identity = proto
            .identify()
            .expect("identify must succeed after restore");
        assert_eq!(identity.signature, crate::EcuSimulator::SIGNATURE);
        sim.flush();
    }

    // Test 9 — protocol error on dropped link during query.
    #[test]
    fn protocol_error_on_dropped_link_during_query() {
        let sim = crate::EcuSimulator::new();
        let client = sim.client_transport();
        let comms = speeduino_plain_comms();
        let mut proto = MsProtocol::new(comms, client);
        // Drop the link before the query.
        sim.set_link_dropped(true);
        let err = proto.signature().unwrap_err();
        assert!(
            matches!(
                err,
                opentune_protocol::ProtocolError::Transport(
                    TransportError::Disconnected | TransportError::Timeout(_)
                )
            ),
            "expected Transport error, got: {err:?}"
        );
    }
}
