// SPDX-License-Identifier: GPL-3.0-or-later
//! Connection lifecycle for the §9 owner task — connect/attach, the idle-link
//! health probe, the 25 Hz poll tick, and M1 link recovery. Split from
//! `owner.rs` for file cohesion; these methods run against [`super::Owner`]'s
//! private session/polling state the same as everything left in the parent.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::ops::{attach_connection, build_session, link_drop, reconnect_session};
use super::{Command, Owner, OwnerEvent, RecoveryOutcome, NOT_CONNECTED, RECOVERY_IN_PROGRESS};
use crate::connection::{ConnectSource, Session};
use crate::events::{ConnectionStateEvent, RealtimeFrameEvent};
use crate::session::PollFrameError;

/// Owner-side handle on a recovery running on the blocking pool.
pub(super) struct RecoveryInFlight {
    /// Cooperative cancel flag; the reconnect loop checks it between attempts
    /// and backoff chunks (~100 ms granularity), so a user's Disconnect
    /// interrupts even the capped 30 s serial backoff promptly.
    pub(super) cancel: Arc<AtomicBool>,
    /// Set when the user disconnected while the recovery was in flight: the
    /// settled session is then subject to the Disconnect retention rule
    /// (offline-origin tune survives without a link) instead of being
    /// restored, and no further event is emitted — Disconnect already
    /// emitted `Disconnected`.
    pub(super) discard: bool,
}

impl Owner {
    /// Probe an idle live link. During realtime, `poll_tick` is already the
    /// probe, so this avoids sending an extra output-channel request.
    pub(super) fn wants_health_check(&self) -> bool {
        #[cfg(test)]
        if self.health_checks_suspended {
            return false;
        }
        !self.polling && matches!(&self.session, Some(Session { conn: Some(_), .. }))
    }

