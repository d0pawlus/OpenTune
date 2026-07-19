// SPDX-License-Identifier: GPL-3.0-or-later
//! IPC commands for the embedded assistant's chat (M7 slice 3, task 4):
//! `ai_send`, `ai_cancel`, `ai_reset`, `ai_proposals`, the `AiChatState`
//! they share, and the `AiStreamEvent` `ai_send`'s spawned task emits.
//!
//! ## Concurrency design
//!
//! `AiChatState`'s fields are `Arc`-wrapped `std::sync::Mutex`es /
//! `AtomicBool`s, not `tokio::sync::Mutex`. Every lock this module takes is
//! held for a short, synchronous span — `mem::take`/replace on the history,
//! or a `get_or_insert_with` on the executor slot — and is always released
//! before the next `.await`. `run_chat_turn` itself never sees a lock: its
//! `history: &mut ChatHistory` argument is a plain owned value that
//! `ai_send`'s spawned task takes out of the `Mutex` before calling it and
//! puts back after. A `tokio::Mutex` would remove the "never hold across
//! `.await`" constraint, but nothing here needs to hold a lock while
//! awaiting, so it would only add a dependency and a `.lock().await` at
//! every call site for no benefit — the simpler std primitive is enough.
//!
//! `running` is cleared by [`RunningGuard`], an RAII guard constructed
//! immediately after `ai_send`'s `compare_exchange` succeeds — *before* the
//! executor/provider setup that follows it — and then moved into the
//! spawned task. Constructing it that early (I3) means a panic during that
//! synchronous setup (still inside `ai_send`, not yet in the spawned task)
//! also clears `running` via unwind, not just a panic inside the task
//! itself. `Drop` runs on every exit path: `run_chat_turn` returning
//! `Ok`/`Err`, an in-task validation failure (e.g. the key vanished between
//! the presence check in `ai_send` and the read inside the task), and even
//! an unexpected panic — this crate does not set `panic = "abort"`, so
//! `Drop` still runs during unwind. A caveat documented on `RunningGuard`: a
//! panic after `history` has been taken out of the `Mutex` loses that
//! turn's messages (the slot is left holding the pre-turn history), since
//! the `history_handle.lock() = history` restore never executes. `running`
//! still clears either way, so the UI never gets stuck — only that one
//! turn's history is lost.
//!
//! `RunningGuard` also guarantees the frontend always sees a terminal
//! stream event (I3): every `ChatEvent` on the task's one send path goes
//! through `RunningGuard::emit`, which flags `terminal_emitted` when the
//! event is `Done`/`Cancelled`/`Error`. If `Drop` runs and that flag is
//! still unset — the task returned or panicked without ever reaching a
//! terminal event — it sends a best-effort synthetic `Error` itself, so the
//! UI is never left stuck on "running" with no explanation.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, State};
use tauri_specta::Event as _;

use opentune_ai::{AuditChannel, GuardrailLimits, PermissionPolicy};

use crate::ai_anthropic::AnthropicProvider;
use crate::ai_chat::{run_chat_turn, system_prompt, ChatEvent, ChatHistory};
use crate::ai_commands::{config_dir, AiKeyStoreState};
use crate::ai_openai::OpenAiProvider;
use crate::ai_provider::{Provider, ToolDef};
use crate::ai_settings::{load_ai_settings_in, AiSettings};
use crate::ai_tools::{AiToolExecutor, FileAuditSink};
use crate::dto::AiProposalDto;
use crate::events::AiStreamEvent;
use crate::owner::OwnerHandle;

/// Max tokens per model response in one chat turn. Conservative relative to
/// modern context windows on purpose: the assistant's replies are meant to
/// be concise diagnostic/tuning guidance, not long-form prose (see
/// `ai_chat::system_prompt`'s "be concise" rule), and a hard cap keeps a
/// single runaway response from dominating the turn's token budget.
pub const CHAT_MAX_TOKENS: u32 = 4096;

/// Audit log filename inside `app_config_dir`, alongside
/// `ai_settings::AI_SETTINGS_FILE`.
const AI_AUDIT_FILE: &str = "ai-audit.jsonl";

/// Shared error string for "a turn is already running" — used by both
/// `validate_send` (pre-flight, before any lock is touched) and the
/// `compare_exchange` guard in `ai_send` (the actual race guard), so the
/// two paths can never drift into different wording for the same failure.
const ALREADY_RUNNING_MSG: &str =
    "a chat turn is already running — wait for it to finish or cancel it";

