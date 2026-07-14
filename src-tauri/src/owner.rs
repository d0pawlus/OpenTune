// SPDX-License-Identifier: GPL-3.0-or-later
//! §9 owner task — the single owner of the live [`Session`].
//!
//! One Tokio task owns `Option<Session>` (connection + definition + tune);
//! every IPC command is a [`Command`] message on one mpsc channel, carrying a
//! oneshot reply. This is the ARCHITECTURE §9 model that M2 deliberately
//! deferred (see `docs/notes/m2-decisions.md`): all hardware access is
//! serialized through the channel — there is no lock and no path to the
//! transport outside this task. The synchronous [`Session`] is wrapped, not
//! rewritten: blocking wire I/O (`serialport` is synchronous) runs via
//! `spawn_blocking`, moving the session in and back out, so the async loop
//! itself never blocks.
//!
//! Link recovery is the one blocking operation the loop does NOT await: the
//! serial retry budget is worth ~150 s of backoff, so [`Owner::start_recovery`]
//! fires the reconnect on the blocking pool and keeps serving commands; the
//! task settles by sending [`Command::RecoverySettled`] back through a weak
//! self-sender.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use opentune_datalog::Field;
use opentune_datalog::Log;
use opentune_model::MergePick;
#[cfg(test)]
use opentune_model::Value;
use opentune_realtime::RealtimePoller;
use recovery::RecoveryInFlight;
use tauri_specta::Event as _;
use tokio::sync::{mpsc, oneshot};

use crate::capture::{CaptureBuffer, CAPTURE_CAPACITY};
#[cfg(test)]
use crate::connection::ActiveConnection;
#[cfg(test)]
use crate::connection::ConnectSource;
use crate::connection::Session;
#[cfg(test)]
use crate::dto::{
    CaptureStatusDto, CellEditDto, LogFormatDto, LogStatsParamsDto, VeAnalysisReportDto,
};
use crate::dto::{DefinitionDto, MergePickDto};
use crate::events::{ConnectionStateEvent, TuneDirtyEvent};

const NOT_CONNECTED: &str = "not connected";
/// Session-needing commands report this — instead of the generic
/// [`NOT_CONNECTED`] — while the session is checked out for link recovery,
/// so callers can tell a transient recovery window from a real disconnect.
const RECOVERY_IN_PROGRESS: &str = "link recovery in progress";
/// M4 final-review fix wave item 4: `StartCapture` rejects with this exact
/// message when realtime polling isn't running — the capture ring is only
/// ever fed from `poll_tick`, which only fires while `polling` is true (see
/// `wants_poll`), so arming a capture without it would silently never fill.
const POLLING_NOT_RUNNING: &str = "realtime polling is not running";

pub use command::Command;
#[cfg(test)]
pub(crate) use command::DebugOwnerState;
use command::OwnerEvent;
use command::RecoveryOutcome;
pub use command::Reply;
use logging::ActiveLog;
pub(crate) use logging::NO_ACTIVE_LOG;
#[cfg(test)]
use logging::{read_log_path, write_log_path, MAX_LOG_FILE_BYTES};

/// The owner's event sink, callable from the blocking pool.
pub type Emitter = Arc<dyn Fn(OwnerEvent) + Send + Sync>;

/// The managed Tauri state: the command sender into the owner task.
pub type OwnerHandle = mpsc::Sender<Command>;

const OWNER_GONE: &str = "owner task gone";

/// Send one command to the owner and await its oneshot reply — the shared
/// body of every thin IPC command. A dropped reply (owner gone/panicked
/// mid-shutdown) maps to an error instead of hanging the caller.
pub async fn request<T>(
    owner: &OwnerHandle,
    make: impl FnOnce(Reply<T>) -> Command,
) -> Result<T, String> {
    let (tx, rx) = oneshot::channel();
    owner
        .send(make(tx))
        .await
        .map_err(|_| OWNER_GONE.to_owned())?;
    rx.await.map_err(|_| OWNER_GONE.to_owned())?
}

/// Spawn the owner task wired to real IPC event emission.
pub fn spawn_owner(app: tauri::AppHandle) -> OwnerHandle {
    let emit: Emitter = Arc::new(move |ev| match ev {
        OwnerEvent::Connection(e) => {
            let _ = e.emit(&app);
        }
        OwnerEvent::TuneDirty(e) => {
            let _ = e.emit(&app);
        }
        OwnerEvent::Realtime(e) => {
            let _ = e.emit(&app);
        }
    });
    spawn_owner_with_emitter(emit)
}

