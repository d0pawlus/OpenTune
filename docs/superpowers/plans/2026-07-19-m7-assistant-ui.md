# M7 Slice 3 — Embedded Assistant (Chat Loop, Streaming Panel, Proposal Review) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the embedded assistant end-to-end: a backend chat loop that connects the slice-2 providers to the slice-1 tool executor (streaming, tool round-trips, audit, guardrails), and a minimal chat panel with live token streaming, a tool-call transcript, and proposal-review cards whose Apply goes through the existing `setCells` path — the M7 `advisory` demo.

**Architecture:** The chat loop lives app-side (`src-tauri/src/ai_chat.rs`): pure, dependency-injected `run_chat_turn(provider, executor, history, …, emit)` testable entirely with `FakeProvider` + the simulator-backed owner — no network in any test. Commands (`ai_send`/`ai_cancel`/`ai_reset`/`ai_proposals`) manage an `AiChatState` (history + running flag + lazily-built `AiToolExecutor` with the `FileAuditSink` finally wired); streaming reaches the frontend via one tagged tauri-specta event (`AiStreamEvent`). The panel is another stacked section (M8 owns UX polish); token deltas are buffered and flushed on an interval so streaming never contends with the rAF gauge loop. This slice also lands the #29 items scheduled for it: shared reqwest client, HTTP error truncation, OpenAI `is_error` prefixing, Anthropic unknown-event tolerance, and the `read_realtime` staleness annotation (issue #27).

**Tech Stack:** existing only — no new dependencies (tokio, reqwest, serde already in; frontend uses existing test infra).

## Global Constraints

These bind every task and every reviewer:

- Work on branch `m7-assistant-ui` off `main`, in worktree `.worktrees/m7-assistant-ui`.
- TDD mandatory: failing test first (RED), then the fix (GREEN).
- Commit format: conventional commits; NO attribution footers of any kind.
- Rust gates before every commit (from `src-tauri/`, prefix `. "$HOME/.cargo/env" && `): `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Frontend gates when frontend files change (repo root): `npm run lint`, `npm run format:check`, `npx tsc --noEmit -p tsconfig.json`, `npm test`.
- NO new dependencies, either stack.
- New files carry `// SPDX-License-Identifier: GPL-3.0-or-later`.
- Secrets discipline unchanged: the API key is read from the `KeyStore` only inside `ai_send`'s provider construction and handed to the provider struct; never logged, echoed, or serialized.
- Safety invariants (ADR-0008, review lens): the assistant NEVER writes to the ECU — the only apply path is the user clicking Apply, which calls the existing `useTuneStore.setCells` (same as AutoTune). `run_chat_turn` must have no code path that calls `set_value`/`set_cells`/`burn` commands. Every tool call goes through `AiToolExecutor::execute` (policy + audit).
- Task 4 adds IPC commands + one event: register in `collect_commands![]` / `collect_events![]` (`src-tauri/src/lib.rs`), regenerate `src/ipc/bindings.ts` via `cargo test binding_gen`, commit the regenerated file. Never hand-edit bindings.
- All user-facing strings through i18n, BOTH `en` and `pl` (parity type-enforced).
- Numeric constants (iteration cap, staleness threshold, throttle interval, truncation length) are named constants with doc comments — no magic numbers.
- Scope discipline: no drive-by refactors beyond the #29 items explicitly listed in Task 3.
- File cap 800 lines — `ai_chat.rs` includes a large test module; if it approaches the cap, move tests to `src-tauri/src/ai_chat_tests.rs` wired with `#[cfg(test)] #[path = …]` (the owner-tests pattern).