/// M7 slice 3 chat session state: one conversation, shared by every `ai_*`
/// command. Managed once in `lib.rs::run()`'s setup (`app.manage`).
///
/// Every field is independently cloneable (behind an `Arc`) or briefly
/// lockable, so `ai_send` can hand owned clones to its spawned tokio task
/// without ever holding a lock across an `.await` — see the module doc.
#[derive(Default)]
pub struct AiChatState {
    /// The conversation so far. `ai_send` takes ownership of it for the
    /// duration of one turn (std `Mutex` + take/replace) rather than
    /// holding the lock across `run_chat_turn`'s `.await`s.
    history: Arc<Mutex<ChatHistory>>,
    /// Cooperative cancel flag `run_chat_turn` polls between steps.
    /// `ai_cancel` sets it; `ai_send` clears it at the start of each turn.
    cancel: Arc<AtomicBool>,
    /// True while a chat turn's spawned task is in flight. Guarded against
    /// a second concurrent `ai_send` by a `compare_exchange` (checked
    /// first, for a clean error, by `validate_send`) and always cleared by
    /// [`RunningGuard`].
    running: Arc<AtomicBool>,
    /// Lazily built on the first `ai_send` — it needs the owner handle and
    /// the audit file path, both only available inside the command, not at
    /// `AiChatState::default()` time. `ai_reset` replaces this with `None`
    /// so old proposals — which live in the executor's in-memory log, not
    /// in `AiChatState` — drop along with the executor; the next `ai_send`
    /// lazily builds a fresh executor, whose proposal ids restart at 1.
    executor: Mutex<Option<Arc<AiToolExecutor>>>,
}

/// RAII guard that (1) clears `running` when the chat turn ends and (2)
/// guarantees a terminal `AiStreamEvent` reaches the frontend, even if the
/// turn ends via panic (I3). Constructed immediately after `ai_send`'s
/// `compare_exchange` succeeds and then moved into the spawned task — see
/// the module doc comment for why that timing matters. Every `ChatEvent`
/// the send path emits should go through [`RunningGuard::emit`] rather than
/// straight to `AiStreamEvent::emit`, so `terminal_emitted` stays accurate;
/// this is the single place `running` is cleared and a terminal event is
/// guaranteed, so exit paths added later stay covered automatically
/// instead of needing their own bookkeeping.
///
/// Generic over the sink function (rather than boxing it as
/// `Arc<dyn Fn(..)>`) so the guard has no runtime dispatch cost and — more
/// usefully for tests — a unit test can plug in a plain `Vec`-collecting
/// closure without needing a real `AppHandle`.
struct RunningGuard<F: Fn(ChatEvent) + Send + Sync> {
    running: Arc<AtomicBool>,
    /// Set once a `Done`/`Cancelled`/`Error` event has actually been sent
    /// through `emit`. `Drop` checks this to decide whether it needs to
    /// synthesize one.
    terminal_emitted: Arc<AtomicBool>,
    /// Where events actually go — `AiStreamEvent::from(ev).emit(&app)` in
    /// production, a `Vec` collector in tests.
    sink: F,
}

impl<F: Fn(ChatEvent) + Send + Sync> RunningGuard<F> {
    /// Send one `ChatEvent` through the guard's sink, marking
    /// `terminal_emitted` first if `ev` is `Done`/`Cancelled`/`Error` — the
    /// single place that decides "did this turn reach a terminal event",
    /// shared by every call site (the task's early-return branches and
    /// `run_chat_turn`'s callback).
    fn emit(&self, ev: ChatEvent) {
        if matches!(
            ev,
            ChatEvent::Done | ChatEvent::Cancelled | ChatEvent::Error { .. }
        ) {
            self.terminal_emitted.store(true, Ordering::SeqCst);
        }
        (self.sink)(ev);
    }
}

impl<F: Fn(ChatEvent) + Send + Sync> Drop for RunningGuard<F> {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        // `swap` (not `load` then `store`) so this check-and-set is a
        // single atomic step — irrelevant for the single-threaded drop
        // path today, but it's the same guarantee `running`'s
        // `compare_exchange` relies on, for free.
        if !self.terminal_emitted.swap(true, Ordering::SeqCst) {
            (self.sink)(ChatEvent::Error {
                message: "assistant task failed unexpectedly".into(),
            });
        }
    }
}