/// Spawn the owner task with an injected event sink (testable core).
pub fn spawn_owner_with_emitter(emit: Emitter) -> OwnerHandle {
    let (tx, rx) = mpsc::channel(32);
    // The owner gets a WEAK self-sender (for `RecoverySettled`): an in-flight
    // recovery task must never keep the command channel — and therefore the
    // owner task — alive past app shutdown.
    tauri::async_runtime::spawn(run_owner(rx, tx.downgrade(), emit));
    tx
}

/// The owner-driven poll cadence: 25 Hz. UI emission is separately coalesced
/// to ≤30 Hz by the [`RealtimePoller`]'s 33 ms gate.
const POLL_INTERVAL: Duration = Duration::from_millis(40);
/// Idle-link health cadence. Realtime polling itself is the health probe while
/// active; this tick keeps M1 reconnect working before/without live gauges.
const HEALTH_INTERVAL: Duration = Duration::from_secs(1);

/// The owner's private state: the session it exclusively owns plus the
/// realtime polling state (Task 6). `poller` holds the ≤30 Hz emit gate;
/// it exists exactly while `polling` is set. `capture`/`capturing` (Task 8)
/// tap the same poll tick to feed the VE-analysis ring buffer; `capture`
/// keeps its rows after `capturing` flips off (`StopCapture` only clears the
/// flag) so `run_ve_analyze` can still read them.
struct Owner {
    session: Option<Session>,
    polling: bool,
    poller: Option<RealtimePoller>,
    capture: Option<CaptureBuffer>,
    capturing: bool,
    active_log: Option<ActiveLog>,
    opened_log: Option<Log>,
    /// Generation token for the current `opened_log` (M5 review CRITICAL —
    /// C2). Starts at `0`, a value no `open_log`/`stop_log` ever hands out
    /// (see [`Owner::assign_opened_log`]), so a `log_id` of `0` never
    /// matches an actual opened log. Bumped every time `opened_log` is
    /// assigned; every command reading `opened_log` must echo the id it was
    /// given back, checked by [`Owner::check_log_id`].
    log_generation: u32,
    /// Set while a link recovery runs on the blocking pool. The session is
    /// checked out for the whole window (`self.session` is `None`, which also
    /// keeps `wants_poll`/`wants_health_check` quiet), and it comes back via
    /// [`Command::RecoverySettled`] → [`Owner::finish_recovery`].
    reconnecting: Option<RecoveryInFlight>,
    /// Weak self-sender for `RecoverySettled` (see [`spawn_owner_with_emitter`]).
    self_tx: mpsc::WeakSender<Command>,
    emit: Emitter,
    #[cfg(test)]
    fail_next_reboot_tune_read: bool,
    #[cfg(test)]
    health_checks_suspended: bool,
    #[cfg(test)]
    panic_next_health_check: bool,
    #[cfg(test)]
    panic_next_recovery: bool,
}

async fn run_owner(
    mut rx: mpsc::Receiver<Command>,
    self_tx: mpsc::WeakSender<Command>,
    emit: Emitter,
) {
    let mut owner = Owner {
        session: None,
        polling: false,
        poller: None,
        capture: None,
        capturing: false,
        active_log: None,
        opened_log: None,
        log_generation: 0,
        reconnecting: None,
        self_tx,
        emit,
        #[cfg(test)]
        fail_next_reboot_tune_read: false,
        #[cfg(test)]
        health_checks_suspended: false,
        #[cfg(test)]
        panic_next_health_check: false,
        #[cfg(test)]
        panic_next_recovery: false,
    };
    let mut tick = tokio::time::interval(POLL_INTERVAL);
    let mut health_tick = tokio::time::interval(HEALTH_INTERVAL);
    // While polling is off (or a long command blocks the loop), ticks pile
    // up unobserved — skip them instead of bursting to "catch up".
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    health_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            // `biased` + commands first: a pending command (write/burn/…)
            // always preempts a poll tick, so realtime traffic never delays
            // or interleaves with user-initiated wire operations.
            biased;
            cmd = rx.recv() => match cmd {
                Some(cmd) => owner.serve(cmd).await,
                None => break,
            },
            _ = tick.tick(), if owner.wants_poll() => owner.poll_tick().await,
            _ = health_tick.tick(), if owner.wants_health_check() => owner.health_tick().await,
        }
    }
    // App/task shutdown must not silently discard an in-progress recording.
    if owner.active_log.is_some() {
        let _ = owner.stop_log().await;
    }
}

