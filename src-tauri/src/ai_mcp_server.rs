// SPDX-License-Identifier: GPL-3.0-or-later
//! HTTP transport + lifecycle for the MCP server (M7 slice 4 task 4).
//!
//! Scope is deliberately narrow: this module owns *only* binding a loopback
//! `axum` listener, mounting rmcp's [`StreamableHttpService`] behind a
//! bearer-auth middleware, and starting/stopping that task. The MCP-facing
//! protocol logic (tool list, tool dispatch) lives in `ai_mcp.rs`'s
//! [`OpenTuneMcp`] — this file never inspects a JSON-RPC frame.
//!
//! ## Security invariants
//! - Binds `127.0.0.1` ONLY — never `0.0.0.0` (see [`start_mcp_server`]).
//! - Every request must carry a matching `Authorization: Bearer <token>`
//!   header, checked with a constant-time comparison
//!   ([`tokens_match`]) — never a plain `==`.
//! - rmcp's Host-header validation stays on ([`build_router`] pins
//!   `allowed_hosts` to loopback forms explicitly, rather than relying on
//!   the library default silently doing the same thing).
//! - rmcp's Origin validation is turned ON explicitly ([`build_router`] sets
//!   `allowed_origins` to loopback forms too) — the library's own default is
//!   an *empty* list, which SKIPS Origin checking entirely
//!   (`validate_origin_header` in rmcp's vendored `tower.rs` returns `Ok`
//!   immediately when `allowed_origins.is_empty()`). The loopback entries
//!   below deliberately omit a port (`"http://127.0.0.1"`, not
//!   `"http://127.0.0.1:8765"`): rmcp's `origin_is_allowed` only compares the
//!   port when the *allow-list entry* itself specifies one
//!   (`a_port.is_none() || a_port == o_port`), so a portless entry matches
//!   every port for that host — required here since `port == 0` (OS-assigned,
//!   used by every test and by a fresh install before the user's chosen port
//!   is known) makes the real bound port unpredictable ahead of time.
//!   Requests with no `Origin` header (every real MCP client, since `Origin`
//!   is a browser-only header) still pass unconditionally either way.
//! - The bearer token is never logged: [`start_mcp_server`]'s only error
//!   paths name the port, never the token, and the token is wrapped in an
//!   `Arc<str>` that only the middleware closure ever reads.
//! - [`start_on_boot`] and [`reconcile_mcp_server`] both read
//!   `AiSettings::mcp_enabled` before ever calling [`start_mcp_server`] — the
//!   server never binds a socket when MCP is switched off.
//!
//! ## Shutdown design
//! [`stop_mcp_server`] is "graceful-enough" by design (per the task brief):
//! it sends a best-effort `oneshot` signal (observed by
//! [`axum::serve`]'s `with_graceful_shutdown`, in case nothing else is
//! mid-flight) and then unconditionally aborts the server task, awaiting the
//! aborted handle so the listening socket is guaranteed closed — and the
//! port free — before this function returns. No `tokio-util`
//! `CancellationToken`: std/tokio primitives only, per the plan's
//! constraint. rmcp's own `StreamableHttpServerConfig` still carries an
//! internal `CancellationToken` (it's a required, non-optional dependency of
//! rmcp itself), but we never construct or touch one — we only ever call
//! `StreamableHttpServerConfig::default()` and builder methods that don't
//! touch that field, so it never appears in this crate's own code.
//!
//! ## API drift vs the verified rmcp notes
//! The task's `rmcp-22-api-notes.md` suggested
//! `StreamableHttpServerConfig::default().with_json_response(true)` would
//! make POST responses plain `application/json`. Reading rmcp 2.2.0's own
//! vendored source and its `tests/test_streamable_http_json_response.rs`
//! (`json_response_ignored_in_stateful_mode`) shows `json_response` only
//! takes effect when `stateful_mode` is ALSO set to `false`
//! (`tower.rs`'s `handle_post`: the `json_response` branch is unreachable
//! under the `if let Some(session_manager) = ...` — i.e. stateful — arm).
//! We deliberately keep `stateful_mode` at its default (`true`): that is
//! the session-ID-bearing, spec-standard mode real MCP clients (Claude
//! Code's `--transport http`, `mcp-remote`) expect, and it is what the task
//! brief's own test-client guidance describes ("echo the `Mcp-Session-Id`
//! response header on follow-up POSTs"). So `with_json_response` is NOT
//! called here — it would be a no-op in this configuration, and setting it
//! would misleadingly suggest otherwise to a future reader. Test responses
//! are parsed as SSE (see `tests::extract_json_rpc_messages`).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{Request, State as AxumState};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use tauri::State;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