/// Pure, testable pre-flight for `ai_send` (TDD RED step). Checked in this
/// order because each earlier failure is more actionable than the next —
/// there is no point telling the user their message is empty if AI is
/// still switched off. English diagnostics only: the frontend renders its
/// own localized copy and falls back to this string only for unexpected
/// errors (see the task brief's "i18n happens frontend-side" note).
pub(crate) fn validate_send(
    settings: &AiSettings,
    key_present: bool,
    text: &str,
    running: bool,
) -> Result<(), String> {
    if !settings.enabled {
        return Err("AI is disabled — enable it in Settings before sending a message".into());
    }
    if !key_present {
        return Err(format!(
            "no API key configured for provider \"{}\" — add one in Settings",
            settings.provider
        ));
    }
    if text.trim().is_empty() {
        return Err("message text must not be empty".into());
    }
    if running {
        return Err(ALREADY_RUNNING_MSG.into());
    }
    Ok(())
}

/// Build the enum-dispatch `Provider` for a validated settings provider id.
/// `ai_settings::validate_provider` already rejects unknown ids at
/// settings-save time, so the `other` arm here is a defensive fallback, not
/// a path normal use reaches.
fn build_provider(provider_name: &str, api_key: String) -> Result<Provider, String> {
    match provider_name {
        "anthropic" => Ok(Provider::Anthropic(AnthropicProvider { api_key })),
        "openai" => Ok(Provider::OpenAi(OpenAiProvider { api_key })),
        other => Err(format!("unknown AI provider: {other}")),
    }
}

/// Validate, then fire-and-forget one user turn: spawns a tokio task
/// running [`run_chat_turn`] and returns immediately — progress streams to
/// the frontend as [`AiStreamEvent`]s, not through this command's return
/// value. The assistant never writes to the ECU here or anywhere else in
/// this file; `run_chat_turn`'s `executor.execute` is the only path a tool
/// call can take, and it is gated by the advisory policy (ADR-0008).
#[tauri::command]
#[specta::specta]
pub async fn ai_send(
    text: String,
    app: AppHandle,
    owner: State<'_, OwnerHandle>,
    keys: State<'_, AiKeyStoreState>,
    chat: State<'_, AiChatState>,
) -> Result<(), String> {
    let dir = config_dir(&app)?;
    let settings = load_ai_settings_in(&dir)?;
    let key_present = keys.0.key_present(&settings.provider)?;
    let running = chat.running.load(Ordering::SeqCst);
    validate_send(&settings, key_present, &text, running)?;

    // `validate_send` just observed `running == false`, but two concurrent
    // `ai_send` calls could both observe that before either sets it — this
    // `compare_exchange` is the actual race guard, sharing the same error
    // string so the user sees one consistent message either way.
    chat.running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| ALREADY_RUNNING_MSG.to_owned())?;

    // I3: built immediately after the compare_exchange succeeds, before any
    // of the executor/provider setup below — so a panic in that setup
    // (still synchronous, still inside `ai_send`, before the task exists)
    // clears `running` via this guard's `Drop` during unwind, the same as a
    // panic inside the spawned task would.
    let running_guard = {
        let app = app.clone();
        RunningGuard {
            running: Arc::clone(&chat.running),
            terminal_emitted: Arc::new(AtomicBool::new(false)),
            sink: move |ev: ChatEvent| {
                let _ = AiStreamEvent::from(ev).emit(&app);
            },
        }
    };

    chat.cancel.store(false, Ordering::SeqCst);

    // Advisory is the only authority level any UI can reach (ADR-0008):
    // shared by the executor's policy gate and by the tool list/system
    // prompt built below.
    let policy = PermissionPolicy::advisory();

    let executor = {
        let mut guard = chat.executor.lock().unwrap();
        let exec = guard.get_or_insert_with(|| {
            let sink = FileAuditSink::new(dir.join(AI_AUDIT_FILE));
            Arc::new(AiToolExecutor::new(
                owner.inner().clone(),
                policy,
                GuardrailLimits::default(),
                AuditChannel::Assistant,
                Box::new(sink),
            ))
        });
        Arc::clone(exec)
    };

    let specs = opentune_ai::available_tools(&policy);
    let system = system_prompt(&specs);
    let tools: Vec<ToolDef> = specs.into_iter().map(ToolDef::from).collect();
    let model = settings.model.clone();
    let provider_name = settings.provider.clone();

    let history_handle = Arc::clone(&chat.history);
    let cancel_handle = Arc::clone(&chat.cancel);
    let key_store = Arc::clone(&keys.0);

    tauri::async_runtime::spawn(async move {
        let running_guard = running_guard;

        let key = match key_store.get_key(&provider_name) {
            Ok(Some(key)) => key,
            Ok(None) => {
                running_guard.emit(ChatEvent::Error {
                    message: format!("no API key configured for provider \"{provider_name}\""),
                });
                return;
            }
            Err(message) => {
                running_guard.emit(ChatEvent::Error { message });
                return;
            }
        };
        let provider = match build_provider(&provider_name, key) {
            Ok(provider) => provider,
            Err(message) => {
                running_guard.emit(ChatEvent::Error { message });
                return;
            }
        };

        let mut history = {
            let mut guard = history_handle.lock().unwrap();
            std::mem::take(&mut *guard)
        };

        let emit = |ev: ChatEvent| running_guard.emit(ev);
        let _ = run_chat_turn(
            &provider,
            &executor,
            &mut history,
            &tools,
            &system,
            &model,
            CHAT_MAX_TOKENS,
            text,
            &cancel_handle,
            &emit,
        )
        .await;

        *history_handle.lock().unwrap() = history;
    });

    Ok(())
}

