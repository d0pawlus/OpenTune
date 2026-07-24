// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests for the MCP HTTP transport (`ai_mcp_server.rs`): the constant-time
//! bearer-token compare, the pure `reconcile_action` decision matrix, and
//! real loopback HTTP integration tests (auth, the initialize/tools-list
//! handshake, start/stop lifecycle, token regeneration restarting a running
//! server, and Origin validation) driven against an actual bound
//! `TcpListener` — no mocked transport.

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
const INITIALIZED_NOTIFICATION: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
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

// ── regenerate_mcp_token restarts a running server ──────────────────

#[tokio::test]
async fn regenerating_the_token_while_running_restarts_it_with_the_fresh_token() {
    let dir = temp_config_dir("regenerate-running");
    let old_token = load_or_create_mcp_token_in(&dir).expect("token A written to disk");
    let state = McpServerState::default();
    start_mcp_server(
        &state,
        Arc::new(AiExecutorState::default()),
        bare_owner(),
        dir.clone(),
        old_token.clone(),
        0,
    )
    .await
    .expect("server starts on an OS-assigned port with token A");
    let port_before = state.local_addr().expect("running").port();

    let new_token = regenerate_mcp_token(
        &state,
        Arc::new(AiExecutorState::default()),
        bare_owner(),
        dir.clone(),
    )
    .await
    .expect("regenerate succeeds while the server is running");
    assert_ne!(new_token, old_token, "a fresh token must be issued");

    let port_after = state
        .local_addr()
        .expect("still running after the restart")
        .port();
    assert_eq!(port_before, port_after, "restart reuses the same port");

    let token_on_disk =
        load_or_create_mcp_token_in(&dir).expect("token file readable after regenerate");
    assert_eq!(
        token_on_disk, new_token,
        "regenerate returns the same token it wrote to disk"
    );

    let url = format!("http://127.0.0.1:{port_after}/mcp");
    let client = crate::ai_provider::http_client();

    let old_token_response = client
        .post(&url)
        .header("Authorization", format!("Bearer {old_token}"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(INITIALIZE_BODY)
        .send()
        .await
        .expect("request reaches the restarted server");
    assert_eq!(
        old_token_response.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "the old (regenerated-away) token must no longer work"
    );

    let new_token_response = client
        .post(&url)
        .header("Authorization", format!("Bearer {new_token}"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(INITIALIZE_BODY)
        .send()
        .await
        .expect("request reaches the restarted server");
    assert_eq!(
        new_token_response.status(),
        200,
        "the fresh token must work against the restarted server"
    );

    stop_mcp_server(&state).await;
}

#[tokio::test]
async fn regenerating_the_token_while_not_running_does_not_start_it() {
    let dir = temp_config_dir("regenerate-not-running");
    let old_token = load_or_create_mcp_token_in(&dir).expect("token A written to disk");
    let state = McpServerState::default();

    let new_token = regenerate_mcp_token(
        &state,
        Arc::new(AiExecutorState::default()),
        bare_owner(),
        dir.clone(),
    )
    .await
    .expect("regenerate succeeds even when nothing is running");

    assert_ne!(new_token, old_token, "a fresh token must be issued");
    assert!(
        state.local_addr().is_none(),
        "regenerate must not start a server that was not already running"
    );

    let token_on_disk =
        load_or_create_mcp_token_in(&dir).expect("token file readable after regenerate");
    assert_eq!(
        token_on_disk, new_token,
        "the token file was actually rewritten"
    );
}

// ── Origin validation ────────────────────────────────────────────────

#[tokio::test]
async fn initialize_without_an_origin_header_still_succeeds() {
    let dir = temp_config_dir("origin-absent");
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

    // No `Origin` header at all: real MCP clients never send one (it is
    // a browser-only header) and this must keep working exactly as it
    // did before Origin validation was turned on.
    let response = crate::ai_provider::http_client()
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(INITIALIZE_BODY)
        .send()
        .await
        .expect("request reaches the server");

    assert_eq!(response.status(), 200);

    stop_mcp_server(&state).await;
}

#[tokio::test]
async fn initialize_with_a_disallowed_origin_is_rejected() {
    let dir = temp_config_dir("origin-disallowed");
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

    let response = crate::ai_provider::http_client()
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Origin", "http://evil.example.com")
        .body(INITIALIZE_BODY)
        .send()
        .await
        .expect("request reaches the server");

    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "a disallowed Origin must be rejected with exactly 403 (not just any 4xx, so a \
         future auth-layer 401 can't mask an origin-check regression)"
    );

    stop_mcp_server(&state).await;
}

#[tokio::test]
async fn initialize_with_an_allowed_loopback_origin_on_the_bound_port_succeeds() {
    // Backs the module doc's claim that the portless allow-list entries
    // (`"http://127.0.0.1"`, not `"http://127.0.0.1:<port>"`) match
    // every port for that host — required since the real bound port is
    // only known after `bind`, here and for a fresh install alike.
    let dir = temp_config_dir("origin-allowed");
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

    let response = crate::ai_provider::http_client()
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Origin", format!("http://127.0.0.1:{port}"))
        .body(INITIALIZE_BODY)
        .send()
        .await
        .expect("request reaches the server");

    assert_eq!(
        response.status(),
        200,
        "a loopback Origin on the actual bound port must be allowed"
    );

    stop_mcp_server(&state).await;
}