impl Owner {
    /// Serve one command: run the matching synchronous [`Session`] method on
    /// the blocking pool, emit any events, then answer the oneshot. Replies
    /// are always sent from here — never from inside a blocking closure — so
    /// a panicked operation still answers its caller with an error.
    async fn serve(&mut self, cmd: Command) {
        match cmd {
            Command::Connect { source, reply } => {
                let _ = reply.send(self.connect(source).await);
            }
            Command::Disconnect { reply } => {
                // Cancel an in-flight recovery first: the blocking reconnect
                // loop observes the flag within ~100 ms and settles without a
                // terminal `Failed` (the `Disconnected` emitted below owns
                // the UI state). The session itself is still out on the
                // blocking pool — `finish_recovery` applies the retention
                // rule below when it comes back.
                if let Some(inflight) = self.reconnecting.as_mut() {
                    inflight.cancel.store(true, Ordering::Relaxed);
                    inflight.discard = true;
                }
                // M5 review M3: a flush failure must not read back as the
                // *disconnect* having failed — every step below runs
                // unconditionally regardless of `flush_error`, and the reply
                // (built after) names the two outcomes distinctly instead of
                // handing the raw flush error back as if it were a
                // disconnect error.
                let flush_error = if self.active_log.is_some() {
                    self.stop_log().await.err()
                } else {
                    None
                };
                // An offline-origin tune must SURVIVE disconnect (design spec
                // §"Disconnect while editing"): drop the live link but keep the
                // session so the tune stays editable/saveable in offline mode.
                // An online (FRESH-read) tune is destroyed as before, so a
                // later connect FRESH-reads rather than ATTACHing a stale tune.
                match self.session.take() {
                    Some(mut s) if s.offline_origin && s.tune.is_some() => {
                        s.conn = None;
                        self.session = Some(s);
                    }
                    _ => {} // online (or empty) session: dropped by `take`
                }
                // Realtime is explicit-start only — a fresh connection must
                // not silently resume a previous session's polling.
                self.polling = false;
                self.poller = None;
                // A fresh session never inherits a previous session's
                // capture either (same M3 polling rule, Task 8).
                self.capture = None;
                self.capturing = false;
                (self.emit)(OwnerEvent::Connection(ConnectionStateEvent::Disconnected));
                let reply_result = match flush_error {
                    Some(error) => {
                        eprintln!(
                            "disconnect: log flush failed while stopping the active log: {error}"
                        );
                        Err(format!(
                            "the device was disconnected, but the log flush failed: {error}"
                        ))
                    }
                    None => Ok(()),
                };
                let _ = reply.send(reply_result);
            }
            Command::SimulateLinkDrop { reply } => {
                let _ = reply.send(self.simulate_link_drop().await);
            }
            Command::RecoverySettled { session, outcome } => {
                self.finish_recovery(session.map(|boxed| *boxed), outcome);
            }
            Command::GetDefinition { reply } => {
                let _ = reply.send(self.with_session(|s| Ok(s.definition())).await);
            }
            Command::LoadTune { reply } => {
                let r = self.with_session(Session::load_tune).await;
                self.emit_dirty(&r);
                let _ = reply.send(r);
            }
            Command::GetValues { names, reply } => {
                let r = self.with_session(move |s| s.read_values(&names)).await;
                let _ = reply.send(r);
            }
            Command::SetValue { name, value, reply } => {
                let r = self.with_session(move |s| s.set_value(&name, value)).await;
                self.emit_dirty(&r);
                let _ = reply.send(r);
            }
            Command::SetCells { name, cells, reply } => {
                let r = self
                    .with_session(move |s| {
                        let cells: Vec<(u32, f64)> =
                            cells.iter().map(|c| (c.index, c.value)).collect();
                        s.set_cells(&name, &cells)
                    })
                    .await;
                self.emit_dirty(&r);
                let _ = reply.send(r);
            }
            Command::Burn { reply } => {
                let r = self.with_session(Session::burn).await;
                self.emit_dirty(&r);
                let _ = reply.send(r);
            }
            Command::Undo { reply } => {
                let r = self.with_session(Session::undo).await;
                self.emit_dirty(&r);
                let _ = reply.send(r);
            }
            Command::Redo { reply } => {
                let r = self.with_session(Session::redo).await;
                self.emit_dirty(&r);
                let _ = reply.send(r);
            }
            Command::EvalConditions { exprs, reply } => {
                let r = self.with_session(move |s| s.eval_conditions(&exprs)).await;
                let _ = reply.send(r);
            }
            Command::SnapshotTune { reply } => {
                let _ = reply.send(self.with_session(Session::snapshot_tune).await);
            }
            Command::DiffTune { reply } => {
                let _ = reply.send(self.with_session(|s| s.diff_tune()).await);
            }
            Command::MergeTune { picks, reply } => {
                let _ = reply.send(self.merge_tune(picks).await);
            }
            Command::ResolveGaugeBounds { reply } => {
                let _ = reply.send(self.with_session(|s| s.resolve_gauge_bounds()).await);
            }
            Command::StartRealtime { reply } => {
                // A live link is required, not just a session: an offline
                // session (`conn: None`) or one whose reconnect just failed
                // must not arm polling against a dead/absent link. Same guard
                // as `wants_health_check`.
                let r = if matches!(&self.session, Some(Session { conn: Some(_), .. })) {
                    self.polling = true;
                    self.poller = Some(RealtimePoller::default());
                    Ok(())
                } else {
                    self.polling = false;
                    self.poller = None;
                    Err(format!(
                        "{} — cannot start realtime",
                        self.not_connected_error()
                    ))
                };
                let _ = reply.send(r);
            }
            Command::StopRealtime { reply } => {
                self.polling = false;
                if self.active_log.is_none() {
                    self.poller = None;
                }
                let _ = reply.send(Ok(()));
            }
            // Start a fresh capture ring, pinned to the session's declared
            // output channels in declaration order (Task 8).
            Command::StartCapture { reply } => {
                let r = match &self.session {
                    Some(s) if self.polling => {
                        let columns: Vec<String> = s
                            .def
                            .output_channels
                            .iter()
                            .map(|c| c.name().to_string())
                            .collect();
                        self.capture = Some(CaptureBuffer::new(columns, CAPTURE_CAPACITY));
                        self.capturing = true;
                        Ok(())
                    }
                    Some(_) => Err(POLLING_NOT_RUNNING.to_owned()),
                    None => Err(self.not_connected_error()),
                };
                let _ = reply.send(r);
            }
            // Stop capturing; the rows stay for `run_ve_analyze` (Task 11) —
            // only the flag clears (Task 8).
            Command::StopCapture { reply } => {
                self.capturing = false;
                let r = self
                    .capture
                    .as_ref()
                    .map(|b| b.status(false))
                    .ok_or_else(|| "no capture".to_string());
                let _ = reply.send(r);
            }
            Command::CaptureStatus { reply } => {
                let r = self
                    .capture
                    .as_ref()
                    .map(|b| b.status(self.capturing))
                    .ok_or_else(|| "no capture".to_string());
                let _ = reply.send(r);
            }
            // Run the deterministic VE-analysis engine (M4 Task 11) against
            // the current capture. The bridge is pure compute over ≤27k rows
            // (a few ms) — it runs inline, not on the blocking pool (that
            // rule exists for wire/disk I/O, which this never touches).
            Command::RunVeAnalyze { table, reply } => {
                let r = match (&self.session, &self.capture) {
                    (Some(s), Some(buf)) => {
                        let samples = buf.to_sample_set();
                        s.tune
                            .as_ref()
                            .ok_or_else(|| "no tune loaded".to_string())
                            .and_then(|t| {
                                crate::analysis_bridge::run_ve_analyze(&s.def, t, &samples, &table)
                            })
                    }
                    (None, _) => Err(self.not_connected_error()),
                    (_, None) => Err("no capture — start a capture first".to_string()),
                };
                let _ = reply.send(r);
            }
            Command::StartLog {
                path,
                format,
                reply,
            } => {
                let _ = reply.send(self.start_log(path, format));
            }
            Command::StopLog { reply } => {
                let _ = reply.send(self.stop_log().await);
            }
            Command::AddLogMarker { text, reply } => {
                let _ = reply.send(self.add_log_marker(text));
            }
            Command::LogStatus { reply } => {
                let _ = reply.send(Ok(self.log_status()));
            }
            Command::OpenLog {
                path,
                format,
                reply,
            } => {
                let _ = reply.send(self.open_log(path, format).await);
            }
            Command::GetLogData {
                log_id,
                offset,
                limit,
                reply,
            } => {
                let r = self
                    .check_log_id(log_id)
                    .and_then(|()| {
                        self.opened_log
                            .as_ref()
                            .ok_or_else(|| "no log opened".to_string())
                    })
                    .and_then(|log| crate::log_bridge::slice(log, offset, limit));
                let _ = reply.send(r);
            }
            Command::SaveLog {
                log_id,
                path,
                format,
                reply,
            } => {
                let _ = reply.send(self.save_log(log_id, path, format).await);
            }
            Command::LogStats {
                log_id,
                params,
                reply,
            } => {
                let _ = reply.send(self.run_log_stats(log_id, params).await);
            }
            Command::DetectAnomaly {
                log_id,
                thresholds,
                reply,
            } => {
                let _ = reply.send(self.run_detect_anomaly(log_id, thresholds).await);
            }
            Command::VirtualDyno {
                log_id,
                params,
                reply,
            } => {
                let _ = reply.send(self.run_virtual_dyno(log_id, params).await);
            }
            #[cfg(test)]
            Command::DebugSimulator { reply } => {
                let r = match &self.session {
                    Some(Session {
                        conn: Some(ActiveConnection::Sim { simulator, .. }),
                        ..
                    }) => Ok(Arc::clone(simulator)),
                    Some(_) => Err("not a simulator connection".to_owned()),
                    None => Err(self.not_connected_error()),
                };
                let _ = reply.send(r);
            }
            Command::NewTune { ini_path, reply } => {
                let _ = reply.send(self.new_tune(ini_path).await);
            }
            Command::OpenTune {
                ini_path,
                msq_path,
                reply,
            } => {
                let _ = reply.send(self.open_tune(ini_path, msq_path).await);
            }
            Command::SaveTune { path, reply } => {
                let r = self
                    .with_session(move |s| {
                        let tune = s
                            .tune
                            .as_ref()
                            .ok_or_else(|| crate::session::NO_TUNE.to_string())?;
                        let xml = opentune_project::msq::tune_to_msq(tune);
                        std::fs::write(&path, xml)
                            .map_err(|e| format!("cannot write `{path}`: {e}"))
                    })
                    .await;
                let _ = reply.send(r);
            }
            Command::WriteTuneToEcu { reply } => {
                let r = self.with_session(Session::write_all_to_ecu).await;
                self.emit_dirty(&r);
                let _ = reply.send(r.map(|_| ()));
            }
            #[cfg(test)]
            Command::DebugPanicSessionOperation { reply } => {
                let r = self
                    .with_session(|_| -> Result<(), String> {
                        panic!("forced session operation panic")
                    })
                    .await;
                let _ = reply.send(r);
            }
            #[cfg(test)]
            Command::DebugPanicNextHealthCheck { reply } => {
                self.panic_next_health_check = true;
                let _ = reply.send(Ok(()));
            }
            #[cfg(test)]
            Command::DebugPanicNextRecovery { reply } => {
                self.panic_next_recovery = true;
                let _ = reply.send(Ok(()));
            }
            #[cfg(test)]
            Command::DebugHoldSessionOperation {
                started,
                release,
                reply,
            } => {
                let r = self
                    .with_session(move |_| {
                        let _ = started.send(());
                        release
                            .recv()
                            .map_err(|_| "test session-operation release dropped".to_owned())
                    })
                    .await;
                let _ = reply.send(r);
            }
            #[cfg(test)]
            Command::DebugFailNextRebootTuneRead { reply } => {
                self.fail_next_reboot_tune_read = true;
                let _ = reply.send(Ok(()));
            }
            #[cfg(test)]
            Command::DebugSuspendHealthChecks { reply } => {
                self.health_checks_suspended = true;
                let _ = reply.send(Ok(()));
            }
            #[cfg(test)]
            Command::DebugState { reply } => {
                let state = DebugOwnerState {
                    session_present: self.session.is_some(),
                    tune_loaded: self
                        .session
                        .as_ref()
                        .is_some_and(|session| session.tune.is_some()),
                    snapshot_present: self
                        .session
                        .as_ref()
                        .is_some_and(|session| session.snapshot.is_some()),
                    polling: self.polling,
                    poller_present: self.poller.is_some(),
                    recovering: self.reconnecting.is_some(),
                };
                let _ = reply.send(Ok(state));
            }
        }
    }