Existing interfaces this slice consumes (verbatim, do not modify except where Task 3 says):
- `AiToolExecutor::new(owner: OwnerHandle, policy: PermissionPolicy, limits: GuardrailLimits, channel: AuditChannel, audit: Box<dyn AuditSink>)`, `execute(&self, name: &str, input: serde_json::Value) -> Result<serde_json::Value, ToolError>`, `proposals() -> Vec<ProposalDto>`, `ToolError { kind: ToolErrorKind, message }`, `ProposalDto { id, constant, reason, ok, cells: Vec<CellVerdict> }`, `FileAuditSink::new(path)` (`src-tauri/src/ai_tools.rs`).
- `Provider::chat(&self, req: &ChatRequest, on_delta: OnDelta<'_>) -> Result<ChatTurn, ProviderError>`; `ChatRequest { system, messages, tools, model, max_tokens }`; `ChatMessage::{User{text}, Assistant{blocks}, ToolResults{results}}`; `AssistantBlock::{Text{text}, ToolUse{id,name,input}}`; `ToolResultMsg { tool_use_id, content, is_error }`; `ToolDef` + `From<opentune_ai::ToolSpec>`; `StopReason`; `FakeProvider::new(turns)` (`src-tauri/src/ai_provider.rs`).
- `available_tools(&PermissionPolicy)` (crate `opentune-ai`); `load_ai_settings_in`, `KeyStore::get_key`, `AiKeyStoreState` (`ai_settings.rs`/`ai_commands.rs`).
- Owner test seam: `spawn_owner_with_emitter`, `ConnectSource::Simulator { ini_path: None }`, `Command::LoadTune`, constant `reqFuel` (see `src-tauri/src/owner_tests.rs:13-38`).

---

### Task 1: Chat core — history, system prompt, staleness annotation

**Files:**
- Create: `src-tauri/src/ai_chat.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod ai_chat;`)

