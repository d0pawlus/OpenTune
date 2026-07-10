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

use std::sync::Arc;
use std::time::Duration;

use opentune_model::Value;
use opentune_realtime::RealtimePoller;
use tauri_specta::Event as _;
use tokio::sync::{mpsc, oneshot};

#[cfg(test)]
use crate::connection::ActiveConnection;
use crate::connection::{ConnectSource, Session};
use crate::dto::{CaptureStatusDto, CellEditDto, DefinitionDto, FieldDiffDto, VeAnalysisReportDto};
use crate::events::{ConnectionStateEvent, RealtimeFrameEvent, TuneDirtyEvent};
use ops::{build_session, link_drop};

const NOT_CONNECTED: &str = "not connected";

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
        picks: Vec<String>,
        reply: Reply<Option<TuneDirtyEvent>>,
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
    tauri::async_runtime::spawn(run_owner(rx, emit));
    tx
}

/// The owner-driven poll cadence: 25 Hz. UI emission is separately coalesced
/// to ≤30 Hz by the [`RealtimePoller`]'s 33 ms gate.
const POLL_INTERVAL: Duration = Duration::from_millis(40);

/// The owner's private state: the session it exclusively owns plus the
/// realtime polling state (Task 6). `poller` holds the ≤30 Hz emit gate;
/// it exists exactly while `polling` is set.
struct Owner {
    session: Option<Session>,
    polling: bool,
    poller: Option<RealtimePoller>,
    emit: Emitter,
}

async fn run_owner(mut rx: mpsc::Receiver<Command>, emit: Emitter) {
    let mut owner = Owner {
        session: None,
        polling: false,
        poller: None,
        emit,
    };
    let mut tick = tokio::time::interval(POLL_INTERVAL);
    // While polling is off (or a long command blocks the loop), ticks pile
    // up unobserved — skip them instead of bursting to "catch up".
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
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
        }
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
                (self.emit)(OwnerEvent::Connection(ConnectionStateEvent::Disconnected));
                let _ = reply.send(Ok(()));
            }
            Command::SimulateLinkDrop { reply } => {
                let _ = reply.send(self.simulate_link_drop().await);
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
            Command::StartRealtime { reply } => {
                self.polling = true;
                self.poller = Some(RealtimePoller::default());
                let _ = reply.send(Ok(()));
            }
            Command::StopRealtime { reply } => {
                self.polling = false;
                self.poller = None;
                let _ = reply.send(Ok(()));
            }
            // M4 Task 0: seams frozen, handlers stubbed until their task
            // (Task 8 / Task 11). Each still sends exactly one reply, per
            // the M3 rule.
            Command::StartCapture { reply } => {
                let _ = reply.send(Err("not implemented (M4)".to_string()));
            }
            Command::StopCapture { reply } => {
                let _ = reply.send(Err("not implemented (M4)".to_string()));
            }
            Command::CaptureStatus { reply } => {
                let _ = reply.send(Err("not implemented (M4)".to_string()));
            }
            Command::RunVeAnalyze { reply, .. } => {
                let _ = reply.send(Err("not implemented (M4)".to_string()));
            }
            #[cfg(test)]
            Command::DebugSimulator { reply } => {
                let r = match &self.session {
                    Some(Session {
                        conn: Some(ActiveConnection::Sim { simulator, .. }),
                        ..
                    }) => Ok(Arc::clone(simulator)),
                    Some(_) => Err("not a simulator connection".to_owned()),
                    None => Err(NOT_CONNECTED.to_owned()),
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
        }
    }

    /// Emit the tune-dirty event carried by a successful mutating op.
    fn emit_dirty(&self, r: &Result<TuneDirtyEvent, String>) {
        if let Ok(ev) = r {
            (self.emit)(OwnerEvent::TuneDirty(ev.clone()));
        }
    }

    /// True while the poll tick should fire: realtime was explicitly started
    /// and there is a live session to poll.
    fn wants_poll(&self) -> bool {
        self.polling && self.session.is_some()
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
            let r = session.poll_frame(&mut poller);
            (session, poller, r)
        })
        .await
        {
            Ok((session, poller, r)) => {
                self.session = Some(session);
                self.poller = Some(poller);
                if let Ok(Some(frame)) = r {
                    let channels = frame
                        .channels
                        .into_iter()
                        .map(|c| (c.name, c.value))
                        .collect();
                    (self.emit)(OwnerEvent::Realtime(RealtimeFrameEvent { channels }));
                }
            }
            // Panicked mid-poll: the session is lost (poisoned-equivalent,
            // same as `with_session`); subsequent commands report
            // "not connected" and the poll gate stays disarmed.
            Err(_) => self.polling = false,
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
            return Err(NOT_CONNECTED.to_owned());
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
            Err(e) => Err(format!("session operation panicked: {e}")),
        }
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
        if matches!(&self.session, Some(s) if s.conn.is_none() && s.tune.is_some()) {
            let mut session = self.session.take().expect("checked by matches! above");
            let emit = Arc::clone(&self.emit);
            // Always hand the session back (tuple pattern, same as
            // `with_session`/`simulate_link_drop`): a *failed* attach — the
            // signature guard rejecting a mismatched ECU, or `connect_serial`
            // erroring on a bad port — must leave the user's unsaved offline
            // tune intact, never destroyed. Only a genuine task panic loses it.
            let (session, r) = tokio::task::spawn_blocking(move || {
                let r = ops::attach_connection(&mut session, source, &emit);
                (session, r)
            })
            .await
            .map_err(|e| format!("attach panicked: {e}"))?;
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
        self.reset_session();
        let emit = Arc::clone(&self.emit);
        let session = tokio::task::spawn_blocking(move || build_session(source, &emit))
            .await
            .map_err(|e| format!("connect panicked: {e}"))??;
        self.session = Some(session);
        Ok(())
    }

    /// Simulator-only: drop the link and drive M1's reconnect loop, emitting
    /// each state. Runs on the blocking pool; the session (definition + tune
    /// preserved) is put back when the reconnect settles.
    async fn simulate_link_drop(&mut self) -> Result<(), String> {
        let Some(session) = self.session.take() else {
            return Err(NOT_CONNECTED.to_owned());
        };
        let emit = Arc::clone(&self.emit);
        let (session, r) = tokio::task::spawn_blocking(move || link_drop(session, &emit))
            .await
            .map_err(|e| format!("link drop panicked: {e}"))?;
        self.session = session;
        r
    }

    /// Merge picks, then emit the tune's *actual* dirty state — read after
    /// the merge attempt regardless of `Ok`/`Err`, because a merge can abort
    /// mid-batch after earlier picks already committed (M2 behavior).
    async fn merge_tune(&mut self, picks: Vec<String>) -> Result<Option<TuneDirtyEvent>, String> {
        let (result, event) = self
            .with_session(move |s| {
                let result = s.merge_tune(&picks);
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
        let had_link = matches!(&self.session, Some(s) if s.conn.is_some());
        self.session = None;
        self.polling = false;
        self.poller = None;
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

#[path = "owner_ops.rs"]
mod ops;

#[cfg(test)]
#[path = "owner_tests.rs"]
mod tests;
