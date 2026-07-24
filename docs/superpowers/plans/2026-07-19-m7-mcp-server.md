# M7 Slice 4 — MCP Server (Streamable HTTP, Bearer Token, Shared Tool Engine) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose OpenTune as an MCP server (decision D-2): rmcp Streamable HTTP bound to 127.0.0.1 with a per-install bearer token, serving the SAME tool registry through the SAME executor and guardrails as the embedded assistant — external agents (Claude Code natively; Claude Desktop via `mcp-remote`) get `advisory`-level access, and their proposals surface in the app's existing proposal-review UI.

**Architecture:** One `AiToolExecutor` is now shared between both access channels — the executor's audit channel moves from a construction-time field to a per-call parameter (`execute_as(channel, name, input)`), giving one rate-limiter budget and one proposal-id space across assistant + MCP (closing the issue-#29 "shared limiter/id space" decision; MCP proposals render in the same `ProposalCard`s). The MCP layer is two new app-side modules: `ai_mcp.rs` (a hand-implemented rmcp `ServerHandler` mapping `available_tools(advisory)` → MCP tools and `call_tool` → the shared executor with `AuditChannel::Mcp`) and `ai_mcp_server.rs` (axum router: bearer-token middleware → rmcp `StreamableHttpService`, bound to `127.0.0.1:<port>`, started/stopped by the settings toggle). The token is generated once per install into `app_config_dir/mcp-token` and displayed (with copy) in the AI settings panel. The webview CSP is untouched; the server binds loopback only, with rmcp's host/origin validation on (DNS-rebinding defense).

**Tech Stack:** `rmcp` 2.2 (pinned minor; the official MCP Rust SDK), `axum` (whatever version rmcp's streamable-http-server feature is built against — align, don't duplicate), `rand` for token bytes. Frontend: existing patterns only.

## Global Constraints

These bind every task and every reviewer:

- Work on branch `m7-mcp-server` off `main`, in worktree `.worktrees/m7-mcp-server`.
- TDD mandatory: failing test first (RED), then the fix (GREEN).
- Commit format: conventional commits; NO attribution footers of any kind.
- Rust gates before every commit (from `src-tauri/`, prefix `. "$HOME/.cargo/env" && `): `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Frontend gates when frontend files change: `npm run lint`, `npm run format:check`, `npx tsc --noEmit -p tsconfig.json`, `npm test`.
- New dependencies allowed ONLY: `rmcp = { version = "2.2", features = [...server + streamable-http-server transport...] }`, `axum` (version aligned with rmcp's own — check `cargo tree` after adding rmcp; if rmcp re-exports enough router surface, skip the direct axum dep entirely), `rand = "0.9"` (or `getrandom` if smaller — pick one, justify in report). Nothing else.
- **rmcp API verification (binding):** the recon that chose rmcp 2.2 predates implementation — before Tasks 3/4, the implementer MUST verify the actual 2.2 API against docs.rs (`ServerHandler` trait shape, `Tool`/`CallToolResult`/`ErrorData` types, `StreamableHttpService::new` signature, session-manager types, `StreamableHttpServerConfig` host/origin validation) and adapt; record every drift from this plan's sketches in the report.
- **Security invariants (review lens):** server binds `127.0.0.1` ONLY (never `0.0.0.0`); every request must carry `Authorization: Bearer <token>` (constant-time comparison — implement a dependency-free xor-fold; a plain `==` is a finding); rmcp host/origin validation stays ON; the MCP path can never reach `apply_change`/`burn` (same advisory policy object); every MCP tool call is audited with `AuditChannel::Mcp`; the token is never logged and never appears in the settings JSON (its own file); MCP server never starts when `mcp_enabled` is false.
- Settings forward-compat: the new `AiSettings` fields carry `#[serde(default)]` so existing `ai-settings.json` files load (this closes the #29 serde-default item for the fields this slice adds).
- Task 1 changes IPC DTOs → regenerate `src/ipc/bindings.ts` via `cargo test binding_gen`; never hand-edit.
- All user-facing strings through i18n, BOTH `en` and `pl`.
- Scope discipline; file cap 800 lines; SPDX headers on new files.

Existing interfaces consumed (verbatim unless a task modifies them):
- `AiToolExecutor` (`src-tauri/src/ai_tools.rs`): `new(owner, policy, limits, channel, audit)`, `execute(&self, name, input)`, `proposals()`; `ai_chat_commands.rs` builds it lazily inside `AiChatState` (executor `Mutex<Option<Arc<…>>>`) and `ai_send` uses it; `ai_reset` replaces it with `None`.
- `opentune_ai::{available_tools, PermissionPolicy, AuditChannel}`; `ToolSpec { name, description, kind, input_schema }`.
- `ai_settings.rs`: `AiSettings`, `save/load_ai_settings_in`, `atomic_write` (in layout.rs); `ai_commands.rs`: `AiSettingsDto` + get/set commands, `config_dir` (pub(crate)).
- Frontend: `AiSettingsPanel` (its `.ai-settings` section), i18n dictionaries, house test mocking.

