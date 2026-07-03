// SPDX-License-Identifier: GPL-3.0-or-later
//! Contract tests for the `opentune-protocol` M1 identity seam.

use opentune_ini::{CommsSettings, Endianness, EnvelopeFormat, PageDef};
use opentune_protocol::{ConnectionState, EcuIdentity, Protocol, ProtocolError, Result};

fn comms_with_signature(sig: &str) -> CommsSettings {
    CommsSettings {
        signature: sig.to_string(),
        query_command: "Q".to_string(),
        version_info: "S".to_string(),
        och_get_command: "r".to_string(),
        page_read_command: "p%2i%2o%2c".to_string(),
        page_value_write: "M%2i%2o%2c%v".to_string(),
        burn_command: "b%2i".to_string(),
        blocking_factor: 251,
        page_activation_delay_ms: 10,
        block_read_timeout_ms: 2_000,
        inter_write_delay_ms: 10,
        endianness: Endianness::Little,
        envelope: EnvelopeFormat::MsEnvelope10,
        och_block_size: 0,
    }
}

#[test]
fn identity_matches_only_on_exact_signature() {
    let id = EcuIdentity {
        signature: "speeduino 202504-dev".to_string(),
        version: "Speeduino 2025.04".to_string(),
    };
    assert!(id.matches(&comms_with_signature("speeduino 202504-dev")));
    assert!(!id.matches(&comms_with_signature("rusEFI master.2025")));
}

#[test]
fn connection_state_models_reconnecting_with_attempt() {
    // Pain point #1: reconnect is a first-class, observable state.
    let s = ConnectionState::Reconnecting { attempt: 3 };
    match s {
        ConnectionState::Reconnecting { attempt } => assert_eq!(attempt, 3),
        _ => panic!("expected Reconnecting"),
    }
}

#[test]
fn protocol_error_reports_signature_mismatch() {
    let e = ProtocolError::SignatureMismatch {
        reported: "rusEFI".to_string(),
        expected: "speeduino".to_string(),
    };
    let msg = e.to_string();
    assert!(msg.contains("rusEFI"));
    assert!(msg.contains("speeduino"));
}

/// A canned `Protocol` impl proves the trait is implementable without hardware —
/// the property the simulator and the generic engine agents build on.
struct FakeProtocol {
    signature: String,
}

impl Protocol for FakeProtocol {
    fn identify(&mut self) -> Result<EcuIdentity> {
        Ok(EcuIdentity {
            signature: self.signature.clone(),
            version: "fake 1.0".to_string(),
        })
    }
    fn signature(&mut self) -> Result<String> {
        Ok(self.signature.clone())
    }
    fn version(&mut self) -> Result<String> {
        Ok("fake 1.0".to_string())
    }
    fn read_secl(&mut self) -> Result<u8> {
        Ok(42)
    }
    fn read_page(&mut self, page: PageDef) -> Result<Vec<u8>> {
        Ok(vec![0u8; page.size])
    }
    fn write(&mut self, _page: u16, _offset: u16, _bytes: &[u8]) -> Result<()> {
        Ok(())
    }
    fn burn(&mut self, _page: u16) -> Result<()> {
        Ok(())
    }
    fn read_output_channels(&mut self, _offset: u16, len: u16) -> Result<Vec<u8>> {
        Ok(vec![0u8; len as usize])
    }
}

#[test]
fn protocol_trait_is_implementable() {
    let mut p = FakeProtocol {
        signature: "speeduino 202504-dev".to_string(),
    };
    let id = p.identify().unwrap();
    assert_eq!(id.signature, "speeduino 202504-dev");
    assert!(id.matches(&comms_with_signature("speeduino 202504-dev")));
    assert_eq!(p.read_secl().unwrap(), 42);
}
