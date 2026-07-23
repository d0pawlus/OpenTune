// SPDX-License-Identifier: GPL-3.0-or-later
//! MCP server handler (M7 slice 4, task 3): maps the shared tool registry
//! and executor onto rmcp's [`ServerHandler`] trait. This module owns the
//! MCP-facing translation only — no network, no HTTP (that's task 4's
//! `ai_mcp_server.rs`); tests here call the handler's own methods directly.
//!
//! `list_tools`/`call_tool` delegate to plain inherent methods
//! ([`OpenTuneMcp::advisory_tools`], [`OpenTuneMcp::dispatch`]) that ignore
//! rmcp's `RequestContext` entirely — this crate has no public way to
//! construct one outside a real transport (`Peer::new` is `pub(crate)` in
//! rmcp), and the MCP contract here never needs to talk back to the peer
//! (no notifications, no sampling). Splitting the logic out keeps it
//! testable with direct async calls instead of a full client/server
//! transport pair.
//!
//! ## RED (TDD)
//! `cargo test --lib ai_mcp` with `advisory_tools` stubbed to `Vec::new()`
//! and `dispatch` stubbed to always return a fixed tool-level error (both
//! ignoring their real inputs) fails all four tests below for the right
//! reason: empty tool list, unexpected tool-level error on `read_tune`, zero
//! audit lines, and no proposal recorded — never a compile error. See the
//! task report for the captured `cargo test` output (RED reconstructed by
//! re-stubbing this already-implemented file, then reverting).

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{Map as JsonMap, Value as Json};

use opentune_ai::{available_tools, AuditChannel, PermissionPolicy, ToolSpec};

use crate::ai_tools::{AiExecutorState, ToolError, ToolErrorKind};
use crate::owner::OwnerHandle;

/// The MCP contract shown to external agents — adapted from
/// `ai_chat::system_prompt`'s advisory rules (the MCP client's own model
/// needs the same never-apply framing, since it never sees the app's UI).
const MCP_INSTRUCTIONS: &str = "OpenTune's tool registry, served at the advisory authority \
     level. All numeric analysis comes from tools — never invent or compute \
     tuning numbers yourself. You can NEVER apply changes to the ECU or burn \
     to flash: apply_change and burn_now are not offered here. To suggest a \
     change, call propose_change; the user reviews and applies it manually \
     inside OpenTune. Never claim a change was applied. If a tool returns an \
     error or a proposal comes back not-ok, report the reason honestly.";

/// One MCP server instance over the shared tool engine. Holds the executor
/// **slot**, not an executor — `dispatch` resolves the current shared
/// executor via [`AiExecutorState::get_or_build`] on every call, never
/// pinning an `Arc<AiToolExecutor>` here. `ai_reset` (the embedded
/// assistant's reset command) replaces the shared slot with `None`; a
/// pinned Arc would silently diverge from that (MCP still serving the old
/// executor's rate-limit budget and proposal log while the assistant gets a
/// fresh one). See `reset_convergence_next_call_uses_a_new_executor` below.
pub struct OpenTuneMcp {
    executor_state: Arc<AiExecutorState>,
    owner: OwnerHandle,
    config_dir: PathBuf,
}

impl OpenTuneMcp {
    pub fn new(
        executor_state: Arc<AiExecutorState>,
        owner: OwnerHandle,
        config_dir: PathBuf,
    ) -> Self {
        Self {
            executor_state,
            owner,
            config_dir,
        }
    }

    /// The advisory tool list, converted to rmcp's `Tool` shape. A plain
    /// async fn rather than the `ServerHandler::list_tools` trait method
    /// itself — see the module doc on why tests call this directly.
    async fn advisory_tools(&self) -> Vec<Tool> {
        available_tools(&PermissionPolicy::advisory())
            .into_iter()
            .map(to_rmcp_tool)
            .collect()
    }

    /// Resolve the shared executor for THIS call (never pinned — see the
    /// struct doc) and run one tool call audited as [`AuditChannel::Mcp`].
    /// Never returns an `Err`: both executor-resolution failures and
    /// [`ToolError`]s become tool-level error content — data for the
    /// calling model, per MCP's two-failure-mode contract — never a
    /// JSON-RPC protocol error.
    async fn dispatch(
        &self,
        name: &str,
        arguments: Option<JsonMap<String, Json>>,
    ) -> CallToolResult {
        let executor = match self
            .executor_state
            .get_or_build(&self.owner, &self.config_dir)
        {
            Ok(executor) => executor,
            Err(message) => return tool_error(message),
        };
        let input = Json::Object(arguments.unwrap_or_default());
        match executor.execute_as(AuditChannel::Mcp, name, input).await {
            Ok(value) => CallToolResult::structured(value),
            Err(err) => tool_error(tool_error_message(&err)),
        }
    }
}

/// A tool-level MCP error result carrying `message` as plain text content.
fn tool_error(message: String) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message)])
}

/// Map a [`ToolErrorKind`] + message into the MCP-facing error text. Unlike
/// the embedded assistant (`ai_chat.rs::execute_tool_block`, which forwards
/// `err.message` alone — the chat UI already has its own success/failure
/// affordance), an external MCP client has no side channel for *why* a call
/// failed beyond this text, so the kind is folded into the message.
fn tool_error_message(err: &ToolError) -> String {
    let kind = match err.kind {
        ToolErrorKind::Denied => "denied",
        ToolErrorKind::InvalidInput => "invalid_input",
        ToolErrorKind::Failed => "failed",
    };
    format!("{kind}: {}", err.message)
}