---

### Task 1: Settings + token store (backend + DTO + bindings)

**Files:**
- Modify: `src-tauri/src/ai_settings.rs` (extend `AiSettings`; add token functions)
- Modify: `src-tauri/src/ai_commands.rs` + `src-tauri/src/dto.rs` (extend `AiSettingsDto`; new `mcp_token_info` command)
- Modify: `src-tauri/src/lib.rs` (register the new command)
- Modify (generated): `src/ipc/bindings.ts`
- Modify: `src-tauri/Cargo.toml` (add `rand`)

**Interfaces produced:**
- `AiSettings` gains `#[serde(default)] pub mcp_enabled: bool` and `#[serde(default = "default_mcp_port")] pub mcp_port: u16` with `pub const DEFAULT_MCP_PORT: u16 = 8765;` (`fn default_mcp_port() -> u16 { DEFAULT_MCP_PORT }`); `Default` impl updated accordingly.
- `pub const MCP_TOKEN_FILE: &str = "mcp-token";`
- `pub fn load_or_create_mcp_token_in(dir: &Path) -> Result<String, String>` — reads the token file; if absent, generates 32 random bytes hex-encoded (64 chars) via `rand`, writes it with `atomic_write`, returns it. Trims on read; rejects (regenerates) an empty/whitespace file.
- `pub fn regenerate_mcp_token_in(dir: &Path) -> Result<String, String>` — always generates + writes a fresh token.
- `AiSettingsDto` gains `mcp_enabled: bool` + `mcp_port: u16` (camelCase on the wire); `From` impls extended.
- New command `mcp_token_info(app, regenerate: bool) -> Result<String, String>` — returns the token (creating it if needed; regenerating when asked). This is the ONE place the token crosses IPC — needed so the user can copy it into their MCP client config; document that in the command doc comment.
- `set_ai_settings_in` validation extended: `mcp_port >= 1024` (non-privileged) — error otherwise.

**Steps (TDD):**
- [ ] **RED:** tests — old-shape JSON (no mcp fields) loads with defaults (this IS the serde-default proof); round-trip with mcp fields; `load_or_create` creates a 64-hex-char token, second call returns the SAME token, `regenerate` returns a DIFFERENT one; empty file regenerates; port validation rejects 80, accepts 8765.
- [ ] **GREEN:** implement; add `rand` dep; wire the command; `cargo test binding_gen` (expect `mcpTokenInfo` + extended `AiSettingsDto`).
- [ ] Gates (+ `npx tsc --noEmit`) + commit:

```bash
git commit -m "feat(ai): add MCP settings fields and per-install token"
```

---

### Task 2: Shared executor with per-call audit channel

**Files:**
- Modify: `src-tauri/src/ai_tools.rs` (channel: field → per-call parameter)
- Modify: `src-tauri/src/ai_chat.rs` + `src-tauri/src/ai_chat_tests.rs` (call-site updates)
- Modify: `src-tauri/src/ai_chat_commands.rs` (executor becomes the app-wide shared instance)
- Modify: `src-tauri/src/lib.rs` (manage the shared executor state)