use crate::ai_commands::config_dir;
use crate::ai_mcp::OpenTuneMcp;
use crate::ai_settings::{
    load_ai_settings_in, load_or_create_mcp_token_in, regenerate_mcp_token_in,
};
use crate::ai_tools::AiExecutorState;
use crate::dto::McpStatusDto;
use crate::owner::OwnerHandle;

const BEARER_PREFIX: &str = "Bearer ";

/// Constant-time token comparison: a length check up front (token length
/// itself is not the secret — only its bytes are) followed by an xor-fold
/// over every byte pair, so no early `return false` on the first mismatched
/// byte can leak *which* byte differed through timing. Dependency-free by
/// design (the plan calls out a naive `==` here as a review finding) — no
/// `subtle`/`constant_time_eq` crate needed for one short hex string.
pub(crate) fn tokens_match(candidate: &[u8], expected: &[u8]) -> bool {
    if candidate.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in candidate.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// axum middleware: every request under `/mcp` must carry
/// `Authorization: Bearer <token>` matching the per-install token
/// ([`tokens_match`]). Anything else — missing header, wrong scheme, wrong
/// token — gets a bare 401 with no body (never echoes what was sent).
async fn require_bearer_token(
    AxumState(expected_token): AxumState<Arc<str>>,
    request: Request,
    next: Next,
) -> Response {
    let provided = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix(BEARER_PREFIX));

    match provided {
        Some(candidate) if tokens_match(candidate.as_bytes(), expected_token.as_bytes()) => {
            next.run(request).await
        }
        _ => StatusCode::UNAUTHORIZED.into_response(),
    }
}

/// Build the one-route (`/mcp`) axum router: rmcp's [`StreamableHttpService`]
/// (constructing a fresh [`OpenTuneMcp`] per session from the shared
/// executor slot, owner handle, and config dir — never a pinned handler
/// instance) behind the bearer-auth layer above.
fn build_router(
    executor_state: Arc<AiExecutorState>,
    owner: OwnerHandle,
    config_dir: PathBuf,
    token: String,
) -> Router {
    let service = StreamableHttpService::new(
        move || {
            Ok(OpenTuneMcp::new(
                Arc::clone(&executor_state),
                owner.clone(),
                config_dir.clone(),
            ))
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default()
            // Explicit, not just relying on the library default: pins the
            // DNS-rebinding-prevention Host check to loopback forms so a
            // future rmcp default change can't silently widen it here.
            .with_allowed_hosts(["localhost", "127.0.0.1", "::1"])
            // The library default for `allowed_origins` is an EMPTY list,
            // which skips Origin checking entirely — unlike `allowed_hosts`,
            // leaving this unset would silently turn Origin validation OFF.
            // Portless entries so any bound port matches (see the module
            // doc's "Security invariants" section for why).
            .with_allowed_origins(["http://localhost", "http://127.0.0.1", "http://[::1]"]),
    );

    Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(
            Arc::<str>::from(token),
            require_bearer_token,
        ))
}

/// One running server's lifecycle handle: the task future (spawned via
/// `tokio::spawn`, not `tauri::async_runtime::spawn` — this module has no
/// dependency on a live `tauri::AppHandle`) plus the shutdown signal.
struct RunningServer {
    shutdown_tx: oneshot::Sender<()>,
    join_handle: JoinHandle<()>,
    local_addr: SocketAddr,
}