/// `opentune_ai::registry()`'s own tests guarantee every `input_schema` is
/// an object schema (`every_schema_is_an_object_schema`) — `unwrap_or_default`
/// is a defensive fallback for that invariant, not a swallowed error path.
fn to_rmcp_tool(spec: ToolSpec) -> Tool {
    let schema = spec.input_schema.as_object().cloned().unwrap_or_default();
    Tool::new(spec.name, spec.description, Arc::new(schema))
}

impl ServerHandler for OpenTuneMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("opentune", env!("CARGO_PKG_VERSION")))
            .with_instructions(MCP_INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(self.advisory_tools().await))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        Ok(self.dispatch(&request.name, request.arguments).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_tools::AI_AUDIT_FILE;
    use crate::connection::ConnectSource;
    use crate::owner::{request, spawn_owner_with_emitter, Command, Emitter};

    /// A simulator-backed, connected owner — mirrors `ai_tools.rs`'s and
    /// `ai_chat_tests.rs`'s `connected_executor` arrangement, minus the
    /// executor itself (this module resolves that per call via
    /// `AiExecutorState`, not a directly-constructed `AiToolExecutor`).
    async fn connected_owner() -> OwnerHandle {
        let emit: Emitter = Arc::new(|_| {});
        let owner = spawn_owner_with_emitter(emit);
        request(&owner, |reply| Command::Connect {
            source: ConnectSource::Simulator { ini_path: None },
            reply,
        })
        .await
        .expect("simulator connects");
        request(&owner, |reply| Command::LoadTune { reply })
            .await
            .expect("tune loads");
        owner
    }

    /// A fresh, empty per-test config dir (mirrors `ai_commands.rs`'s test
    /// helpers) — `dispatch` writes the real `FileAuditSink` here via
    /// `AiExecutorState::get_or_build`, so audit assertions read it back.
    fn temp_config_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("opentune-ai-mcp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp config dir created");
        dir
    }

    fn audit_lines(dir: &std::path::Path) -> Vec<Json> {
        std::fs::read_to_string(dir.join(AI_AUDIT_FILE))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("audit line is valid JSON"))
            .collect()
    }

    fn args(pairs: Vec<(&str, Json)>) -> JsonMap<String, Json> {
        pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
    }

    #[tokio::test]
    async fn list_tools_returns_exactly_the_advisory_tool_names() {
        let owner = connected_owner().await;
        let handler = OpenTuneMcp::new(
            Arc::new(AiExecutorState::default()),
            owner,
            temp_config_dir("list"),
        );
        let tools = handler.advisory_tools().await;
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(
            names,
            vec![
                "read_tune",
                "read_realtime",
                "run_ve_analyze",
                "get_log_stats",
                "detect_anomaly",
                "virtual_dyno",
                "propose_change",
            ],
            "advisory list, in registry order, apply_change/burn_now excluded"
        );
    }

    #[tokio::test]
    async fn call_tool_read_tune_returns_values_content() {
        let owner = connected_owner().await;
        let handler = OpenTuneMcp::new(
            Arc::new(AiExecutorState::default()),
            owner,
            temp_config_dir("read"),
        );
        let result = handler
            .dispatch(
                "read_tune",
                Some(args(vec![("names", serde_json::json!(["reqFuel"]))])),
            )
            .await;

        assert_ne!(result.is_error, Some(true), "read_tune must not error");
        let structured = result
            .structured_content
            .expect("success result carries structured content");
        assert!(structured["values"].is_array());
        assert_eq!(structured["values"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn call_tool_apply_change_is_a_tool_level_error_audited_as_mcp_denied() {
        let owner = connected_owner().await;
        let dir = temp_config_dir("deny");
        let handler = OpenTuneMcp::new(Arc::new(AiExecutorState::default()), owner, dir.clone());

        let result = handler
            .dispatch(
                "apply_change",
                Some(args(vec![("proposal_id", serde_json::json!(1))])),
            )
            .await;

        assert_eq!(
            result.is_error,
            Some(true),
            "apply_change is locked at advisory — a tool-level error, not a panic"
        );

        let lines = audit_lines(&dir);
        assert_eq!(lines.len(), 1, "exactly one audit line: {lines:?}");
        assert_eq!(lines[0]["channel"], "mcp");
        assert_eq!(lines[0]["outcome"]["kind"], "denied");
    }

    #[tokio::test]
    async fn reset_convergence_next_call_uses_a_new_executor() {
        let owner = connected_owner().await;
        let dir = temp_config_dir("reset");
        let executor_state = Arc::new(AiExecutorState::default());
        let handler = OpenTuneMcp::new(Arc::clone(&executor_state), owner, dir);

        let proposal_args = || {
            Some(args(vec![
                ("constant", serde_json::json!("reqFuel")),
                ("edits", serde_json::json!([{ "index": 0, "value": 13.0 }])),
                ("reason", serde_json::json!("first")),
            ]))
        };

        let before = handler.dispatch("propose_change", proposal_args()).await;
        assert_ne!(before.is_error, Some(true), "first proposal recorded");
        let built = executor_state
            .get_or_build(&handler.owner, &handler.config_dir)
            .expect("executor resolvable before reset");
        assert_eq!(built.proposals().len(), 1, "one proposal before reset");

        // Simulate `ai_reset`: the shared executor slot goes back to None.
        *executor_state.0.lock().unwrap() = None;

        let after = handler.dispatch("propose_change", proposal_args()).await;
        assert_ne!(after.is_error, Some(true), "second proposal recorded");

        let rebuilt = executor_state
            .get_or_build(&handler.owner, &handler.config_dir)
            .expect("executor resolvable after reset");
        assert_eq!(
            rebuilt.proposals().len(),
            1,
            "a NEW executor with its own fresh proposal log — the pre-reset \
             proposal must be gone, not carried over as a second entry"
        );
    }
}
