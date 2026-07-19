// SPDX-License-Identifier: GPL-3.0-or-later
//! The §9 owner's wire protocol — the [`Command`] message type sent over the
//! owner's mpsc channel plus its companion reply/event types. Split from
//! `owner.rs` for file cohesion (these are plain data types with no access
//! to [`super::Owner`]'s private state).

#[cfg(test)]
use std::sync::Arc;

use tokio::sync::oneshot;

use crate::connection::{ConnectSource, Session};
use crate::dto::{
    AnomalyReportDto, AnomalyThresholdsDto, CaptureStatusDto, CellEditDto, DefinitionDto,
    FieldDiffDto, LogDataDto, LogFormatDto, LogStatsParamsDto, LogStatsReportDto, LogStatusDto,
    LogSummaryDto, MergePickDto, RealtimeSnapshotDto, ResolvedGaugeBoundsDto, VeAnalysisReportDto,
    VirtualDynoParamsDto, VirtualDynoReportDto,
};
use crate::events::TuneDirtyEvent;
use crate::events::{ConnectionStateEvent, RealtimeFrameEvent};
use opentune_model::Value;

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
        log_id: u32,
        offset: u32,
        limit: u32,
        reply: Reply<LogDataDto>,
    },
    SaveLog {
        log_id: u32,
        path: String,
        format: LogFormatDto,
        reply: Reply<()>,
    },
    LogStats {
        log_id: u32,
        params: LogStatsParamsDto,
        reply: Reply<LogStatsReportDto>,
    },
    DetectAnomaly {
        log_id: u32,
        thresholds: AnomalyThresholdsDto,
        reply: Reply<AnomalyReportDto>,
    },
    VirtualDyno {
        log_id: u32,
        params: VirtualDynoParamsDto,
        reply: Reply<VirtualDynoReportDto>,
    },
    /// Latest retained realtime frame for the AI `read_realtime` tool (M7).
    RealtimeSnapshot {
        reply: Reply<Option<RealtimeSnapshotDto>>,
    },
    /// Resolved INI `[low, high]` bounds of a constant — the guardrail
    /// input for AI change proposals (M7).
    ConstantBounds {
        name: String,
        reply: Reply<(f64, f64)>,
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
    /// Build a fresh offline session and load a `.msq` into it
    /// (M4 Task 3 — offline tune lifecycle). Replies with the definition
    /// plus the `.msq` load report (skipped/clamped/failed constants).
    OpenTune {
        ini_path: String,
        msq_path: String,
        reply: Reply<crate::dto::OpenTuneDto>,
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
    pub(super) fn into_result(self) -> Result<(), String> {
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