/// Managed app-wide: the current MCP server's lifecycle state, if any.
/// `None` when stopped (including "never started").
#[derive(Default)]
pub struct McpServerState(Mutex<Option<RunningServer>>);

impl McpServerState {
    /// The bound address, if the server is currently running. `Mutex`
    /// poisoning is recovered from (a panic in another accessor should not
    /// permanently wedge status queries) rather than propagated — mirrors
    /// `lib.rs`'s `binding_gen::BINDINGS_LOCK` recovery idiom.
    fn local_addr(&self) -> Option<SocketAddr> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|running| running.local_addr)
    }
}

/// Bind a loopback listener and start serving MCP over it. Binds
/// `127.0.0.1` ONLY (never `0.0.0.0` — never exposed off-box). `port == 0`
/// asks the OS for a free ephemeral port; read it back afterwards via
/// [`McpServerState::local_addr`] (used by tests instead of a fixed port +
/// sleep). Errors (most commonly: the configured port is already taken) are
/// a plain user-readable `String` — never the token, never an internal
/// path/stack trace.
///
/// Returns `Err` without side effects if a server is already running on
/// this `state` — callers (`reconcile_mcp_server`, `start_on_boot`) always
/// `stop_mcp_server` first when they mean to restart on a new port.
pub async fn start_mcp_server(
    state: &McpServerState,
    executor_state: Arc<AiExecutorState>,
    owner: OwnerHandle,
    config_dir: PathBuf,
    token: String,
    port: u16,
) -> Result<(), String> {
    if state.local_addr().is_some() {
        return Err("MCP server is already running — stop it first".into());
    }

    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| format!("could not start the MCP server on 127.0.0.1:{port}: {e}"))?;
    let local_addr = listener
        .local_addr()
        .map_err(|e| format!("could not read the MCP server's bound address: {e}"))?;

    let router = build_router(executor_state, owner, config_dir, token);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let join_handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    let mut guard = state
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(RunningServer {
        shutdown_tx,
        join_handle,
        local_addr,
    });
    Ok(())
}

/// Stop the running server, if any — idempotent (a second call finds
/// nothing to stop and returns immediately). See the module doc's
/// "Shutdown design" section for why this is signal-then-abort rather than
/// a bounded graceful drain.
pub async fn stop_mcp_server(state: &McpServerState) {
    let running = {
        let mut guard = state
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.take()
    };
    let Some(running) = running else {
        return;
    };
    // Best-effort: observed by `with_graceful_shutdown` if the task happens
    // to still be polling it when this arrives. Harmless if the abort below
    // wins the race instead — the abort guarantees the listener closes
    // either way.
    let _ = running.shutdown_tx.send(());
    running.join_handle.abort();
    // Await the aborted handle rather than returning immediately: this is
    // what makes "stop, then connect" deterministic in tests (no sleep) —
    // by the time this resolves, the task (and the `TcpListener` it owned)
    // has actually been torn down and the port is free.
    let _ = running.join_handle.await;
}

/// Pure decision matrix for reconciling desired MCP settings against the
/// currently running server — extracted so it is unit-testable without any
/// real task or socket (mirrors `lib.rs`'s `should_defer_exit` pattern).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconcileAction {
    /// Desired state already matches reality.
    NoOp,
    /// Not running, should be: bind and start.
    Start,
    /// Running, should not be: stop.
    Stop,
    /// Running on the wrong port: stop, then start on the new one.
    Restart,
}

fn reconcile_action(
    desired_enabled: bool,
    desired_port: u16,
    running_port: Option<u16>,
) -> ReconcileAction {
    match (desired_enabled, running_port) {
        (false, None) => ReconcileAction::NoOp,
        (false, Some(_)) => ReconcileAction::Stop,
        (true, None) => ReconcileAction::Start,
        (true, Some(port)) if port == desired_port => ReconcileAction::NoOp,
        (true, Some(_)) => ReconcileAction::Restart,
    }
}

