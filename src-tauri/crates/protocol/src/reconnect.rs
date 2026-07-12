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

/// True when `new` cannot be a forward step from `last` on the wrapping u8
/// second counter — i.e. the counter regressed (reboot signal).
/// ponytail: half-range discriminator — a reboot landing within 128 s of the
/// old counter position is indistinguishable from forward motion; upgrade
/// path is pairing secl with a page checksum/identify probe.
fn secl_regressed(last: u8, new: u8) -> bool {
    new.wrapping_sub(last) >= 128
}

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
    /// The live protocol/transport. Keeping it here makes the manager an actual
    /// connection owner: health checks can detect an unplug after the handshake,
    /// and reconnect atomically replaces the dead link.
    protocol: Option<MsProtocol<T>>,
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
            protocol: None,
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

    /// Keep the reboot-detection baseline live while realtime polling runs
    /// (M3 Task 6 blocker c). `secl` is byte 0 of the output-channel block
    /// by MS/TS convention, so the owner feeds every successfully polled
    /// block's first byte here. Without this, the baseline captured at
    /// connect goes stale while the counter advances/wraps (or is zeroed by
    /// the firmware's first-och-request reset) during polling — a later
    /// glitch reconnect then reads `new_secl < last_secl`, falsely detects
    /// a reboot, and the owner's reboot path re-reads the tune, silently
    /// discarding unburned edits.
    pub fn note_secl(&mut self, secl: u8) {
        self.last_secl = secl;
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

    /// Probe the live link through the INI-defined output-channel command.
    ///
    /// The owner calls this while realtime polling is idle. Any transport or
    /// protocol error is a drop signal and should enter the reconnect loop.
    pub fn check_link(&mut self) -> Result<u8> {
        let proto = self.protocol.as_mut().ok_or(ProtocolError::Transport(
            opentune_transport::TransportError::Disconnected,
        ))?;
        let secl = proto.read_secl()?;
        // A backwards jump outside normal wrap motion is a reboot signal.
        // Preserve the old baseline and let the owner enter reconnect so the
        // normal re-identify/re-read path runs.
        if secl_regressed(self.last_secl, secl) {
            return Err(ProtocolError::MalformedResponse(format!(
                "ECU second counter moved backwards ({} -> {secl})",
                self.last_secl
            )));
        }
        self.last_secl = secl;
        Ok(secl)
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
        if !identity.matches(&self.comms) {
            let error = ProtocolError::SignatureMismatch {
                reported: identity.signature,
                expected: self.comms.signature.clone(),
            };
            self.state = ConnectionState::Failed {
                reason: error.to_string(),
            };
            return Err(error);
        }
        // Capture baseline secl so reconnect can detect backwards movement.
        self.last_secl = proto.read_secl().unwrap_or(0);

        let connected = ConnectionState::Connected { identity };
        self.state = connected.clone();
        self.protocol = Some(proto);
        Ok(connected)
    }

    /// Run the reconnect loop, collecting the emitted states. Thin delegation
    /// to [`Self::reconnect_streaming`] with no observer and no cancellation.
    /// On success, compares new secl with `last_secl` via [`secl_regressed`]
    /// (wrap-aware):
    /// - forward motion, including an ordinary u8 wrap → glitch (GREEN: test 3).
    /// - a regression outside normal wrap motion → reboot;
    ///   `last_reconnect_caused_reidentify = true` (GREEN: test 4).
    pub fn reconnect_collect_states(&mut self) -> Vec<ConnectionState> {
        self.reconnect_streaming(|_| {}, || false, |_| {})
    }

    /// Reconnect with a callback after each failed non-terminal attempt.
    ///
    /// The hook lets the simulator demo restore its deliberately dropped link
    /// only after proving that one real reconnect attempt failed.
    pub fn reconnect_collect_states_with_retry_hook(
        &mut self,
        on_failed_attempt: impl FnMut(u32),
    ) -> Vec<ConnectionState> {
        self.reconnect_streaming(|_| {}, || false, on_failed_attempt)
    }

    /// The primary reconnect loop: streaming, cancellable.
    ///
    /// `on_state` observes each state the moment it is produced —
    /// `Reconnecting { attempt }` *before* the attempt's backoff sleep,
    /// `Connected`/`Failed` when reached — so a caller can forward live
    /// progress to a UI while the full backoff schedule (worth minutes on
    /// serial) is still running. The collected states are also returned;
    /// callers derive the outcome from the terminal state.
    ///
    /// `cancelled` is checked at the top of every attempt and between short
    /// chunks of every backoff sleep, so a cancel interrupts even the capped
    /// 30 s serial backoff promptly. A cancelled loop returns the states
    /// emitted so far WITHOUT a terminal `Failed`: the cancel initiator owns
    /// the UI transition, and a stale `Failed` event landing after a user's
    /// Disconnect would corrupt the connection state it just displayed.
    pub fn reconnect_streaming(
        &mut self,
        mut on_state: impl FnMut(&ConnectionState),
        mut cancelled: impl FnMut() -> bool,
        mut on_failed_attempt: impl FnMut(u32),
    ) -> Vec<ConnectionState> {
        let mut emitted = Vec::new();
        let mut last_error = None;
        self.protocol = None;

        for attempt in 1..=self.config.max_attempts {
            if cancelled() {
                return emitted;
            }
            let reconnecting = ConnectionState::Reconnecting { attempt };
            self.state = reconnecting.clone();
            on_state(&reconnecting);
            emitted.push(reconnecting);

            // Exponential backoff between attempts (zero in tests, real delay
            // in production), slept in cancel-checked chunks.
            if sleep_backoff_cancellable(self.backoff_delay(attempt), &mut cancelled) {
                return emitted;
            }

            let result: Result<(ConnectionState, MsProtocol<T>, u8)> = (|| {
                let mut transport = (self.factory)()?;
                transport.open().map_err(ProtocolError::Transport)?;
                let mut proto = MsProtocol::new(self.comms.clone(), transport);
                let identity = proto.identify()?;
                if !identity.matches(&self.comms) {
                    return Err(ProtocolError::SignatureMismatch {
                        reported: identity.signature,
                        expected: self.comms.signature.clone(),
                    });
                }

                // secl resync: detect reboot when counter went backwards.
                let new_secl = proto.read_secl().unwrap_or(self.last_secl);

                let connected = ConnectionState::Connected { identity };
                Ok((connected, proto, new_secl))
            })();

            match result {
                Ok((state, proto, new_secl)) => {
                    self.last_reconnect_caused_reidentify =
                        secl_regressed(self.last_secl, new_secl);
                    self.last_secl = new_secl;
                    self.protocol = Some(proto);
                    self.state = state.clone();
                    on_state(&state);
                    emitted.push(state);
                    return emitted;
                }
                Err(error @ ProtocolError::SignatureMismatch { .. }) => {
                    let failed = ConnectionState::Failed {
                        reason: error.to_string(),
                    };
                    self.state = failed.clone();
                    on_state(&failed);
                    emitted.push(failed);
                    return emitted;
                }
                Err(error) => {
                    last_error = Some(error.to_string());
                    on_failed_attempt(attempt);
                }
            }
        }

        let failed = ConnectionState::Failed {
            reason: match last_error {
                Some(detail) => format!(
                    "reconnect failed after {} attempts: {detail}",
                    self.config.max_attempts
                ),
                None => format!(
                    "reconnect failed after {} attempts",
                    self.config.max_attempts
                ),
            },
        };
        self.state = failed.clone();
        on_state(&failed);
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

/// Chunk length for cancel checks during backoff sleeps: long enough to stay
/// cheap, short enough that a cancel interrupts even the capped 30 s serial
/// backoff within ~100 ms.
const CANCEL_CHECK_INTERVAL: Duration = Duration::from_millis(100);

/// Sleep `delay` in cancel-checked chunks. Returns `true` when cancelled.
fn sleep_backoff_cancellable(delay: Duration, cancelled: &mut impl FnMut() -> bool) -> bool {
    let mut remaining = delay;
    while !remaining.is_zero() {
        if cancelled() {
            return true;
        }
        let chunk = remaining.min(CANCEL_CHECK_INTERVAL);
        std::thread::sleep(chunk);
        remaining = remaining.saturating_sub(chunk);
    }
    false
}
