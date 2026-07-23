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
use crate::ai_settings::{load_ai_settings_in, load_or_create_mcp_token_in};
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
            .with_allowed_hosts(["localhost", "127.0.0.1", "::1"]),
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
mod tests {
    use super::*;
    use crate::owner::spawn_owner_with_emitter;

    /// An unconnected owner — sufficient for every test in this module: the
    /// 401 cases never reach the handler at all, and the handshake test
    /// only exercises `initialize`/`tools/list`, neither of which touches
    /// the connection (unlike `call_tool`, which `ai_mcp.rs`'s own tests
    /// already cover against a connected simulator).
    fn bare_owner() -> OwnerHandle {
        spawn_owner_with_emitter(Arc::new(|_| {}))
    }

    fn temp_config_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "opentune-ai-mcp-server-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp config dir created");
        dir
    }

    /// Every SSE event block's `data:` line, parsed as JSON — real
    /// JSON-RPC messages only. Priming events (`SEP-1699`) carry an empty
    /// `data:` line, so they fail to parse and are silently skipped; no
    /// special-casing needed regardless of whether a priming event
    /// precedes the real response.
    fn extract_json_rpc_messages(sse_body: &str) -> Vec<serde_json::Value> {
        sse_body
            .split("\n\n")
            .filter_map(|event| {
                event
                    .lines()
                    .find_map(|line| line.strip_prefix("data:"))
                    .map(str::trim)
                    .filter(|data| !data.is_empty())
                    .and_then(|data| serde_json::from_str(data).ok())
            })
            .collect()
    }

    const INITIALIZE_BODY: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#;
    const INITIALIZED_NOTIFICATION: &str =
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    const TOOLS_LIST_BODY: &str = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;

    // ── constant-time compare ────────────────────────────────────────────

    #[test]
    fn tokens_match_identical_bytes() {
        assert!(tokens_match(b"a-real-token", b"a-real-token"));
    }

    #[test]
    fn tokens_match_rejects_unequal_bytes_of_equal_length() {
        assert!(!tokens_match(b"a-real-token", b"a-fake-token"));
    }

    #[test]
    fn tokens_match_rejects_length_mismatch() {
        assert!(!tokens_match(b"short", b"a-much-longer-candidate"));
        assert!(!tokens_match(b"a-much-longer-candidate", b"short"));
    }

    // ── reconcile_action (pure) ──────────────────────────────────────────

    #[test]
    fn reconcile_disabled_and_not_running_is_a_noop() {
        assert_eq!(reconcile_action(false, 8765, None), ReconcileAction::NoOp);
    }

    #[test]
    fn reconcile_disabling_a_running_server_stops_it() {
        assert_eq!(
            reconcile_action(false, 8765, Some(8765)),
            ReconcileAction::Stop
        );
    }

    #[test]
    fn reconcile_enabling_when_not_running_starts_it() {
        assert_eq!(reconcile_action(true, 8765, None), ReconcileAction::Start);
    }

    #[test]
    fn reconcile_enabled_on_the_same_port_is_a_noop() {
        assert_eq!(
            reconcile_action(true, 8765, Some(8765)),
            ReconcileAction::NoOp
        );
    }

    #[test]
    fn reconcile_enabled_on_a_different_port_restarts() {
        assert_eq!(
            reconcile_action(true, 9000, Some(8765)),
            ReconcileAction::Restart
        );
    }

    // ── real loopback HTTP integration tests ────────────────────────────

    #[tokio::test]
    async fn request_without_a_bearer_token_is_rejected() {
        let dir = temp_config_dir("no-token");
        let state = McpServerState::default();
        start_mcp_server(
            &state,
            Arc::new(AiExecutorState::default()),
            bare_owner(),
            dir,
            "the-real-token".into(),
            0,
        )
        .await
        .expect("server starts on an OS-assigned port");
        let port = state.local_addr().expect("running").port();

        let response = crate::ai_provider::http_client()
            .post(format!("http://127.0.0.1:{port}/mcp"))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .body(INITIALIZE_BODY)
            .send()
            .await
            .expect("request reaches the server");

        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);

        stop_mcp_server(&state).await;
    }

    #[tokio::test]
    async fn request_with_the_wrong_bearer_token_is_rejected() {
        let dir = temp_config_dir("wrong-token");
        let state = McpServerState::default();
        start_mcp_server(
            &state,
            Arc::new(AiExecutorState::default()),
            bare_owner(),
            dir,
            "the-real-token".into(),
            0,
        )
        .await
        .expect("server starts on an OS-assigned port");
        let port = state.local_addr().expect("running").port();

        let response = crate::ai_provider::http_client()
            .post(format!("http://127.0.0.1:{port}/mcp"))
            .header("Authorization", "Bearer not-the-real-token")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .body(INITIALIZE_BODY)
            .send()
            .await
            .expect("request reaches the server");

        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);

        stop_mcp_server(&state).await;
    }

    #[tokio::test]
    async fn correct_token_completes_the_initialize_and_tools_list_handshake() {
        let dir = temp_config_dir("handshake");
        let state = McpServerState::default();
        let token = "the-real-token";
        start_mcp_server(
            &state,
            Arc::new(AiExecutorState::default()),
            bare_owner(),
            dir,
            token.into(),
            0,
        )
        .await
        .expect("server starts on an OS-assigned port");
        let port = state.local_addr().expect("running").port();
        let url = format!("http://127.0.0.1:{port}/mcp");
        let client = crate::ai_provider::http_client();
        let auth = format!("Bearer {token}");

        let init_response = client
            .post(&url)
            .header("Authorization", &auth)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .body(INITIALIZE_BODY)
            .send()
            .await
            .expect("initialize request sent");
        assert_eq!(init_response.status(), 200);
        let session_id = init_response
            .headers()
            .get("mcp-session-id")
            .expect("stateful mode issues a session id")
            .to_str()
            .expect("session id header is ASCII")
            .to_owned();
        let init_body = init_response.text().await.expect("initialize body read");
        let init_messages = extract_json_rpc_messages(&init_body);
        assert_eq!(init_messages.len(), 1, "got: {init_body}");
        assert_eq!(init_messages[0]["id"], 1);
        assert!(init_messages[0]["result"].is_object());

        let notify_status = client
            .post(&url)
            .header("Authorization", &auth)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("mcp-session-id", &session_id)
            .body(INITIALIZED_NOTIFICATION)
            .send()
            .await
            .expect("notifications/initialized sent")
            .status();
        assert_eq!(notify_status, 202);

        let list_response = client
            .post(&url)
            .header("Authorization", &auth)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("mcp-session-id", &session_id)
            .body(TOOLS_LIST_BODY)
            .send()
            .await
            .expect("tools/list sent");
        assert_eq!(list_response.status(), 200);
        let list_body = list_response.text().await.expect("tools/list body read");
        let list_messages = extract_json_rpc_messages(&list_body);
        assert_eq!(list_messages.len(), 1, "got: {list_body}");
        let tools = list_messages[0]["result"]["tools"]
            .as_array()
            .expect("result.tools is an array");
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().expect("tool name is a string"))
            .collect();
        assert!(names.contains(&"read_tune"), "got: {names:?}");
        assert!(
            !names.contains(&"apply_change"),
            "advisory list excludes apply_change: {names:?}"
        );

        stop_mcp_server(&state).await;
    }

    #[tokio::test]
    async fn stopped_server_refuses_new_connections() {
        let dir = temp_config_dir("stop");
        let state = McpServerState::default();
        start_mcp_server(
            &state,
            Arc::new(AiExecutorState::default()),
            bare_owner(),
            dir,
            "the-real-token".into(),
            0,
        )
        .await
        .expect("server starts on an OS-assigned port");
        let port = state.local_addr().expect("running").port();

        stop_mcp_server(&state).await;
        assert!(state.local_addr().is_none(), "state clears on stop");

        let result = crate::ai_provider::http_client()
            .get(format!("http://127.0.0.1:{port}/mcp"))
            .send()
            .await;
        assert!(
            result.is_err(),
            "expected connection refused after stop, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn starting_twice_on_the_same_state_is_rejected_with_a_readable_error() {
        let dir = temp_config_dir("already-running");
        let state = McpServerState::default();
        start_mcp_server(
            &state,
            Arc::new(AiExecutorState::default()),
            bare_owner(),
            dir.clone(),
            "the-real-token".into(),
            0,
        )
        .await
        .expect("first start succeeds");

        let err = start_mcp_server(
            &state,
            Arc::new(AiExecutorState::default()),
            bare_owner(),
            dir,
            "the-real-token".into(),
            0,
        )
        .await
        .expect_err("second start on the same state must fail");
        assert!(
            err.to_lowercase().contains("running"),
            "error should explain the server is already running: {err}"
        );

        stop_mcp_server(&state).await;
    }

    #[tokio::test]
    async fn bind_failure_on_a_taken_port_is_a_readable_error() {
        let dir_a = temp_config_dir("bind-conflict-a");
        let state_a = McpServerState::default();
        start_mcp_server(
            &state_a,
            Arc::new(AiExecutorState::default()),
            bare_owner(),
            dir_a,
            "token-a".into(),
            0,
        )
        .await
        .expect("first server binds an OS-assigned port");
        let taken_port = state_a.local_addr().expect("running").port();

        let dir_b = temp_config_dir("bind-conflict-b");
        let state_b = McpServerState::default();
        let err = start_mcp_server(
            &state_b,
            Arc::new(AiExecutorState::default()),
            bare_owner(),
            dir_b,
            "token-b".into(),
            taken_port,
        )
        .await
        .expect_err("binding an already-taken port must fail");
        assert!(
            err.contains(&taken_port.to_string()),
            "error should name the port: {err}"
        );

        stop_mcp_server(&state_a).await;
    }
}
