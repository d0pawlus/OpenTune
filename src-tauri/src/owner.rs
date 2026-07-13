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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use opentune_datalog::{Field, Log, LogEntry, Marker, Record};
use opentune_model::{MergePick, Value};
use opentune_realtime::RealtimePoller;
use tauri_specta::Event as _;
use tokio::sync::{mpsc, oneshot};

use crate::capture::{CaptureBuffer, CAPTURE_CAPACITY};
#[cfg(test)]
use crate::connection::ActiveConnection;
use crate::connection::{ConnectSource, Session};
use crate::dto::{
    AnomalyReportDto, AnomalyThresholdsDto, CaptureStatusDto, CellEditDto, DefinitionDto,
    FieldDiffDto, LogDataDto, LogFormatDto, LogStatsParamsDto, LogStatsReportDto, LogStatusDto,
    LogSummaryDto, MergePickDto, ResolvedGaugeBoundsDto, VeAnalysisReportDto, VirtualDynoParamsDto,
    VirtualDynoReportDto,
};
use crate::events::{ConnectionStateEvent, RealtimeFrameEvent, TuneDirtyEvent};
use crate::session::PollFrameError;
use ops::{build_session, link_drop, reconnect_session};

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

/// A oneshot reply channel carrying an operation's result back to the
/// awaiting IPC command.
pub type Reply<T> = oneshot::Sender<Result<T, String>>;