    pub(super) async fn health_tick(&mut self) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        #[cfg(test)]
        let force_panic = std::mem::take(&mut self.panic_next_health_check);
        #[cfg(not(test))]
        let force_panic = false;
        let result = tokio::task::spawn_blocking(move || {
            if force_panic {
                panic!("forced health check panic");
            }
            let result = session.check_link();
            (session, result)
        })
        .await;
        match result {
            Ok((session, Ok(()))) => self.session = Some(session),
            Ok((session, Err(_))) => {
                self.session = Some(session);
                self.start_recovery();
            }
            // Panicked mid-probe: the session moved into the task and is
            // lost. The uniform cleanup clears the poller and emits
            // `Disconnected` — `polling = false` alone left the UI stuck
            // on "Connected" over a dead seat (M2/M3 re-review finding 5).
            Err(_) => self.lose_session_after_panic(),
        }
    }

    /// One 25 Hz poll tick: run [`Session::poll_frame`] on the blocking pool
    /// (it touches the wire — same `spawn_blocking` move-in/move-out pattern
    /// as [`Self::with_session`], extended to carry the poller's gate state),
    /// and emit [`RealtimeFrameEvent`] when a coalesced frame comes back.
    ///
    /// A failed poll is dropped, not fatal (fail-open): the wire may be
    /// mid-glitch or the INI may declare no och block — the next tick simply
    /// tries again, and stopping is always the user's explicit command.
    pub(super) async fn poll_tick(&mut self) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        let mut poller = self.poller.take().unwrap_or_default();
        match tokio::task::spawn_blocking(move || {
            let r = session.poll_frame_full(&mut poller);
            (session, poller, r)
        })
        .await
        {
            Ok((session, poller, r)) => {
                self.session = Some(session);
                self.poller = Some(poller);
                let link_failed = matches!(&r, Err(PollFrameError::Link(_)));
                if let Ok(Some((frame, emit_to_ui))) = r {
                    // Owner-side consumers see every acquired frame, before
                    // the UI's ≤30 Hz coalescing decision.
                    if self.capturing {
                        if let Some(buf) = self.capture.as_mut() {
                            buf.push(&frame);
                        }
                    }
                    if let Some(active) = self.active_log.as_mut() {
                        active.push(&frame);
                    }
                    if emit_to_ui && self.polling {
                        let channels = frame
                            .channels
                            .into_iter()
                            .map(|c| (c.name, c.value))
                            .collect();
                        (self.emit)(OwnerEvent::Realtime(RealtimeFrameEvent { channels }));
                    }
                }
                if link_failed {
                    self.start_recovery();
                }
            }
            // Panicked mid-poll: the session is lost (poisoned-equivalent,
            // same as `with_session`); subsequent commands report
            // "not connected" and the poll gate stays disarmed.
            Err(_) => self.lose_session_after_panic(),
        }
    }

    /// Run `f` against the owned session on the blocking pool (serial I/O is
    /// synchronous), moving the session in and back out. If the closure
    /// panics the session is lost (poisoned-equivalent) and the caller gets
    /// an error; subsequent commands report "not connected".
    pub(super) async fn with_session<T: Send + 'static>(
        &mut self,
        f: impl FnOnce(&mut Session) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let Some(mut session) = self.session.take() else {
            return Err(self.not_connected_error());
        };
        match tokio::task::spawn_blocking(move || {
            let r = f(&mut session);
            (session, r)
        })
        .await
        {
            Ok((session, r)) => {
                self.session = Some(session);
                r
            }
            Err(e) => {
                let error = format!("session operation panicked: {e}");
                self.lose_session_after_panic();
                Err(error)
            }
        }
    }

    /// A panicked blocking operation cannot safely hand its moved session
    /// back. Fully disarm realtime and tell the UI that the live link is gone.
    pub(super) fn lose_session_after_panic(&mut self) {
        self.session = None;
        self.polling = false;
        self.poller = None;
        (self.emit)(OwnerEvent::Connection(ConnectionStateEvent::Disconnected));
    }

    /// Connect to an ECU. Branches on whether an offline tune is already
    /// loaded:
    /// - **ATTACH**: an offline session (`conn: None`, `tune: Some`) is kept
    ///   as-is — only the live link is added, after the ECU's signature is
    ///   verified against the offline tune's INI. The user's offline edits
    ///   are never overwritten by a read.
    /// - **FRESH**: no offline tune is loaded (or the session already has a
    ///   connection) — tear down and build a brand-new session, reading the
    ///   tune from the ECU (unchanged M2/M3 behavior).
    ///
    /// `Connecting`/`Connected` are emitted from inside the handshake so a
    /// slow serial connect still shows live progress either way.
    pub(super) async fn connect(&mut self, source: ConnectSource) -> Result<(), String> {
        // Safety net against resurrection races: while a recovery is in
        // flight its (possibly stale) session will settle back soon — a
        // fresh connect now could be silently clobbered or clobber it. The
        // UI disables Connect during reconnecting anyway.
        if self.reconnecting.is_some() {
            return Err(format!("{RECOVERY_IN_PROGRESS} — disconnect first"));
        }
        if self.active_log.is_some() {
            self.stop_log().await?;
        }
        // ATTACH: an offline tune is loaded — keep it, just add the live link
        // (never overwrite the user's unsaved offline edits with an ECU read).
        if matches!(&self.session, Some(s) if s.conn.is_none() && s.tune.is_some()) {
            let mut session = self.session.take().expect("checked by matches! above");
            let emit = Arc::clone(&self.emit);
            // Always hand the session back (tuple pattern, same as
            // `with_session`/`simulate_link_drop`): a *failed* attach — the
            // signature guard rejecting a mismatched ECU, or `connect_serial`
            // erroring on a bad port — must leave the user's unsaved offline
            // tune intact, never destroyed. Only a genuine task panic loses it.
            let (session, r) = match tokio::task::spawn_blocking(move || {
                let r = attach_connection(&mut session, source, &emit);
                (session, r)
            })
            .await
            {
                Ok(settled) => settled,
                Err(e) => {
                    // The offline tune moved into the panicked task and is
                    // lost; the uniform cleanup also corrects the UI back to
                    // disconnected in case the attach panicked after already
                    // emitting `Connected` (M2/M3 re-review finding 5).
                    let error = format!("attach panicked: {e}");
                    self.lose_session_after_panic();
                    return Err(error);
                }
            };
            self.session = Some(session);
            // A refused attach may have already emitted `Connected` (serial
            // `connect_serial` connects before the signature guard rejects) —
            // correct the UI back to disconnected so it doesn't stay stuck on
            // a false "Connected". The offline tune itself survived above.
            if r.is_err() {
                (self.emit)(OwnerEvent::Connection(ConnectionStateEvent::Disconnected));
            }
            return r;
        }
        // FRESH: no offline tune — tear down (incl. polling/capture) and build
        // a brand-new session, reading the tune from the ECU.
        self.reset_session();
        let emit = Arc::clone(&self.emit);
        let session = match tokio::task::spawn_blocking(move || build_session(source, &emit)).await
        {
            Ok(result) => result?,
            Err(e) => {
                let error = format!("connect panicked: {e}");
                self.lose_session_after_panic();
                return Err(error);
            }
        };
        self.session = Some(session);
        Ok(())
    }

    /// Simulator-only: drop the link and drive M1's reconnect loop, emitting
    /// each state. Runs on the blocking pool; the session (definition + tune
    /// preserved) is put back when the reconnect settles.
    pub(super) async fn simulate_link_drop(&mut self) -> Result<(), String> {
        let Some(session) = self.session.take() else {
            return Err(self.not_connected_error());
        };
        let emit = Arc::clone(&self.emit);
        #[cfg(test)]
        let fail_tune_reread = std::mem::take(&mut self.fail_next_reboot_tune_read);
        #[cfg(not(test))]
        let fail_tune_reread = false;
        let (session, r) =
            match tokio::task::spawn_blocking(move || link_drop(session, &emit, fail_tune_reread))
                .await
            {
                Ok(result) => result,
                Err(e) => {
                    let error = format!("link drop panicked: {e}");
                    self.lose_session_after_panic();
                    return Err(error);
                }
            };
        self.session = session;
        r
    }

    /// Begin recovery after a real health/poll failure — WITHOUT blocking the
    /// command loop. The serial retry budget is worth ~150 s of backoff, so
    /// the reconnect runs fire-and-forget on the blocking pool while the
    /// owner keeps serving commands (a user's Disconnect must not wait out
    /// the whole schedule). While in flight, `self.session` is `None` and
    /// `self.reconnecting` is `Some`, so the poll/health gates stay quiet and
    /// session commands answer [`RECOVERY_IN_PROGRESS`]. The task settles by
    /// sending [`Command::RecoverySettled`]; a successful reconnect keeps
    /// realtime armed so frames resume automatically.
    pub(super) fn start_recovery(&mut self) {
        // One recovery at a time. The gates cannot fire while the session is
        // out, so this is purely defensive.
        if self.reconnecting.is_some() {
            return;
        }
        let Some(session) = self.session.take() else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.reconnecting = Some(RecoveryInFlight {
            cancel: Arc::clone(&cancel),
            discard: false,
        });
        #[cfg(test)]
        let fail_tune_reread = std::mem::take(&mut self.fail_next_reboot_tune_read);
        #[cfg(not(test))]
        let fail_tune_reread = false;
        #[cfg(test)]
        let force_panic = std::mem::take(&mut self.panic_next_recovery);
        #[cfg(not(test))]
        let force_panic = false;
        let emit = Arc::clone(&self.emit);
        let self_tx = self.self_tx.clone();
        tokio::task::spawn_blocking(move || {
            // A panicking recovery must still settle — an unanswered
            // `reconnecting` would block Connect/Disconnect forever.
            let settled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if force_panic {
                    panic!("forced recovery panic");
                }
                reconnect_session(session, &emit, &cancel, fail_tune_reread)
            }));
            let (session, outcome) = settled.unwrap_or_else(|panic| {
                let reason = format!("recovery panicked: {}", panic_text(panic.as_ref()));
                // The reconnect loop died mid-stream, so no terminal state
                // ever reached the UI — without this emit it would sit on
                // "Reconnecting {n}" forever (M2/M3 re-review finding 5).
                emit(OwnerEvent::Connection(ConnectionStateEvent::Failed {
                    reason: reason.clone(),
                }));
                (None, RecoveryOutcome::Failed(reason))
            });
            // A failed upgrade means the owner (and app) shut down while the
            // recovery ran — there is no one to settle with; drop everything.
            if let Some(tx) = self_tx.upgrade() {
                let _ = tx.blocking_send(Command::RecoverySettled {
                    session: session.map(Box::new),
                    outcome,
                });
            }
        });
    }

    /// Apply a settled recovery ([`Command::RecoverySettled`]).
    ///
    /// No event is emitted from here: every connection state — including the
    /// terminal `Failed` — was already streamed live by the reconnect loop,
    /// and the discard path's `Disconnected` was emitted by Disconnect itself.
    pub(super) fn finish_recovery(&mut self, session: Option<Session>, outcome: RecoveryOutcome) {
        let Some(inflight) = self.reconnecting.take() else {
            // Stale settle (no recovery is tracked): never resurrect the
            // session it carries.
            return;
        };
        // The seat was re-occupied while the recovery was in flight
        // (`new_tune`/`open_tune` replaced the session): the user's fresh
        // session wins; the settled one is stale and dropped.
        if self.session.is_some() {
            return;
        }
        if inflight.discard {
            self.retain_offline_tune_only(session);
            return;
        }
        match outcome {
            RecoveryOutcome::Connected => self.session = session,
            RecoveryOutcome::ConnectedButTuneRereadFailed(_) => {
                // The link IS live — keep the session (transport intact,
                // tune/snapshot already cleared) but stop polling: there is
                // no current tune for frames to be meaningful against.
                self.session = session;
                self.polling = false;
            }
            // Give-up terminates the M1 retry storm: with no live conn left
            // in the seat, `wants_health_check` stays quiet instead of
            // launching the next ~150 s cycle one second later. `Cancelled`
            // without `discard` cannot normally happen; treat it as the same
            // give-up (no events either way).
            RecoveryOutcome::Failed(_) | RecoveryOutcome::Cancelled => {
                self.polling = false;
                self.retain_offline_tune_only(session);
            }
        }
    }

    /// The Disconnect retention rule (design spec §"Disconnect while
    /// editing") applied to a session returning from a dead-link recovery:
    /// an offline-origin tune survives, editable/saveable, with the link
    /// dropped; any other session is destroyed so a later connect
    /// FRESH-reads instead of ATTACHing a stale online tune.
    pub(super) fn retain_offline_tune_only(&mut self, session: Option<Session>) {
        match session {
            Some(mut s) if s.offline_origin && s.tune.is_some() => {
                s.conn = None;
                self.session = Some(s);
            }
            _ => {}
        }
    }

    /// The `NOT_CONNECTED`-class error for the current owner state: a
    /// distinct diagnostic while the session is merely checked out for link
    /// recovery.
    pub(super) fn not_connected_error(&self) -> String {
        if self.reconnecting.is_some() {
            RECOVERY_IN_PROGRESS.to_owned()
        } else {
            NOT_CONNECTED.to_owned()
        }
    }
}

/// Best-effort text of a caught panic payload (the standard `&str`/`String`
/// shapes; anything else degrades to a fixed marker).
fn panic_text(panic: &(dyn std::any::Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-string panic payload")
}