    /// Emit the tune-dirty event carried by a successful mutating op.
    fn emit_dirty(&self, r: &Result<TuneDirtyEvent, String>) {
        if let Ok(ev) = r {
            (self.emit)(OwnerEvent::TuneDirty(ev.clone()));
        }
    }

    /// True while acquisition has a consumer. UI realtime and datalogging are
    /// independent: stopping dashboard updates must never stop an active log.
    fn wants_poll(&self) -> bool {
        self.session.is_some() && (self.polling || self.active_log.is_some())
    }

    /// Merge picks, then emit the tune's *actual* dirty state — read after
    /// the merge attempt regardless of `Ok`/`Err`, because a merge can abort
    /// mid-batch after earlier picks already committed (M2 behavior).
    async fn merge_tune(
        &mut self,
        picks: Vec<MergePickDto>,
    ) -> Result<Option<TuneDirtyEvent>, String> {
        let picks: Vec<MergePick> = picks.into_iter().map(MergePick::from).collect();
        let (result, event) = self
            .with_session(move |s| {
                let result = s.merge_picks(&picks);
                Ok((result, s.current_dirty_event()))
            })
            .await?;
        if let Some(ev) = &event {
            (self.emit)(OwnerEvent::TuneDirty(ev.clone()));
        }
        result.map(|_| event)
    }