/// One request to the wire owner. Each carries a oneshot reply channel so the
/// async command facade can await the synchronous [`Session`] result.
pub enum Command {
    Connect {
        source: ConnectSource,
        reply: Reply<()>,
    },
    Disconnect {
        reply: Reply<()>,
    },
    SimulateLinkDrop {
        reply: Reply<()>,
    },
    /// Owner-internal: an in-flight link recovery settled. Carries no reply —
    /// nobody awaits it. Never constructed by the IPC layer (commands.rs);
    /// only [`Owner::start_recovery`]'s blocking task sends it, through the
    /// weak self-sender. Boxed so this rare variant doesn't inflate every
    /// queued command by `Session`'s size.
    RecoverySettled {
        session: Option<Box<Session>>,
        outcome: RecoveryOutcome,
    },
    GetDefinition {
        reply: Reply<DefinitionDto>,
    },
    LoadTune {
        reply: Reply<TuneDirtyEvent>,
    },
    GetValues {
        names: Vec<String>,
        reply: Reply<Vec<Value>>,
    },
    SetValue {
        name: String,
        value: Value,
        reply: Reply<TuneDirtyEvent>,
    },
    Burn {
        reply: Reply<TuneDirtyEvent>,
    },
    Undo {
        reply: Reply<TuneDirtyEvent>,
    },
    Redo {
        reply: Reply<TuneDirtyEvent>,
    },
    EvalConditions {
        exprs: Vec<String>,
        reply: Reply<Vec<bool>>,
    },
    SnapshotTune {
        reply: Reply<()>,
    },
    DiffTune {
        reply: Reply<Vec<FieldDiffDto>>,
    },
    MergeTune {
        picks: Vec<MergePickDto>,
        reply: Reply<Option<TuneDirtyEvent>>,
    },
    ResolveGaugeBounds {
        reply: Reply<Vec<ResolvedGaugeBoundsDto>>,
    },
    /// Task 6 fills the realtime handlers; for now they just flip the flag.
    StartRealtime {
        reply: Reply<()>,
    },
    StopRealtime {
        reply: Reply<()>,
    },
    /// Set flat cells of a named table's array constant (M4 Task 3).
    SetCells {
        name: String,
        cells: Vec<CellEditDto>,
        reply: Reply<TuneDirtyEvent>,
    },
    /// Start the realtime-capture ring buffer for VE analysis (M4 Task 8).
    StartCapture {
        reply: Reply<()>,
    },
    /// Stop capturing and return the final status (M4 Task 8).
    StopCapture {
        reply: Reply<CaptureStatusDto>,
    },
    /// Report the capture ring buffer's current status (M4 Task 8).
    CaptureStatus {
        reply: Reply<CaptureStatusDto>,
    },
    /// Run the deterministic VE analysis engine against the current capture
    /// for a named table (M4 Task 11).
    RunVeAnalyze {
        table: String,
        reply: Reply<VeAnalysisReportDto>,
    },
    StartLog {
        path: String,
        format: LogFormatDto,
        reply: Reply<LogStatusDto>,
    },
    StopLog {
        reply: Reply<LogSummaryDto>,
    },
    AddLogMarker {
        text: String,
        reply: Reply<()>,
    },
    LogStatus {
        reply: Reply<LogStatusDto>,
    },
    OpenLog {
        path: String,
        format: LogFormatDto,
        reply: Reply<LogSummaryDto>,
    },
    GetLogData {
        offset: u32,
        limit: u32,
        reply: Reply<LogDataDto>,
    },
    SaveLog {
        path: String,
        format: LogFormatDto,
        reply: Reply<()>,
    },
    LogStats {
        params: LogStatsParamsDto,
        reply: Reply<LogStatsReportDto>,
    },
    DetectAnomaly {
        thresholds: AnomalyThresholdsDto,
        reply: Reply<AnomalyReportDto>,
    },
    VirtualDyno {
        params: VirtualDynoParamsDto,
        reply: Reply<VirtualDynoReportDto>,
    },
    /// Test-only: hand back the live simulator so tests can drive secl /
    /// reboot scenarios (same access the M2 session tests used directly).
    #[cfg(test)]
    DebugSimulator {
        reply: Reply<Arc<opentune_simulator::EcuSimulator>>,
    },
    /// Build a fresh offline session (no ECU link) around a blank tune from
    /// `ini_path` (M4 Task 3 — offline tune lifecycle).
    NewTune {
        ini_path: String,
        reply: Reply<DefinitionDto>,
    },
    /// Build a fresh offline session and load a `.msq` into its tune
    /// (M4 Task 3 — offline tune lifecycle).
    OpenTune {
        ini_path: String,
        msq_path: String,
        reply: Reply<DefinitionDto>,
    },
    /// Serialize the current tune to `.msq` at `path` (M4 Task 3 — offline
    /// tune lifecycle).
    SaveTune {
        path: String,
        reply: Reply<()>,
    },
    /// Push the entire tune to the ECU: write every page, then burn (M4
    /// Task 4 — offline "Write to ECU"). Requires a live connection.
    WriteTuneToEcu {
        reply: Reply<()>,
    },
    /// Test-only: panic inside a session `spawn_blocking` operation.
    #[cfg(test)]
    DebugPanicSessionOperation {
        reply: Reply<()>,
    },
    /// Test-only: panic inside the next health-check `spawn_blocking` task.
    #[cfg(test)]
    DebugPanicNextHealthCheck {
        reply: Reply<()>,
    },
    /// Test-only: panic inside the next recovery's blocking task.
    #[cfg(test)]
    DebugPanicNextRecovery {
        reply: Reply<()>,
    },
    /// Test-only: hold the session operation until `release` is signalled.
    #[cfg(test)]
    DebugHoldSessionOperation {
        started: oneshot::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
        reply: Reply<()>,
    },
    /// Test-only: make the next reboot-triggered tune re-read fail.
    #[cfg(test)]
    DebugFailNextRebootTuneRead {
        reply: Reply<()>,
    },
    /// Test-only: disable the idle-link health prober. The reboot
    /// choreographies (`sim.reboot()` → `SimulateLinkDrop`) race it: a 1 s
    /// tick landing in that window reads the rebooted secl first and starts
    /// its own (now fire-and-forget) recovery, so the test's drop command
    /// answers `RECOVERY_IN_PROGRESS` instead of driving the reconnect. The
    /// health path keeps its own dedicated tests.
    #[cfg(test)]
    DebugSuspendHealthChecks {
        reply: Reply<()>,
    },
    /// Test-only: inspect owner-private safety state.
    #[cfg(test)]
    DebugState {
        reply: Reply<DebugOwnerState>,
    },
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugOwnerState {
    pub session_present: bool,
    pub tune_loaded: bool,
    pub snapshot_present: bool,
    pub polling: bool,
    pub poller_present: bool,
    pub recovering: bool,
}

/// How a link recovery settled. Richer than a `Result` because
/// [`Owner::finish_recovery`] must tell apart four distinct session
/// dispositions: a live link to restore, a live link whose tune was
/// invalidated, a dead link whose retry budget is exhausted, and a
/// user-cancelled recovery.
#[derive(Debug)]
pub enum RecoveryOutcome {
    /// Reconnected; the session's link is live again.
    Connected,
    /// The link IS live, but the post-reboot tune re-read failed: the session
    /// keeps its transport with tune/snapshot cleared, and polling stops.
    ConnectedButTuneRereadFailed(String),
    /// Attempts exhausted or signature mismatch — the session's manager holds
    /// no live protocol.
    Failed(String),
    /// The cancel flag stopped the loop; no terminal state was emitted.
    Cancelled,
}

impl RecoveryOutcome {
    /// Project onto the command-reply shape (the simulator demo path, whose
    /// reconnect is never cancelled).
    fn into_result(self) -> Result<(), String> {
        match self {
            Self::Connected => Ok(()),
            Self::ConnectedButTuneRereadFailed(e) | Self::Failed(e) => Err(e),
            Self::Cancelled => Err("reconnect cancelled".to_string()),
        }
    }
}

/// An event the owner wants delivered to the frontend. Decouples the loop
/// from `AppHandle` so tests inject a collecting closure (the `connect.rs`
/// emit-fn pattern).
#[derive(Debug, Clone)]
pub enum OwnerEvent {
    Connection(ConnectionStateEvent),
    TuneDirty(TuneDirtyEvent),
    /// A decoded realtime frame, already coalesced to ≤30 Hz (Task 6).
    Realtime(RealtimeFrameEvent),
}

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

/// Owner-side handle on a recovery running on the blocking pool.
struct RecoveryInFlight {
    /// Cooperative cancel flag; the reconnect loop checks it between attempts
    /// and backoff chunks (~100 ms granularity), so a user's Disconnect
    /// interrupts even the capped 30 s serial backoff promptly.
    cancel: Arc<AtomicBool>,
    /// Set when the user disconnected while the recovery was in flight: the
    /// settled session is then subject to the Disconnect retention rule
    /// (offline-origin tune survives without a link) instead of being
    /// restored, and no further event is emitted — Disconnect already
    /// emitted `Disconnected`.
    discard: bool,
}

struct ActiveLog {
    path: String,
    format: LogFormatDto,
    log: Log,
    started: Instant,
    counter: u8,
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
                let log_result = if self.active_log.is_some() {
                    self.stop_log().await.map(|_| ())
                } else {
                    Ok(())
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
                let _ = reply.send(log_result);
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
                offset,
                limit,
                reply,
            } => {
                let r = self
                    .opened_log
                    .as_ref()
                    .ok_or_else(|| "no log opened".to_string())
                    .and_then(|log| crate::log_bridge::slice(log, offset, limit));
                let _ = reply.send(r);
            }
            Command::SaveLog {
                path,
                format,
                reply,
            } => {
                let _ = reply.send(self.save_log(path, format).await);
            }
            Command::LogStats { params, reply } => {
                let _ = reply.send(self.run_log_stats(params).await);
            }
            Command::DetectAnomaly { thresholds, reply } => {
                let _ = reply.send(self.run_detect_anomaly(thresholds).await);
            }
            Command::VirtualDyno { params, reply } => {
                let _ = reply.send(self.run_virtual_dyno(params).await);
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

    /// Probe an idle live link. During realtime, `poll_tick` is already the
    /// probe, so this avoids sending an extra output-channel request.
    fn wants_health_check(&self) -> bool {
        #[cfg(test)]
        if self.health_checks_suspended {
            return false;
        }
        !self.polling && matches!(&self.session, Some(Session { conn: Some(_), .. }))
    }

    async fn health_tick(&mut self) {
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
    async fn poll_tick(&mut self) {
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
    async fn with_session<T: Send + 'static>(
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
    fn lose_session_after_panic(&mut self) {
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
    async fn connect(&mut self, source: ConnectSource) -> Result<(), String> {
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
                let r = ops::attach_connection(&mut session, source, &emit);
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
    async fn simulate_link_drop(&mut self) -> Result<(), String> {
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
    fn start_recovery(&mut self) {
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
    fn finish_recovery(&mut self, session: Option<Session>, outcome: RecoveryOutcome) {
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
    fn retain_offline_tune_only(&mut self, session: Option<Session>) {
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
    fn not_connected_error(&self) -> String {
        if self.reconnecting.is_some() {
            RECOVERY_IN_PROGRESS.to_owned()
        } else {
            NOT_CONNECTED.to_owned()
        }
    }

    fn start_log(&mut self, path: String, format: LogFormatDto) -> Result<LogStatusDto, String> {
        if self.active_log.is_some() {
            return Err("a log is already active".into());
        }
        if path.is_empty() {
            return Err("log path must not be empty".into());
        }
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| self.not_connected_error())?;
        let fields = session
            .def
            .output_channels
            .iter()
            .map(|channel| Field::float(channel.name(), ""))
            .collect();
        let mut log = Log::new(fields);
        log.started_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                u32::try_from(duration.as_secs()).unwrap_or(u32::MAX)
            });
        self.active_log = Some(ActiveLog {
            path,
            format,
            log,
            started: Instant::now(),
            counter: 0,
        });
        // Logging requires acquisition, but does not implicitly enable UI
        // realtime events. `wants_poll` keeps the owner ticking for the log.
        self.poller.get_or_insert_with(RealtimePoller::default);
        Ok(self.log_status())
    }

    async fn stop_log(&mut self) -> Result<LogSummaryDto, String> {
        let active = self
            .active_log
            .take()
            .ok_or_else(|| "no active log".to_string())?;
        let path = active.path.clone();
        let format = active.format;
        let log = active.log;
        let result = tokio::task::spawn_blocking(move || {
            let result = write_log_path(&path, format, &log);
            (log, result)
        })
        .await
        .map_err(|error| format!("log write panicked: {error}"))?;
        let (log, write_result) = result;
        let summary = crate::log_bridge::summary(&log);
        self.opened_log = Some(log);
        if !self.polling {
            self.poller = None;
        }
        write_result?;
        Ok(summary)
    }

    fn add_log_marker(&mut self, text: String) -> Result<(), String> {
        if !text.is_ascii() || text.len() > 49 {
            return Err("MLG v1 marker text must be ASCII and at most 49 bytes".into());
        }
        let active = self
            .active_log
            .as_mut()
            .ok_or_else(|| "no active log".to_string())?;
        let timestamp_10us = active.timestamp();
        active.log.entries.push(LogEntry::Marker(Marker {
            counter: active.counter,
            timestamp_10us,
            text,
        }));
        active.counter = next_counter(active.counter);
        Ok(())
    }

    fn log_status(&self) -> LogStatusDto {
        match &self.active_log {
            Some(active) => LogStatusDto {
                active: true,
                path: Some(active.path.clone()),
                format: Some(active.format),
                record_count: saturating_u32(active.log.records().count()),
            },
            None => LogStatusDto {
                active: false,
                path: None,
                format: None,
                record_count: 0,
            },
        }
    }

    async fn open_log(
        &mut self,
        path: String,
        format: LogFormatDto,
    ) -> Result<LogSummaryDto, String> {
        let log = tokio::task::spawn_blocking(move || read_log_path(&path, format))
            .await
            .map_err(|error| format!("log read panicked: {error}"))??;
        let summary = crate::log_bridge::summary(&log);
        self.opened_log = Some(log);
        Ok(summary)
    }

    async fn save_log(&mut self, path: String, format: LogFormatDto) -> Result<(), String> {
        let log = self
            .opened_log
            .clone()
            .ok_or_else(|| "no log opened".to_string())?;
        tokio::task::spawn_blocking(move || write_log_path(&path, format, &log))
            .await
            .map_err(|error| format!("log write panicked: {error}"))?
    }

    async fn run_log_stats(
        &mut self,
        params: LogStatsParamsDto,
    ) -> Result<LogStatsReportDto, String> {
        let samples = self.opened_samples()?;
        tokio::task::spawn_blocking(move || {
            let params = crate::log_bridge::stats_params(params);
            opentune_analysis::log_stats(&samples, &params)
                .map(crate::log_bridge::stats_report)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| format!("log stats panicked: {error}"))?
    }

    async fn run_detect_anomaly(
        &mut self,
        thresholds: AnomalyThresholdsDto,
    ) -> Result<AnomalyReportDto, String> {
        let samples = self.opened_samples()?;
        tokio::task::spawn_blocking(move || {
            let thresholds = crate::log_bridge::anomaly_params(thresholds);
            opentune_analysis::detect_anomaly(&samples, &thresholds)
                .map(crate::log_bridge::anomaly_report)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| format!("anomaly analysis panicked: {error}"))?
    }

    async fn run_virtual_dyno(
        &mut self,
        params: VirtualDynoParamsDto,
    ) -> Result<VirtualDynoReportDto, String> {
        let samples = self.opened_samples()?;
        tokio::task::spawn_blocking(move || {
            let params = crate::log_bridge::dyno_params(params);
            opentune_analysis::virtual_dyno(&samples, &params)
                .map(crate::log_bridge::dyno_report)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| format!("virtual dyno panicked: {error}"))?
    }

    fn opened_samples(&self) -> Result<opentune_analysis::SampleSet, String> {
        self.opened_log
            .as_ref()
            .map(crate::log_bridge::to_samples)
            .ok_or_else(|| "no log opened".to_string())
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

impl ActiveLog {
    fn timestamp(&self) -> u64 {
        let ticks = self.started.elapsed().as_micros() / 10;
        u64::try_from(ticks).unwrap_or(u64::MAX)
    }

    fn push(&mut self, frame: &opentune_realtime::RealtimeFrame) {
        let values = self
            .log
            .fields
            .iter()
            .map(|field| {
                frame
                    .channels
                    .iter()
                    .find(|channel| channel.name == field.name)
                    .map_or(f64::NAN, |channel| channel.value)
            })
            .collect();
        self.log.entries.push(LogEntry::Record(Record {
            counter: self.counter,
            timestamp_10us: self.timestamp(),
            values,
        }));
        self.counter = next_counter(self.counter);
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

fn next_counter(counter: u8) -> u8 {
    if counter == 254 {
        0
    } else {
        counter + 1
    }
}

fn read_log_path(path: &str, format: LogFormatDto) -> Result<Log, String> {
    let file = std::fs::File::open(path).map_err(|error| format!("{path}: {error}"))?;
    opentune_datalog::read_log(std::io::BufReader::new(file), format.into())
        .map_err(|error| error.to_string())
}

fn write_log_path(path: &str, format: LogFormatDto, log: &Log) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|error| format!("{path}: {error}"))?;
    let mut writer = std::io::BufWriter::new(file);
    opentune_datalog::write_log(log, &mut writer, format.into())
        .map_err(|error| error.to_string())?;
    use std::io::Write as _;
    writer.flush().map_err(|error| error.to_string())
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

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