/// Reconcile the server's running state with freshly saved settings —
/// called from `ai_commands::set_ai_settings` on every successful save.
/// Covers all three transitions: enable (Start), disable (Stop), and a port
/// change while already enabled (Restart, via `reconcile_action`'s own
/// branch). Loads/creates the bearer token itself (only needed on the
/// Start/Restart paths) so callers never have to compute one just to
/// possibly not use it.
pub async fn reconcile_mcp_server(
    state: &McpServerState,
    executor_state: Arc<AiExecutorState>,
    owner: OwnerHandle,
    config_dir: PathBuf,
    desired_enabled: bool,
    desired_port: u16,
) -> Result<(), String> {
    let running_port = state.local_addr().map(|addr| addr.port());
    let action = reconcile_action(desired_enabled, desired_port, running_port);

    if matches!(action, ReconcileAction::Stop | ReconcileAction::Restart) {
        stop_mcp_server(state).await;
    }
    if matches!(action, ReconcileAction::Start | ReconcileAction::Restart) {
        let token = load_or_create_mcp_token_in(&config_dir)?;
        start_mcp_server(
            state,
            executor_state,
            owner,
            config_dir,
            token,
            desired_port,
        )
        .await?;
    }
    Ok(())
}

/// Regenerate the per-install MCP bearer token in `config_dir` and, if a
/// server is currently running on `state`, restart it on the *same* port so
/// the fresh token takes effect immediately. Without this, a running server
/// keeps accepting the old (possibly leaked) token forever — and the newly
/// issued token 401s — until some unrelated later reconcile or app restart,
/// the opposite of what "regenerate" is meant to accomplish. A no-op restart
/// (server left stopped) when nothing is running: regenerating the token
/// must not have the side effect of starting a server the user never asked
/// to enable. This is the core function `ai_commands::mcp_token_info` calls
/// for `regenerate: true` — extracted here (mirrors [`reconcile_mcp_server`])
/// so this module's own loopback-HTTP test helpers can drive it directly.
pub(crate) async fn regenerate_mcp_token(
    state: &McpServerState,
    executor_state: Arc<AiExecutorState>,
    owner: OwnerHandle,
    config_dir: PathBuf,
) -> Result<String, String> {
    let token = regenerate_mcp_token_in(&config_dir)?;
    if let Some(port) = state.local_addr().map(|addr| addr.port()) {
        stop_mcp_server(state).await;
        start_mcp_server(
            state,
            executor_state,
            owner,
            config_dir,
            token.clone(),
            port,
        )
        .await?;
    }
    Ok(token)
}

/// Start the server at app boot if MCP was already enabled in a previous
/// run. A no-op (`Ok(())`), not an error, when it's disabled — most app
/// launches never touch the network. Reads settings and the executor/owner
/// state straight off the `AppHandle` since, unlike `set_ai_settings`, there
/// is no in-flight IPC call to carry them.
pub async fn start_on_boot(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;

    let dir = config_dir(app)?;
    let settings = load_ai_settings_in(&dir)?;
    if !settings.mcp_enabled {
        return Ok(());
    }

    let token = load_or_create_mcp_token_in(&dir)?;
    let executor_state = app.state::<Arc<AiExecutorState>>().inner().clone();
    let owner = app.state::<OwnerHandle>().inner().clone();
    let mcp_state = app.state::<McpServerState>();

    start_mcp_server(
        &mcp_state,
        executor_state,
        owner,
        dir,
        token,
        settings.mcp_port,
    )
    .await
}

/// Current server status for the Settings UI (task 5). `port` is the real
/// bound port when running (meaningful even when the configured port was
/// `0`, e.g. in tests) and `0` when stopped.
#[tauri::command]
#[specta::specta]
pub async fn mcp_status(state: State<'_, McpServerState>) -> Result<McpStatusDto, String> {
    Ok(match state.local_addr() {
        Some(addr) => McpStatusDto {
            running: true,
            port: addr.port(),
        },
        None => McpStatusDto {
            running: false,
            port: 0,
        },
    })
}

#[cfg(test)]
#[path = "ai_mcp_server_tests.rs"]
mod tests;