**Interfaces produced (Task 3/4 consume):**
- `AiToolExecutor::new(owner, policy, limits, audit)` — `channel` REMOVED from the constructor.
- `pub async fn execute_as(&self, channel: AuditChannel, name: &str, input: Json) -> Result<Json, ToolError>` — the real path; `pub async fn execute(&self, name, input)` becomes a thin `execute_as(AuditChannel::Assistant, …)` shim so existing chat-loop call sites stay valid (keep or inline — implementer's call, but the chat loop MUST audit as Assistant and MCP as Mcp).
- `pub struct AiExecutorState(pub Mutex<Option<Arc<AiToolExecutor>>>);` managed app-wide (moved out of `AiChatState`), with `pub fn get_or_build(&self, owner: &OwnerHandle, dir: &Path) -> Result<Arc<AiToolExecutor>, String>` building with `FileAuditSink` at `dir/ai-audit.jsonl`; `ai_send` and the MCP layer both use it. `ai_reset` still replaces it with `None` (document: this also drops MCP-created proposals — acceptable; the executor is rebuilt on next use by either channel).

**Steps (TDD):**
- [ ] **RED:** adapt the executor tests: audit records must carry the channel passed per call — one test executes the same tool once as Assistant and once as Mcp on ONE executor and asserts the two audit lines differ only in channel; rate-limit test proves ONE budget across channels (propose as Assistant, immediate propose as Mcp → RateLimited); proposal ids monotonic across channels.
- [ ] **GREEN:** refactor; update all call sites; full workspace green.
- [ ] Gates + commit:

```bash
git commit -m "refactor(ai): share one tool executor across audit channels"
```

---

### Task 3: MCP handler (registry → rmcp tools, call_tool → executor)

**Files:**
- Create: `src-tauri/src/ai_mcp.rs`
- Modify: `src-tauri/Cargo.toml` (add `rmcp`)
- Modify: `src-tauri/src/lib.rs` (module)

**Interfaces produced:**
- `pub struct OpenTuneMcp { executor_state: Arc<AiExecutorState>, owner: OwnerHandle, config_dir: PathBuf }` implementing rmcp's `ServerHandler`. IMPORTANT: the handler resolves the executor via `executor_state.get_or_build(&owner, &config_dir)` PER CALL — never pins an `Arc<AiToolExecutor>` at construction. Rationale: `ai_reset` replaces the shared executor with `None`; a pinned Arc would silently diverge (MCP keeping old proposals/limiter while the assistant gets a fresh one). A test in this task pins the convergence: after simulating a reset (state set to None), the next `call_tool` uses a NEW executor (assert old proposals gone).
  - server info: name "opentune", instructions naming the advisory contract (reuse/adapt `ai_chat::system_prompt`'s rules text — the MCP client's model needs the same never-apply framing).
  - `list_tools`: `available_tools(&PermissionPolicy::advisory())` mapped to rmcp `Tool { name, description, input_schema }` (schemas are already JSON objects — pass through).
  - `call_tool`: `executor.execute_as(AuditChannel::Mcp, name, arguments)` → success JSON as the tool result content; `ToolError` → an MCP tool-call error result (map kinds: Denied/InvalidInput/Failed into the error message; do NOT convert into protocol-level errors — tool errors are data for the model).
- Exact rmcp trait/type names VERIFIED against docs.rs 2.2 before implementing (binding constraint above) — the sketch here is intent, not gospel.

**Steps (TDD):**
- [ ] **RED:** tests (direct handler calls, no HTTP): `list_tools` returns exactly the advisory tool names (no apply_change/burn_now); `call_tool("read_tune", {names:["reqFuel"]})` against a simulator-backed shared executor returns values content; `call_tool("apply_change", …)` returns a tool-level error (not a panic/protocol error) and the audit line records channel=mcp denied.
- [ ] **GREEN:** add rmcp (pin the minor; note the resolved exact version + any API drift in the report); implement.
- [ ] Gates + commit:

```bash
git commit -m "feat(ai): add MCP handler over the shared tool engine"
```

---

### Task 4: HTTP server lifecycle (axum + bearer middleware + settings toggle)

**Files:**
- Create: `src-tauri/src/ai_mcp_server.rs`
- Modify: `src-tauri/src/ai_commands.rs` or `ai_chat_commands.rs` (apply-on-save hook) and `src-tauri/src/lib.rs` (start-on-boot when enabled; manage `McpServerState`)

**Interfaces produced:**
- `pub struct McpServerState { … }` (managed): current server task handle + shutdown signal (`tokio::sync::oneshot` or `CancellationToken`-less: oneshot + abort — keep std/tokio only).
- `pub async fn start_mcp_server(state, executor_state: Arc<AiExecutorState>, owner: OwnerHandle, config_dir: PathBuf, token, port) -> Result<(), String>` — constructs `OpenTuneMcp` with the STATE handle (per-call executor resolution, see Task 3); binds `127.0.0.1:port` (bind failure → user-readable error, e.g. port in use), axum router: middleware validating `Authorization: Bearer <token>` via constant-time compare (dependency-free xor-fold over bytes + length check; unit-tested) → rmcp `StreamableHttpService` (session manager per rmcp docs; host/origin validation ON — verify the config surface at implementation and set allowed hosts to localhost forms).
- `pub async fn stop_mcp_server(state)` — graceful-enough: signal + abort join handle; idempotent.
- Wiring: on `set_ai_settings` success, reconcile desired state (enabled+port) vs running state (start/stop/restart on port change); on app setup, start if `mcp_enabled`. Failures surface as the command's `Err` (frontend alert shows it).
- `pub fn mcp_status(state) -> McpStatusDto { running: bool, port: u16 }` command + DTO (+ bindings regen if added here — fold into Task 1's binding note if simpler; either way NEVER hand-edit bindings).

**Steps (TDD):**
- [ ] **RED:** integration tests (tokio, real loopback HTTP via the existing shared `reqwest` client): start on an OS-assigned free port (bind port 0 and read back the local addr — add that capability to `start_mcp_server` for tests), then: (1) request WITHOUT token → 401; (2) wrong token → 401; (3) correct token + MCP `initialize`/`tools/list` handshake → 200 with the advisory tool list (drive the minimal JSON-RPC frames per the streamable-HTTP spec — or, if rmcp's client feature makes this cleaner, justify enabling it for dev-dependencies only); (4) stop → connection refused. Constant-time-compare unit test (equal/unequal/length-mismatch).
- [ ] **GREEN:** implement; verify rmcp service integration compiles against the real API.
- [ ] Gates + commit:

```bash
git commit -m "feat(ai): serve MCP over localhost with bearer auth"
```

---

### Task 5: Settings UI — MCP section

**Files:**
- Modify: `src/components/ai/AiSettingsPanel.tsx` (+ its test), `src/components/ai/ai.css`
- Modify: `src/i18n/en.ts`, `src/i18n/pl.ts`

**Behavior (binding):**
- New sub-section in the existing AI settings panel: MCP toggle (`mcpEnabled`), port number input (min 1024), status line (from `mcp_status`: running on port / stopped), token display — MASKED by default (`••••`) with a Show toggle and a Copy button (`navigator.clipboard.writeText`; jsdom test mocks it), Regenerate button (calls `mcpTokenInfo(true)`, warns via the status line that clients must be updated), and a one-line hint with the Claude Code command: `claude mcp add --transport http opentune http://127.0.0.1:<port>/mcp --header "Authorization: Bearer <token>"` rendered with the REAL port (token stays masked in the hint; the copy button carries the real value).
- Save goes through the existing settings save (extended DTO); errors via the panel's existing alert.
- i18n keys (en+pl, exact): `ai.mcp.title` ("MCP server"/"Serwer MCP"), `ai.mcp.enable` ("Expose tools over MCP (local)"/"Udostępnij narzędzia przez MCP (lokalnie)"), `ai.mcp.port` ("Port"/"Port"), `ai.mcp.running` ("Running on port {port}"/"Działa na porcie {port}") — follow the house pattern for interpolation (check how existing strings do it; if none interpolate, use two keys around the number), `ai.mcp.stopped` ("Stopped"/"Zatrzymany"), `ai.mcp.token` ("Access token"/"Token dostępu"), `ai.mcp.show` ("Show"/"Pokaż"), `ai.mcp.copy` ("Copy"/"Kopiuj"), `ai.mcp.copied` ("Copied"/"Skopiowano"), `ai.mcp.regenerate` ("Regenerate"/"Wygeneruj nowy"), `ai.mcp.regenerated` ("New token — update your MCP clients"/"Nowy token — zaktualizuj klientów MCP"), `ai.mcp.hint` ("Connect Claude Code:"/"Podłącz Claude Code:").
- Tests: toggle+port save through extended DTO; token fetched only on Show/Copy (not on mount — the token should not sit in component state unmasked by default); Copy calls clipboard with the fetched token; Regenerate refetches and shows the warning.

**Steps:** RED → GREEN → full frontend gates + Rust sanity → commit:

```bash
git commit -m "feat(ai): add MCP section to the AI settings panel"
```

---

### Task 6: Docs

**Files:**
- Create: `docs/mcp.md` — connecting external agents: Claude Code (`claude mcp add --transport http …`), Claude Desktop (stdio-only local config → `npx mcp-remote http://127.0.0.1:<port>/mcp` bridge, Node requirement stated), the advisory contract (read + propose only; the user applies in-app), the token model (per-install, regenerate invalidates), security notes (loopback-only, bearer required, origin validation).
- Modify: `docs/ARCHITECTURE.md` §5.10 (MCP server exists as of slice 4 — factually precise: shared executor, advisory-only, loopback+bearer); `docs/index.md` or README nav if the docs site lists pages (check how m6 docs pages are linked and follow).

**Steps:** write → both-stack gates → commit:

```bash
git commit -m "docs(mcp): document external agent access"
```

---

## Final: whole-branch review

Dispatch the final reviewer (most capable model) over `main..HEAD` with this plan + the minors ledger. Review lenses: (1) network exposure — loopback bind proven, auth on every route (including the SSE/streaming endpoints), constant-time compare, no token in logs/JSON/bindings beyond `mcp_token_info`'s documented return; (2) the shared-executor refactor — no channel mislabeling, one limiter budget, chat regression-free; (3) rmcp integration drift vs this plan (the report must list it); (4) lifecycle races (toggle spam, port change while running, app-exit shutdown). Fix wave; gates; push `-u origin m7-mcp-server`; PR titled `feat(m7): expose OpenTune as an MCP server`, body mapping deliverables to commits, noting the Claude-Code/Desktop connection instructions and that a live external-client smoke (Claude Code against the running app) is the user's manual step.