    /// Tear down the current session and any realtime polling state — the
    /// shared body of `new_tune`/`open_tune`: an offline session never
    /// inherits a previous session's live link or polling (same rule as
    /// `Disconnect`/`connect`).
    ///
    /// If the torn-down session held a live link, emit `Disconnected` so the
    /// UI doesn't keep showing a false "connected" after e.g. creating a new
    /// offline tune while connected.
    fn reset_session(&mut self) {
        // A recovery in flight means the (checked-out) session had a live
        // link. Cancel it — its settled session is stale next to the
        // replacement built here, and `finish_recovery`'s occupied-seat
        // guard drops it on arrival.
        let recovering = self.reconnecting.is_some();
        if let Some(inflight) = self.reconnecting.as_mut() {
            inflight.cancel.store(true, Ordering::Relaxed);
        }
        let had_link = recovering || matches!(&self.session, Some(s) if s.conn.is_some());
        self.session = None;
        self.polling = false;
        self.poller = None;
        // A replaced session never inherits a previous session's capture
        // (same M3 polling rule, Task 8) — matches the Disconnect teardown.
        self.capture = None;
        self.capturing = false;
        if had_link {
            (self.emit)(OwnerEvent::Connection(ConnectionStateEvent::Disconnected));
        }
    }

    /// Build a blank offline tune from `ini_path` and make it the current
    /// session. Built off-loop first — a bad INI must not wipe the user's
    /// current session before the replacement is known to succeed.
    async fn new_tune(&mut self, ini_path: String) -> Result<DefinitionDto, String> {
        let session = tokio::task::spawn_blocking(move || ops::build_offline_session(&ini_path))
            .await
            .map_err(|e| format!("new_tune panicked: {e}"))??;
        let dto = DefinitionDto::from(session.def.as_ref());
        self.reset_session();
        self.session = Some(session);
        Ok(dto)
    }

    /// Build an offline tune from `ini_path` with a `.msq` loaded into it,
    /// and make it the current session. Same build-first ordering as
    /// [`Self::new_tune`].
    async fn open_tune(
        &mut self,
        ini_path: String,
        msq_path: String,
    ) -> Result<DefinitionDto, String> {
        let session = tokio::task::spawn_blocking(move || {
            ops::build_offline_session_from_msq(&ini_path, &msq_path)
        })
        .await
        .map_err(|e| format!("open_tune panicked: {e}"))??;
        let dto = DefinitionDto::from(session.def.as_ref());
        self.reset_session();
        self.session = Some(session);
        Ok(dto)
    }
}

#[path = "owner_command.rs"]
mod command;

#[path = "owner_recovery.rs"]
mod recovery;

#[path = "owner_logging.rs"]
mod logging;

#[path = "owner_ops.rs"]
mod ops;

#[cfg(test)]
#[path = "owner_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "owner_analysis_tests.rs"]
mod analysis_tests;

#[cfg(test)]
#[path = "owner_log_tests.rs"]
mod log_tests;
