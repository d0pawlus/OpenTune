// SPDX-License-Identifier: GPL-3.0-or-later
//! `ConnectionManager` — reliable reconnect (M1 pain point #1).
//!
//! TDD cycle so far:
//! - Test 1 RED→GREEN: `connect()` opens transport + identify → Connected.
//! - Test 2 RED→GREEN: `reconnect_collect_states()` emits Reconnecting states.
//! - Test 3 RED (compile)→GREEN: stubs for `advance_secl` / `last_reconnect_caused_reidentify`.
//! - Test 4 RED assertion: `secl going backwards must trigger re-identify`
//!   → now implementing `last_secl` + reboot detection.

use std::time::Duration;

use crate::{ConnectionState, MsProtocol, Protocol, ProtocolError, Result};
use opentune_ini::CommsSettings;
use opentune_transport::Transport;

/// Backoff + retry configuration.
#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            max_attempts: 10,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
        }
    }
}

/// Orchestrates connect, drop detection, and auto-reconnect.
pub struct ConnectionManager<T, F>
where
    T: Transport,
    F: FnMut() -> Result<T>,
{
    comms: CommsSettings,
    config: ReconnectConfig,
    factory: F,
    state: ConnectionState,
    /// `secl` read after the last successful handshake — baseline for reboot detection.
    last_secl: u8,
    /// Set true when `secl` went backwards on the last reconnect (ECU rebooted).
    last_reconnect_caused_reidentify: bool,
    _marker: std::marker::PhantomData<T>,
}

impl<T, F> ConnectionManager<T, F>
where
    T: Transport,
    F: FnMut() -> Result<T>,
{
    pub fn new(comms: CommsSettings, config: ReconnectConfig, factory: F) -> Self {
        Self {
            comms,
            config,
            factory,
            state: ConnectionState::Disconnected,
            last_secl: 0,
            last_reconnect_caused_reidentify: false,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn state(&self) -> &ConnectionState {
        &self.state
    }

    /// `secl` read after the last successful handshake.
    pub fn last_secl(&self) -> u8 {
        self.last_secl
    }

    /// Force internal state to a value — test helper only.
    #[doc(hidden)]
    pub fn force_state_for_test(&mut self, state: ConnectionState) {
        self.state = state;
    }

    /// True when the last reconnect detected an ECU reboot (secl went backwards).
    pub fn last_reconnect_caused_reidentify(&self) -> bool {
        self.last_reconnect_caused_reidentify
    }

    /// Open transport, run MS handshake, read `secl` baseline, → Connected.
    /// GREEN: test 1 `initial_connect_reaches_connected`.
    pub fn connect(&mut self) -> Result<ConnectionState> {
        self.state = ConnectionState::Connecting;
        self.last_reconnect_caused_reidentify = false;

        let mut transport = (self.factory)()?;
        transport.open().map_err(ProtocolError::Transport)?;
        let mut proto = MsProtocol::new(self.comms.clone(), transport);
        let identity = proto.identify()?;
        // Capture baseline secl so reconnect can detect backwards movement.
        self.last_secl = proto.read_secl().unwrap_or(0);

        let connected = ConnectionState::Connected { identity };
        self.state = connected.clone();
        Ok(connected)
    }

    /// Run the reconnect loop. Emits Reconnecting states, ends with Connected
    /// or Failed. On success, compares new secl with `last_secl`:
    /// - secl advanced → glitch (GREEN: test 3).
    /// - secl went backwards → reboot; `last_reconnect_caused_reidentify = true`
    ///   (GREEN: test 4).
    pub fn reconnect_collect_states(&mut self) -> Vec<ConnectionState> {
        let mut emitted = Vec::new();

        for attempt in 1..=self.config.max_attempts {
            let reconnecting = ConnectionState::Reconnecting { attempt };
            self.state = reconnecting.clone();
            emitted.push(reconnecting);

            // Exponential backoff between attempts (zero in tests, real delay in production).
            let delay = self.backoff_delay(attempt);
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }

            let result: Result<ConnectionState> = (|| {
                let mut transport = (self.factory)()?;
                transport.open().map_err(ProtocolError::Transport)?;
                let mut proto = MsProtocol::new(self.comms.clone(), transport);
                let identity = proto.identify()?;

                // secl resync: detect reboot when counter went backwards.
                let new_secl = proto.read_secl().unwrap_or(self.last_secl);
                self.last_reconnect_caused_reidentify = new_secl < self.last_secl;
                self.last_secl = new_secl;

                let connected = ConnectionState::Connected { identity };
                self.state = connected.clone();
                Ok(connected)
            })();

            match result {
                Ok(state) => {
                    emitted.push(state);
                    return emitted;
                }
                Err(_) => { /* retry */ }
            }
        }

        let failed = ConnectionState::Failed {
            reason: format!(
                "reconnect failed after {} attempts",
                self.config.max_attempts
            ),
        };
        self.state = failed.clone();
        emitted.push(failed);
        emitted
    }

    fn backoff_delay(&self, attempt: u32) -> Duration {
        // 2^(attempt-1), capped at u32::MAX to avoid overflow.
        let factor: u32 = 1u32.checked_shl((attempt - 1).min(30)).unwrap_or(u32::MAX);
        let raw = self.config.base_delay.saturating_mul(factor);
        raw.min(self.config.max_delay)
    }
}
