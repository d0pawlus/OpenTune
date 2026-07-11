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

use opentune_model::{MergePick, Value};
use opentune_realtime::RealtimePoller;
use tauri_specta::Event as _;
use tokio::sync::{mpsc, oneshot};

#[cfg(test)]
use crate::connection::ActiveConnection;
use crate::connection::{ConnectSource, Session};
use crate::dto::{DefinitionDto, FieldDiffDto, MergePickDto, ResolvedGaugeBoundsDto};
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
    /// Test-only: hand back the live simulator so tests can drive secl /
    /// reboot scenarios (same access the M2 session tests used directly).
    #[cfg(test)]
    DebugSimulator {
        reply: Reply<Arc<opentune_simulator::EcuSimulator>>,
    },
    /// Test-only: panic inside a session `spawn_blocking` operation.
    #[cfg(test)]
    DebugPanicSessionOperation {
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
    #[cfg(test)]
    fail_next_reboot_tune_read: bool,
}

async fn run_owner(mut rx: mpsc::Receiver<Command>, emit: Emitter) {
    let mut owner = Owner {
        session: None,
        polling: false,
        poller: None,
        emit,
        #[cfg(test)]
        fail_next_reboot_tune_read: false,
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
                self.session = None;
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
                let r = if self.session.is_some() {
                    self.polling = true;
                    self.poller = Some(RealtimePoller::default());
                    Ok(())
                } else {
                    self.polling = false;
                    self.poller = None;
                    Err(format!("{NOT_CONNECTED} — cannot start realtime"))
                };
                let _ = reply.send(r);
            }
            Command::StopRealtime { reply } => {
                self.polling = false;
                self.poller = None;
                let _ = reply.send(Ok(()));
            }
            #[cfg(test)]
            Command::DebugSimulator { reply } => {
                let r = match &self.session {
                    Some(Session {
                        conn: ActiveConnection::Sim { simulator, .. },
                        ..
                    }) => Ok(Arc::clone(simulator)),
                    Some(_) => Err("not a simulator connection".to_owned()),
                    None => Err(NOT_CONNECTED.to_owned()),
                };
                let _ = reply.send(r);
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

    /// Tear down any current session, then build a fresh one: parse the
    /// definition, open the transport, and run the handshake (all blocking).
    /// `Connecting`/`Connected` are emitted from inside the handshake so a
    /// slow serial connect still shows live progress.
    async fn connect(&mut self, source: ConnectSource) -> Result<(), String> {
        self.session = None;
        // Realtime is explicit-start only: a fresh session never inherits a
        // previous session's polling (same rule as Disconnect).
        self.polling = false;
        self.poller = None;
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
            return Err(NOT_CONNECTED.to_owned());
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
}

#[path = "owner_ops.rs"]
mod ops;

#[cfg(test)]
#[path = "owner_tests.rs"]
mod tests;