/// Signal cancellation of the in-flight turn, if any. Cooperative: the
/// running `run_chat_turn` observes the flag between steps (see its
/// `cancel.load` checks) and stops at the next opportunity — this command
/// does not itself wait for that to happen.
#[tauri::command]
#[specta::specta]
pub async fn ai_cancel(chat: State<'_, AiChatState>) -> Result<(), String> {
    chat.cancel.store(true, Ordering::SeqCst);
    Ok(())
}

/// Clear the conversation and drop any recorded proposals. Errors while a
/// turn is running rather than racing `ai_send`'s task for the history
/// slot. Proposals are not stored in `AiChatState` directly — they live in
/// the executor's in-memory log — so clearing them means replacing the
/// executor itself with `None`; `ai_send` lazily rebuilds one on the next
/// turn (see `AiChatState::executor`'s doc comment).
#[tauri::command]
#[specta::specta]
pub async fn ai_reset(chat: State<'_, AiChatState>) -> Result<(), String> {
    if chat.running.load(Ordering::SeqCst) {
        return Err("cannot reset while a chat turn is running — cancel it first".into());
    }
    *chat.history.lock().unwrap() = ChatHistory::new();
    *chat.executor.lock().unwrap() = None;
    Ok(())
}

/// The proposals recorded so far this session (empty before the first
/// `ai_send`, since the executor is lazily created).
#[tauri::command]
#[specta::specta]
pub async fn ai_proposals(chat: State<'_, AiChatState>) -> Result<Vec<AiProposalDto>, String> {
    let executor = chat.executor.lock().unwrap().clone();
    Ok(executor
        .map(|exec| {
            exec.proposals()
                .into_iter()
                .map(AiProposalDto::from)
                .collect()
        })
        .unwrap_or_default())
}

#[cfg(test)]
mod validate_send_tests {
    use super::*;

    fn settings(enabled: bool) -> AiSettings {
        AiSettings {
            enabled,
            provider: "anthropic".into(),
            model: "claude-x".into(),
        }
    }

    #[test]
    fn disabled_ai_is_rejected_with_an_enable_hint() {
        let err = validate_send(&settings(false), true, "hello", false).unwrap_err();
        assert!(
            err.to_lowercase().contains("enable"),
            "error should mention enabling AI: {err}"
        );
    }

    #[test]
    fn missing_key_is_rejected() {
        let err = validate_send(&settings(true), false, "hello", false).unwrap_err();
        assert!(!err.is_empty());
        assert!(err.contains("anthropic"), "names the provider: {err}");
    }

    #[test]
    fn empty_text_is_rejected() {
        let err = validate_send(&settings(true), true, "   ", false).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn already_running_is_rejected() {
        let err = validate_send(&settings(true), true, "hello", true).unwrap_err();
        assert_eq!(err, ALREADY_RUNNING_MSG);
    }

    #[test]
    fn happy_path_is_ok() {
        assert!(validate_send(&settings(true), true, "hello", false).is_ok());
    }

    #[test]
    fn checks_are_ordered_enabled_before_key_before_text_before_running() {
        // Disabled + everything else also wrong -> the enable error wins.
        let err = validate_send(&settings(false), false, "", true).unwrap_err();
        assert!(err.to_lowercase().contains("enable"), "got: {err}");
    }
}

#[cfg(test)]
mod build_provider_tests {
    use super::*;

