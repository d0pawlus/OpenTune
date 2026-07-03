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

use opentune_model::Value;
use tauri_specta::Event as _;
use tokio::sync::{mpsc, oneshot};

#[cfg(test)]
use crate::connection::ActiveConnection;
use crate::connection::{ConnectSource, Session};
use crate::dto::{DefinitionDto, FieldDiffDto};
use crate::events::{ConnectionStateEvent, TuneDirtyEvent};
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
    /// Test-only: hand back the live simulator so tests can drive secl /
    /// reboot scenarios (same access the M2 session tests used directly).
    #[cfg(test)]
    DebugSimulator {
        reply: Reply<Arc<opentune_simulator::EcuSimulator>>,
    },
}

/// An event the owner wants delivered to the frontend. Decouples the loop
/// from `AppHandle` so tests inject a collecting closure (the `connect.rs`
/// emit-fn pattern).
#[derive(Debug, Clone)]
pub enum OwnerEvent {
    Connection(ConnectionStateEvent),
    TuneDirty(TuneDirtyEvent),
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
    });
    spawn_owner_with_emitter(emit)
}

/// Spawn the owner task with an injected event sink (testable core).
pub fn spawn_owner_with_emitter(emit: Emitter) -> OwnerHandle {
    let (tx, rx) = mpsc::channel(32);
    tauri::async_runtime::spawn(run_owner(rx, emit));
    tx
}

/// The owner's private state: the session it exclusively owns plus the
/// realtime `polling` flag (ticked by Task 6).
struct Owner {
    session: Option<Session>,
    polling: bool,
    emit: Emitter,
}

async fn run_owner(mut rx: mpsc::Receiver<Command>, emit: Emitter) {
    let mut owner = Owner {
        session: None,
        polling: false,
        emit,
    };
    while let Some(cmd) = rx.recv().await {
        owner.serve(cmd).await;
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
            Command::StartRealtime { reply } => {
                self.polling = true; // Task 6 adds the poll tick.
                let _ = reply.send(Ok(()));
            }
            Command::StopRealtime { reply } => {
                self.polling = false;
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
        }
    }

    /// Emit the tune-dirty event carried by a successful mutating op.
    fn emit_dirty(&self, r: &Result<TuneDirtyEvent, String>) {
        if let Ok(ev) = r {
            (self.emit)(OwnerEvent::TuneDirty(ev.clone()));
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

    /// Tear down any current session, then build a fresh one: parse the
    /// definition, open the transport, and run the handshake (all blocking).
    /// `Connecting`/`Connected` are emitted from inside the handshake so a
    /// slow serial connect still shows live progress.
    async fn connect(&mut self, source: ConnectSource) -> Result<(), String> {
        self.session = None;
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
}

#[path = "owner_ops.rs"]
mod ops;

#[cfg(test)]
#[path = "owner_tests.rs"]
mod tests;