**Interfaces:**
- Consumes: `ChatMessage`, `AssistantBlock`, `ToolResultMsg` from `ai_provider.rs`; `ToolSpec` from `opentune_ai`.
- Produces (Task 2/4 depend on these):
  - `pub const MAX_TOOL_ITERATIONS: usize = 8;` (runaway tool-loop cap)
  - `pub const MAX_HISTORY_MESSAGES: usize = 40;` (context cap)
  - `pub const REALTIME_STALE_MS: u64 = 2000;` (issue #27 staleness cutoff)
  - `pub struct ChatHistory { … }` with `new()`, `push(&mut self, msg: ChatMessage)` (evicts oldest beyond cap), `messages(&self) -> &[ChatMessage]`, `clear(&mut self)`, `len(&self) -> usize`, `is_empty(&self) -> bool`
  - `pub fn system_prompt(tools: &[opentune_ai::ToolSpec]) -> String`
  - `pub fn annotate_realtime_staleness(result: &mut serde_json::Value)` — inserts `"stale": true` when `ageMs > REALTIME_STALE_MS`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod core_tests {
    use super::*;
    use crate::ai_provider::ChatMessage;

    #[test]
    fn history_evicts_oldest_beyond_cap() {
        let mut h = ChatHistory::new();
        for i in 0..(MAX_HISTORY_MESSAGES + 5) {
            h.push(ChatMessage::User { text: format!("m{i}") });
        }
        assert_eq!(h.len(), MAX_HISTORY_MESSAGES);
        match &h.messages()[0] {
            ChatMessage::User { text } => assert_eq!(text, "m5"),
            other => panic!("unexpected head: {other:?}"),
        }
        h.clear();
        assert!(h.is_empty());
    }

    #[test]
    fn system_prompt_names_tools_and_advisory_rules() {
        let policy = opentune_ai::PermissionPolicy::advisory();
        let tools = opentune_ai::available_tools(&policy);
        let prompt = system_prompt(&tools);
        assert!(prompt.contains("read_tune"));
        assert!(prompt.contains("propose_change"));
        assert!(!prompt.contains("apply_change"), "locked tools are not advertised");
        assert!(prompt.to_lowercase().contains("never appl"), "advisory rule stated");
        assert!(prompt.contains("ageMs"), "staleness rule stated");
    }

    #[test]
    fn realtime_staleness_annotation() {
        let mut fresh = serde_json::json!({ "channels": [], "ageMs": 100 });
        annotate_realtime_staleness(&mut fresh);
        assert!(fresh.get("stale").is_none());
        let mut stale = serde_json::json!({ "channels": [], "ageMs": 5000 });
        annotate_realtime_staleness(&mut stale);
        assert_eq!(stale["stale"], true);
        let mut none = serde_json::json!(null); // snapshot absent
        annotate_realtime_staleness(&mut none);
        assert!(none.get("stale").is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo test ai_chat`
Expected: FAIL to compile (add `pub mod ai_chat;` to lib.rs first).

- [ ] **Step 3: Implement**

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
//! The embedded assistant's chat core (M7 slice 3): bounded history, the
//! system prompt, and the chat loop connecting providers (slice 2) to the
//! tool executor (slice 1). The assistant NEVER writes to the ECU: the only
//! apply path is the user clicking Apply in the UI, which uses the same
//! `set_cells` path as AutoTune.

use crate::ai_provider::ChatMessage;

/// Hard cap on tool round-trips within one user turn (runaway-loop guard).
pub const MAX_TOOL_ITERATIONS: usize = 8;
/// Hard cap on retained conversation messages (context guard).
/// ponytail: naive head-eviction — token-aware trimming if models hit limits.
pub const MAX_HISTORY_MESSAGES: usize = 40;
/// A realtime snapshot older than this is annotated stale (issue #27).
pub const REALTIME_STALE_MS: u64 = 2000;

/// Bounded conversation history. Eviction drops the oldest message; the
/// system prompt is NOT stored here (rebuilt per request).
#[derive(Debug, Default)]
pub struct ChatHistory {
    messages: Vec<ChatMessage>,
}

impl ChatHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
        while self.messages.len() > MAX_HISTORY_MESSAGES {
            self.messages.remove(0);
        }
    }

    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

/// The assistant's operating contract. Lists the available tools by name
/// and states the advisory-level rules in plain language. Numbers come
/// from tools, never from the model (ADR-0008 thin-AI rule).
pub fn system_prompt(tools: &[opentune_ai::ToolSpec]) -> String {
    let tool_lines: String = tools
        .iter()
        .map(|t| format!("- {}: {}\n", t.name, t.description))
        .collect();
    format!(
        "You are OpenTune's tuning assistant, running at the advisory \
         authority level inside an engine-tuning application.\n\
         \n\
         Available tools:\n{tool_lines}\
         \n\
         Rules you must follow:\n\
         - All numeric analysis comes from tools. Never invent or compute \
           tuning numbers yourself.\n\
         - You can NEVER apply changes to the ECU. To suggest a change, call \
           propose_change; the user reviews and applies it manually. Never \
           claim a change was applied.\n\
         - read_realtime results include ageMs (snapshot age in \
           milliseconds). If a result is marked stale or ageMs is large, \
           say so and do not treat it as current.\n\
         - If a tool returns an error or a proposal comes back not-ok, \
           report the reason honestly.\n\
         - Be concise; the user may be tuning a running engine."
    )
}

/// Issue #27: annotate a read_realtime tool result whose snapshot is older
/// than [`REALTIME_STALE_MS`] so the model cannot mistake it for current
/// data. `null` results (no frame yet) are left untouched.
pub fn annotate_realtime_staleness(result: &mut serde_json::Value) {
    let Some(age) = result.get("ageMs").and_then(|v| v.as_u64()) else {
        return;
    };
    if age > REALTIME_STALE_MS {
        result["stale"] = serde_json::Value::Bool(true);
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo test ai_chat`
Expected: PASS (3 tests).

- [ ] **Step 5: Gates + commit**

```bash
git add src-tauri/src/ai_chat.rs src-tauri/src/lib.rs
git commit -m "feat(ai): add chat history, system prompt, and staleness annotation"
```

---

### Task 2: The chat loop

**Files:**
- Modify: `src-tauri/src/ai_chat.rs` (add the loop + `AiStreamEmit` abstraction)
- Create: `src-tauri/src/ai_chat_tests.rs` (integration tests; wire with `#[cfg(test)] #[path = "ai_chat_tests.rs"] mod loop_tests;` inside `ai_chat.rs`)

**Interfaces:**
- Consumes: Task 1's items; `Provider`, `ChatRequest`, `ChatTurn`, `StopReason`, `AssistantBlock`, `ToolResultMsg`, `ToolDef`, `FakeProvider`; `AiToolExecutor`, `ToolError`, `ToolErrorKind`.
- Produces (Task 4 wires these to IPC/events):
  - `pub enum ChatEvent { Delta { text: String }, ToolStart { name: String }, ToolEnd { name: String, ok: bool, summary: String }, ProposalReady { id: u32 }, Done, Cancelled, Error { message: String } }` (plain Rust — the specta event DTO comes in Task 4)
  - `pub async fn run_chat_turn(provider: &Provider, executor: &AiToolExecutor, history: &mut ChatHistory, tools: &[ToolDef], system: &str, model: &str, max_tokens: u32, user_text: String, cancel: &std::sync::atomic::AtomicBool, emit: &(dyn Fn(ChatEvent) + Send + Sync)) -> Result<(), String>`

**Behavior contract (the tests below pin it):**
1. Push `User{user_text}`; then loop (bounded by `MAX_TOOL_ITERATIONS`):
2. Cancel check (`cancel.load(SeqCst)`) before each provider call and before each tool execution → emit `Cancelled`, return Ok. (`// ponytail: flag checks between steps — mid-stream cancel needs select!; add if users ask`.)
3. `provider.chat` with an `on_delta` that forwards every delta as `ChatEvent::Delta`.
4. Provider error → emit `Error { message }` (ProviderError's Display), return Err.
5. Push the returned turn's blocks as `Assistant{blocks}`.
6. `StopReason::EndTurn` → emit `Done`, return Ok. `MaxTokens`/`Other` → emit `Error` naming the stop reason, return Ok (turn is preserved). `ToolUse` → execute every `ToolUse` block IN ORDER via `executor.execute(name, input)`:
   - emit `ToolStart{name}` before, `ToolEnd{name, ok, summary}` after (summary: for ok = compact one-liner like `"ok"` or for propose_change the proposal id/ok; for err = the ToolError message).
   - `read_realtime` ok-results pass through `annotate_realtime_staleness` BEFORE serialization into the tool result.
   - `propose_change` ok-results additionally emit `ProposalReady { id }` (parse `id` from the result JSON).
   - ok → `ToolResultMsg { tool_use_id: block id, content: result JSON as string, is_error: false }`; Err → `{ content: error message, is_error: true }` — errors go BACK to the model, they do not abort the loop.
7. Push `ToolResults{results}` (one message with all results, preserving order) and continue the loop.
8. Iteration cap exceeded → emit `Error` mentioning the cap, return Ok.

- [ ] **Step 1: Write the failing integration tests**

`src-tauri/src/ai_chat_tests.rs` — arrange helpers mirror `owner_ai_tests.rs`: build a simulator-backed owner (`spawn_owner_with_emitter` + `Connect{Simulator{ini_path:None}}` + `LoadTune`), an `AiToolExecutor` with a Vec-collecting `AuditSink` (copy the `VecSink` test helper pattern from `ai_tools.rs` tests), `PermissionPolicy::advisory()`, default limits, `AuditChannel::Assistant`. Events collected via `let events: Arc<Mutex<Vec<ChatEvent>>>` closure. Tests:

```rust
#[tokio::test]
async fn full_turn_with_tool_round_trip() {
    // FakeProvider script: turn 1 = Text("checking") + ToolUse read_tune
    // {names:["reqFuel"]}; turn 2 = Text("done") EndTurn.
    // Assert: events contain >=1 Delta, ToolStart/ToolEnd(ok) for read_tune,
    // Done last; history is [User, Assistant, ToolResults, Assistant];
    // the ToolResults content parses as JSON with a "values" array;
    // audit sink recorded exactly 1 line (the read_tune execution).
}

#[tokio::test]
async fn tool_error_is_returned_to_the_model_not_fatal() {
    // Script: turn 1 = ToolUse read_tune {"bogus": true} (invalid input);
    // turn 2 = Text EndTurn. Assert: ToolEnd has ok=false; the ToolResults
    // message carries is_error=true; loop continued to Done.
}

#[tokio::test]
async fn runaway_tool_loop_stops_at_cap() {
    // Script: MAX_TOOL_ITERATIONS + 1 identical ToolUse turns.
    // Assert: exactly MAX_TOOL_ITERATIONS ToolStart events, then an
    // Error event mentioning the cap; return is Ok.
}

#[tokio::test]
async fn cancel_flag_stops_before_next_provider_call() {
    // Script: turn 1 = ToolUse read_tune. Set cancel=true from inside the
    // ToolEnd emit closure (flag flips mid-turn). Assert: Cancelled emitted,
    // no second provider call (script still has 1 unconsumed turn — assert
    // via FakeProvider turns remaining), return Ok.
}

#[tokio::test]
async fn propose_change_emits_proposal_ready() {
    // Script: turn 1 = ToolUse propose_change {constant:"reqFuel",
    // edits:[{index:0,value:13.0}], reason:"test"}; turn 2 = Text EndTurn.
    // Assert: ProposalReady{id:1} emitted; executor.proposals() has 1 entry.
}
```

Write these as REAL tests (full arrange/act/assert code following the comments — the comments above are the specification; the implementer writes the complete bodies, reusing the exact connect helpers from `owner_ai_tests.rs` and `ai_tools.rs` test modules).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo test ai_chat`
Expected: FAIL to compile — `run_chat_turn`/`ChatEvent` not defined.

- [ ] **Step 3: Implement `run_chat_turn` + `ChatEvent`** per the behavior contract above. Keep the function under 100 lines by extracting `execute_tool_block(executor, block, emit) -> ToolResultMsg` as a helper.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo test ai_chat`
Expected: PASS (3 core + 5 loop tests).

- [ ] **Step 5: Gates + commit**

```bash
git add src-tauri/src/ai_chat.rs src-tauri/src/ai_chat_tests.rs
git commit -m "feat(ai): add assistant chat loop with tool round-trips"
```

---

### Task 3: Provider polish (issue #29 items scheduled for this slice)

**Files:**
- Modify: `src-tauri/src/ai_provider.rs` (shared client + truncation helper)
- Modify: `src-tauri/src/ai_anthropic.rs`, `src-tauri/src/ai_openai.rs`
- Modify: `src-tauri/src/ai_tools.rs` (ONLY the stale comment at line ~28: FileAuditSink wiring is slice-3 (`ai_send`), not "slice 2 setup")

**Interfaces:**
- Produces: `pub(crate) fn http_client() -> &'static reqwest::Client` (std `OnceLock`) in `ai_provider.rs`, used by both providers instead of per-call `Client::new()`; `pub(crate) fn truncate_message(s: String) -> String` capping at `MAX_HTTP_ERROR_LEN: usize = 2000` chars (named constant, char-boundary-safe truncation with an appended `"…"`).

Changes, each TDD:
1. **Shared client:** both `chat` fns use `http_client()`. No behavior change — existing tests stay green (no new test needed beyond compilation; note in report).
2. **HTTP error truncation:** both non-success branches wrap the body in `truncate_message`. Test (in `ai_provider.rs`): 3000-char input → 2000 chars + `…`, multi-byte boundary safe (include "ż" around the cut); short input unchanged.
3. **OpenAI `is_error` semantics:** in `build_request_body`, a `ToolResultMsg` with `is_error: true` gets its content prefixed `"Error: "` (Anthropic carries the flag natively — no change there). Test: builder with an errored result asserts the `role:"tool"` message content starts with `"Error: "`; non-errored content unprefixed.
4. **Anthropic unknown-event tolerance:** `SseAssembler::feed` must NOT fail on an unknown event with non-JSON data — restructure so JSON parsing happens only for known events (`message_start` and `ping` need no parse either). Test: `feed("some_future_event", "not json", …)` → `Ok(())`; known-event non-JSON still errors.

- [ ] **Step 1: RED** — write the four test additions, watch the new ones fail.
- [ ] **Step 2: GREEN** — implement; run `cargo test ai_provider && cargo test ai_anthropic && cargo test ai_openai`.
- [ ] **Step 3: Gates + commit**

```bash
git add src-tauri/src/ai_provider.rs src-tauri/src/ai_anthropic.rs src-tauri/src/ai_openai.rs src-tauri/src/ai_tools.rs
git commit -m "fix(ai): provider polish from slice-2 follow-ups"
```

---

### Task 4: IPC commands + stream event + bindings

**Files:**
- Create: `src-tauri/src/ai_chat_commands.rs`
- Modify: `src-tauri/src/events.rs` (add `AiStreamEvent`)
- Modify: `src-tauri/src/dto.rs` (add `AiProposalDto` + `AiCellVerdictDto` mirroring `ai_tools::ProposalDto`/`opentune_ai::CellVerdict`)
- Modify: `src-tauri/src/lib.rs` (module; 4 commands in `collect_commands![]`; `AiStreamEvent` in `collect_events![]`; `app.manage(AiChatState::default())` in setup)
- Modify (generated): `src/ipc/bindings.ts` via `cargo test binding_gen`

**Interfaces:**
- Consumes: everything from Tasks 1–2; `AiKeyStoreState`, `load_ai_settings_in`, `OwnerHandle`, `FileAuditSink`, `AuditChannel::Assistant`, `available_tools`, `ToolDef::from`.
- Produces:
  - Event (events.rs, follows house derive set): `#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type, Event)] #[serde(tag = "kind", rename_all = "camelCase")] pub enum AiStreamEvent { Delta { text: String }, ToolStart { name: String }, ToolEnd { name: String, ok: bool, summary: String }, ProposalReady { id: u32 }, Done, Cancelled, Error { message: String } }` with `impl From<crate::ai_chat::ChatEvent> for AiStreamEvent` (mechanical 1:1).
  - DTOs (dto.rs): `AiCellVerdictDto { index: u32, value: f64, ok: bool, note: Option<String> }` (note = human-readable rendering of non-Ok `CellCheck` variants), `AiProposalDto { id, constant, reason, ok, cells: Vec<AiCellVerdictDto>, edits: Vec<CellEditDto> }` — `edits` pre-maps ok proposals to the exact `{index, value}` list the frontend passes to `setCells`; `From<ai_tools::ProposalDto>`.
  - `pub struct AiChatState { … }` (Default): `Mutex`-guarded history + `Arc<AtomicBool>` cancel + running flag + lazily-created `Arc<AiToolExecutor>`.
  - Commands: `ai_send(text: String, app: tauri::AppHandle, owner: State<OwnerHandle>, keys: State<AiKeyStoreState>, chat: State<AiChatState>) -> Result<(), String>` — validates (AI enabled; provider key present; non-empty text; not already running), builds the provider (Anthropic/OpenAi from settings + key), builds/reuses the executor (FileAuditSink at `app_config_dir/ai-audit.jsonl`), spawns a tokio task running `run_chat_turn` with `emit` = tauri event emission (`AiStreamEvent::from(ev).emit(&app)`); clears `running` when the task ends (RAII guard or explicit on both paths). `ai_cancel(chat)` sets the flag. `ai_reset(chat) -> Result<(), String>` errors while running, else clears history + proposals? (proposals live in the executor — reset REPLACES the executor so old proposals drop; document). `ai_proposals(chat) -> Result<Vec<AiProposalDto>, String>`.
  - Extracted testable core: `pub(crate) fn validate_send(settings: &AiSettings, key_present: bool, text: &str, running: bool) -> Result<(), String>` with exact error strings (i18n happens frontend-side via generic error display; backend strings are English diagnostics).

- [ ] **Step 1: RED** — tests for `validate_send` (disabled → err mentioning enable; missing key → err; empty/whitespace text → err; running → err; happy → ok) and for the `From<ProposalDto>` mapping: a non-Ok cell maps to `ok: false` with a human-readable `note`; `edits` contains every cell's `{index, value}` when the proposal as a whole is ok, and is EMPTY when the proposal is not ok (the frontend must have nothing applicable to pass to `setCells`) — assert both cases.
- [ ] **Step 2: GREEN** — implement; wire lib.rs; `cargo test binding_gen` regenerates bindings (expect `aiSend`, `aiCancel`, `aiReset`, `aiProposals`, `AiStreamEvent`, `AiProposalDto` in bindings.ts).
- [ ] **Step 3: Full Rust gates + `npx tsc --noEmit -p tsconfig.json` + commit**

```bash
git add src-tauri/src/ai_chat_commands.rs src-tauri/src/events.rs src-tauri/src/dto.rs src-tauri/src/lib.rs src/ipc/bindings.ts
git commit -m "feat(ai): add assistant chat commands and stream event"
```

---

### Task 5: Chat panel with streaming transcript

**Files:**
- Create: `src/components/ai/AiChatPanel.tsx`
- Create: `src/components/ai/AiChatPanel.test.tsx`
- Modify: `src/components/ai/ai.css` (chat classes, tokens only)
- Modify: `src/i18n/en.ts`, `src/i18n/pl.ts`
- Modify: `src/App.tsx` (render after `<AiSettingsPanel …>`)
- Modify: `src/App.a11y.test.tsx`, `src/App.m6.test.tsx` (mock the new panel, house pattern)

**Interfaces:**
- Consumes: `commands.aiSend/aiCancel/aiReset`, `events.aiStreamEvent` (generated); `t(key, locale)`; existing tokens.
- Produces: `AiChatPanel({ locale }: { locale: Locale })`.

**Behavior:**
- Transcript state: list of entries `{ role: "user" | "assistant", text }` plus tool chips `{ name, ok, summary }` rendered inline between assistant chunks; container `role="log"` `aria-live="polite"` `aria-busy` while streaming.
- **Delta throttling (binding):** deltas append to a `useRef` buffer; a `DELTA_FLUSH_MS = 100` interval flushes the buffer into React state only while streaming — never a setState per delta (the rAF gauge loop shares the main thread).
- Send: input + button (disabled while running or empty); on send push the user entry, call `aiSend`, error branch → `<p role="alert">`. Cancel button visible while running → `aiCancel`. Reset button (disabled while running) → `aiReset` + clear transcript.
- `Done`/`Cancelled`/`Error{message}` events end the running state (flush remaining buffer; Error also sets the alert).
- i18n keys (en + pl): `ai.chat.title` ("Assistant"/"Asystent"), `ai.chat.placeholder` ("Ask about your tune…"/"Zapytaj o strojenie…"), `ai.chat.send` ("Send"/"Wyślij"), `ai.chat.cancel` ("Cancel"/"Przerwij"), `ai.chat.reset` ("Reset"/"Wyczyść"), `ai.chat.running` ("Assistant is responding…"/"Asystent odpowiada…"), `ai.chat.toolOk` ("tool"/"narzędzie"), `ai.chat.toolFailed` ("tool failed"/"narzędzie zawiodło"), `ai.chat.you` ("You"/"Ty").

- [ ] **Step 1: RED** — tests (mock bindings module incl. `events.aiStreamEvent.listen` returning a controllable callback; house pattern): (1) send calls `aiSend` and renders the user entry; (2) firing Delta events buffers then renders after the flush interval (use fake timers); (3) ToolEnd renders a chip with the tool name; (4) Error event surfaces `role="alert"`; (5) Cancel visible only while running and calls `aiCancel`.
- [ ] **Step 2: GREEN** — implement; run `npm test -- AiChatPanel`.
- [ ] **Step 3: Full frontend gates + Rust `cargo test --workspace` + commit**

```bash
git add src/components/ai src/i18n/en.ts src/i18n/pl.ts src/App.tsx src/App.a11y.test.tsx src/App.m6.test.tsx
git commit -m "feat(ai): add assistant chat panel with streaming transcript"
```

---

### Task 6: Proposal review cards + manual apply

**Files:**
- Create: `src/components/ai/ProposalCard.tsx`
- Create: `src/components/ai/ProposalCard.test.tsx`
- Modify: `src/components/ai/AiChatPanel.tsx` (on `ProposalReady{id}` → `commands.aiProposals()` → render cards for unseen ids)
- Modify: `src/components/ai/ai.css`, `src/i18n/en.ts`, `src/i18n/pl.ts`

**Interfaces:**
- Consumes: `AiProposalDto` (generated), `useTuneStore` (`setCells(name, edits)` — the EXACT call `AutoTunePanel.apply` uses, see `src/components/autotune/AutoTunePanel.tsx` ~line 131 and `src/stores/tune.ts` `setCells`).
- Produces: `ProposalCard({ proposal, locale, onApplied }: { proposal: AiProposalDto; locale: Locale; onApplied: () => void })`.

**Behavior:**
- Card: constant name, reason (verbatim), per-cell list (index, value, ✓/note), Apply button — enabled ONLY when `proposal.ok` AND `edits.length > 0`; Apply calls `useTuneStore.getState().setCells(proposal.constant, proposal.edits)`, then success status + `onApplied`; store errors surface via `role="alert"` (setCells rethrows on command failure — catch it). Dismiss button hides the card.
- i18n keys (en + pl): `ai.proposal.title` ("Proposed change"/"Proponowana zmiana"), `ai.proposal.apply` ("Apply"/"Zastosuj"), `ai.proposal.dismiss` ("Dismiss"/"Odrzuć"), `ai.proposal.applied` ("Applied"/"Zastosowano"), `ai.proposal.invalid` ("Not applicable — failed validation"/"Nie do zastosowania — nie przeszła walidacji").

- [ ] **Step 1: RED** — tests: (1) ok proposal → Apply enabled; click calls `setCells("reqFuel", [{index:0,value:13}])` (mock the store) and shows applied status; (2) not-ok proposal → Apply disabled + invalid note visible; (3) setCells rejection surfaces alert; (4) Dismiss hides the card.
- [ ] **Step 2: GREEN**; `npm test -- ProposalCard && npm test -- AiChatPanel`.
- [ ] **Step 3: Full frontend gates + commit**

```bash
git add src/components/ai src/i18n/en.ts src/i18n/pl.ts
git commit -m "feat(ai): add proposal review cards with manual apply"
```

---

### Task 7: Docs + whole-slice gates

**Files:**
- Modify: `docs/ARCHITECTURE.md` (§5.10: embedded assistant exists as of slice 3 — chat loop, streaming panel, proposal review with manual apply; MCP server remains for slice 4)

- [ ] **Step 1:** Update the doc (accurate, style-matched; describe the loop as provider→executor round-trips at advisory, no AI write path).
- [ ] **Step 2:** Full gates, both stacks (same commands as prior slices).
- [ ] **Step 3: Commit**

```bash
git add docs/ARCHITECTURE.md
git commit -m "docs(architecture): record embedded assistant"
```

---

## Final: whole-branch review

Dispatch the final code reviewer (most capable model) over `main..HEAD` with this plan + the minors ledger. Review lenses: (1) the no-AI-write invariant — grep-level proof that `run_chat_turn`/commands never reach `set_value`/`set_cells`/`burn`; (2) event/state races (running flag, cancel flag, executor replacement on reset); (3) streaming throttle actually bounds re-renders; (4) key handling unchanged. Fix wave for Critical/Important; then gates, push `-u origin m7-assistant-ui`, PR titled `feat(m7): add embedded assistant (chat loop, streaming panel, proposal review)`, body mapping deliverables to commits, noting the live-provider round-trip remains manual-smoke (BYOK key required) and MCP = slice 4.