    #[test]
    fn maps_anthropic() {
        assert!(matches!(
            build_provider("anthropic", "k".into()),
            Ok(Provider::Anthropic(_))
        ));
    }

    #[test]
    fn maps_openai() {
        assert!(matches!(
            build_provider("openai", "k".into()),
            Ok(Provider::OpenAi(_))
        ));
    }

    #[test]
    fn rejects_unknown_provider() {
        // `Provider` has no `Debug` impl, so `Result::unwrap_err` (which
        // requires the `Ok` side to be `Debug` for its panic message)
        // doesn't type-check here — match instead.
        match build_provider("acme", "k".into()) {
            Err(err) => assert!(err.contains("acme")),
            Ok(_) => panic!("unknown provider must be rejected"),
        }
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;

    #[test]
    fn default_chat_state_is_idle_with_no_executor() {
        let state = AiChatState::default();
        assert!(!state.running.load(Ordering::SeqCst));
        assert!(!state.cancel.load(Ordering::SeqCst));
        assert!(state.executor.lock().unwrap().is_none());
        assert!(state.history.lock().unwrap().is_empty());
    }

    #[test]
    fn running_guard_clears_flag_on_drop_even_on_early_return() {
        let running = Arc::new(AtomicBool::new(true));
        {
            let _guard = RunningGuard {
                running: Arc::clone(&running),
                terminal_emitted: Arc::new(AtomicBool::new(false)),
                sink: |_ev: ChatEvent| {},
            };
            assert!(running.load(Ordering::SeqCst), "still running under guard");
        }
        assert!(
            !running.load(Ordering::SeqCst),
            "guard must clear on drop, including panic unwind"
        );
    }

    // I3: RunningGuard also guarantees a terminal stream event — these two
    // tests exercise that mechanism directly (a plain Vec-collecting sink,
    // no real AppHandle needed), covering both the "task exited without
    // ever emitting a terminal event" case (e.g. a panic mid-turn) and the
    // "no double emission" case (a real terminal event already went
    // through `.emit`).

    #[test]
    fn running_guard_synthesizes_a_terminal_error_on_drop_when_none_was_emitted() {
        let running = Arc::new(AtomicBool::new(true));
        let events: Arc<Mutex<Vec<ChatEvent>>> = Arc::new(Mutex::new(Vec::new()));
        {
            let events_sink = Arc::clone(&events);
            let _guard = RunningGuard {
                running: Arc::clone(&running),
                terminal_emitted: Arc::new(AtomicBool::new(false)),
                sink: move |ev: ChatEvent| events_sink.lock().unwrap().push(ev),
            };
            // Simulates a task that returns (or panics) without the turn
            // ever reaching Done/Cancelled/Error.
        }
        assert!(!running.load(Ordering::SeqCst), "guard clears running");
        let events = events.lock().unwrap();
        assert_eq!(
            events.len(),
            1,
            "drop synthesizes exactly one terminal event: {events:?}"
        );
        assert!(
            matches!(&events[0], ChatEvent::Error { message } if message.contains("unexpectedly")),
            "synthetic error names the failure: {events:?}"
        );
    }

    #[test]
    fn running_guard_does_not_double_emit_when_a_terminal_event_already_went_through() {
        let running = Arc::new(AtomicBool::new(true));
        let events: Arc<Mutex<Vec<ChatEvent>>> = Arc::new(Mutex::new(Vec::new()));
        {
            let events_sink = Arc::clone(&events);
            let guard = RunningGuard {
                running: Arc::clone(&running),
                terminal_emitted: Arc::new(AtomicBool::new(false)),
                sink: move |ev: ChatEvent| events_sink.lock().unwrap().push(ev),
            };
            guard.emit(ChatEvent::Done);
        }
        let events = events.lock().unwrap();
        assert_eq!(
            events.len(),
            1,
            "only the real Done event — no synthetic duplicate: {events:?}"
        );
        assert!(matches!(&events[0], ChatEvent::Done));
    }
}
