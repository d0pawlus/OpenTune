# M7 Slice 2 — BYOK Providers (AiProvider, Anthropic, OpenAI, Key Storage, AI Settings) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the BYOK provider layer — an `AiProvider` abstraction with Anthropic and OpenAI implementations (streaming, tool use), OS-keyring key storage, persisted AI settings (off by default), and the opt-in AI settings UI section — so slice 3 can wire a chat loop to the slice-1 tool executor.

**Architecture:** Provider code lives app-side (`src-tauri/src/ai_provider.rs`, `ai_anthropic.rs`, `ai_openai.rs`, `ai_settings.rs`, `ai_commands.rs`) because it needs HTTP/keyring; the pure `opentune-ai` crate stays dependency-light and untouched except where noted. Dispatch is a `Provider` enum (Anthropic/OpenAi/Fake) — two known implementations don't justify `dyn` + `async_trait` (ponytail). Wire modules split pure, fixture-testable request-builders and SSE assemblers from the thin HTTP calls. Keys go through a `KeyStore` trait (OS keyring impl + in-memory test impl); settings are an atomic JSON file in `app_config_dir` mirroring `layout.rs`. Enabling AI is the explicit consent step (ADR-0008: off by default, BYOK). The webview CSP already blocks frontend HTTP — all provider calls are backend-only by construction.

**Tech Stack:** Rust: `reqwest` 0.12 (json, stream, rustls-tls), `futures-util` 0.3, `keyring` 4 (apple-native, windows-native, sync-secret-service). Frontend: existing React 19 + zustand-free panel pattern, hand-rolled i18n (en+pl).

## Global Constraints

These bind every task and every reviewer:

- Work on branch `m7-provider-byok` off `main`, in worktree `.worktrees/m7-provider-byok`.
- TDD mandatory: write the failing test first (RED), then the fix (GREEN).
- Commit format: `<type>(<scope>): <description>` (conventional commits). NO attribution footers of any kind — attribution is disabled for this repository.
- Rust gates before every commit (from `src-tauri/`, prefix `. "$HOME/.cargo/env" && `): `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Frontend gates when frontend files change (repo root): `npm run lint`, `npm run format:check`, `npx tsc --noEmit -p tsconfig.json`, `npm test`.
- New files carry `// SPDX-License-Identifier: GPL-3.0-or-later` as the first line.
- **Secrets discipline (hard):** the API key must NEVER be logged, formatted into an error message, serialized into settings JSON, returned by any command, or appear in any test fixture as a realistic-looking value (use `"test-key"`). `Debug` impls on types holding keys must redact.
- New dependencies allowed ONLY as listed: `reqwest = { version = "0.12", default-features = false, features = ["json", "stream", "rustls-tls"] }`, `futures-util = "0.3"`, `keyring = { version = "4", features = ["apple-native", "windows-native", "sync-secret-service"] }`. Nothing else.
- Task 2 adds IPC commands: register them in `build_specta()`'s `collect_commands![]` (`src-tauri/src/lib.rs`) and regenerate `src/ipc/bindings.ts` via `. "$HOME/.cargo/env" && cargo test binding_gen` — never hand-edit bindings.ts. Commit the regenerated file with the task.
- All user-facing strings through i18n with BOTH `en` and `pl` keys (parity is type-enforced; `src/i18n/pl.ts` is `Record<keyof typeof en, string>`).
- Do NOT touch OS keychains in tests: unit/integration tests use `MemoryKeyStore` only. `OsKeyStore` is a thin adapter verified by review, not by CI (macOS/Windows CI keychain access is prompt-gated; Linux CI has no Secret Service).
- AI defaults: `enabled = false`, `provider = "anthropic"`, `model = "claude-sonnet-5"` (OpenAI model default is empty — BYOK users enter theirs; validation requires non-empty model only when enabled).
- Wire shapes below are from July 2026 provider docs. Before implementing Tasks 4/5, the implementer verifies field names against current official docs (Context7 / platform.claude.com / platform.openai.com) and adapts if drifted — noting any drift in the report.
- Scope discipline: no drive-by refactors. `opentune-analysis` and slice-1 files unchanged except where a task names them.
- File cap 800 lines; split by responsibility if approaching it.

---

### Task 1: Key store + settings persistence (backend, no IPC)

**Files:**
- Create: `src-tauri/src/ai_settings.rs`
- Modify: `src-tauri/Cargo.toml` (add `keyring` under `[dependencies]`)
- Modify: `src-tauri/src/lib.rs` (add `pub mod ai_settings;` next to the other module declarations)

**Interfaces:**
- Consumes: `serde`, `serde_json`, `keyring::Entry`; the atomic-write pattern from `src-tauri/src/layout.rs:27` (`save_layout_in`: temp file + fsync + rename) and its test style.
- Produces (Task 2 depends on these exact signatures):
  - `pub trait KeyStore: Send + Sync { fn set_key(&self, provider: &str, key: &str) -> Result<(), String>; fn key_present(&self, provider: &str) -> Result<bool, String>; fn get_key(&self, provider: &str) -> Result<Option<String>, String>; fn clear_key(&self, provider: &str) -> Result<(), String>; }`
  - `pub struct OsKeyStore;` (keyring-backed) and `pub struct MemoryKeyStore` (`Default`, test/in-memory)
  - `pub const AI_PROVIDERS: [&str; 2] = ["anthropic", "openai"];` and `pub fn validate_provider(provider: &str) -> Result<(), String>`
  - `pub struct AiSettings { pub enabled: bool, pub provider: String, pub model: String }` (serde camelCase, `Default` as per Global Constraints)
  - `pub fn save_ai_settings_in(dir: &Path, settings: &AiSettings) -> Result<(), String>` and `pub fn load_ai_settings_in(dir: &Path) -> Result<AiSettings, String>` (missing file → defaults; corrupt file → `Err`)

- [ ] **Step 1: Add the keyring dependency**

In `src-tauri/Cargo.toml` `[dependencies]`:

```toml
keyring = { version = "4", features = ["apple-native", "windows-native", "sync-secret-service"] }
```

- [ ] **Step 2: Write the failing tests**

`src-tauri/src/ai_settings.rs` (skeleton with tests; implementation comes in Step 4):

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
//! BYOK key storage and AI settings persistence (M7 slice 2).
//!
//! Keys live in the OS credential store via the `keyring` crate — never in
//! the settings JSON, never in logs, never returned to the frontend.
//! Settings are an atomic JSON file in `app_config_dir`, mirroring
//! `layout.rs`. AI is OFF by default: enabling it is the user's explicit
//! consent for data to leave the machine (ADR-0008).

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("opentune-ai-settings-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn defaults_are_off_anthropic_sonnet() {
        let s = AiSettings::default();
        assert!(!s.enabled);
        assert_eq!(s.provider, "anthropic");
        assert_eq!(s.model, "claude-sonnet-5");
    }

    #[test]
    fn settings_round_trip_via_file() {
        let dir = tmp_dir("roundtrip");
        let s = AiSettings { enabled: true, provider: "openai".into(), model: "gpt-x".into() };
        save_ai_settings_in(&dir, &s).expect("save");
        let back = load_ai_settings_in(&dir).expect("load");
        assert_eq!(back.enabled, s.enabled);
        assert_eq!(back.provider, s.provider);
        assert_eq!(back.model, s.model);
    }

    #[test]
    fn missing_settings_file_loads_defaults() {
        let dir = tmp_dir("missing");
        let s = load_ai_settings_in(&dir).expect("defaults");
        assert!(!s.enabled);
    }

    #[test]
    fn corrupt_settings_file_errors() {
        let dir = tmp_dir("corrupt");
        std::fs::write(dir.join(AI_SETTINGS_FILE), b"{not json").expect("write");
        assert!(load_ai_settings_in(&dir).is_err());
    }

    #[test]
    fn memory_key_store_set_present_get_clear() {
        let ks = MemoryKeyStore::default();
        assert!(!ks.key_present("anthropic").unwrap());
        ks.set_key("anthropic", "test-key").unwrap();
        assert!(ks.key_present("anthropic").unwrap());
        assert_eq!(ks.get_key("anthropic").unwrap().as_deref(), Some("test-key"));
        assert!(!ks.key_present("openai").unwrap());
        ks.clear_key("anthropic").unwrap();
        assert!(!ks.key_present("anthropic").unwrap());
        assert_eq!(ks.get_key("anthropic").unwrap(), None);
    }

    #[test]
    fn provider_names_are_validated() {
        assert!(validate_provider("anthropic").is_ok());
        assert!(validate_provider("openai").is_ok());
        let err = validate_provider("acme").unwrap_err();
        assert!(!err.contains("test-key"), "errors never echo secrets");
        assert!(err.contains("acme"));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo test ai_settings`
Expected: FAIL to compile — types not defined. (Add `pub mod ai_settings;` to `src-tauri/src/lib.rs` first.)

- [ ] **Step 4: Implement**

Above the test module in `ai_settings.rs`:

```rust
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Settings file name inside `app_config_dir` (same directory layout.rs uses).
pub const AI_SETTINGS_FILE: &str = "ai-settings.json";

/// The two supported BYOK providers. Provider ids double as keyring account
/// names — validate at every boundary.
pub const AI_PROVIDERS: [&str; 2] = ["anthropic", "openai"];

pub fn validate_provider(provider: &str) -> Result<(), String> {
    if AI_PROVIDERS.contains(&provider) {
        Ok(())
    } else {
        Err(format!("unknown AI provider: {provider}"))
    }
}

/// Persisted AI configuration. The API key is deliberately NOT here — it
/// lives in the OS credential store only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
}

impl Default for AiSettings {
    fn default() -> Self {
        let model = ["claude", "sonnet", "5"].join("-");
        Self { enabled: false, provider: "anthropic".into(), model }
    }
}

/// Where API keys live. Trait so tests never touch OS keychains.
pub trait KeyStore: Send + Sync {
    fn set_key(&self, provider: &str, key: &str) -> Result<(), String>;
    fn key_present(&self, provider: &str) -> Result<bool, String>;
    fn get_key(&self, provider: &str) -> Result<Option<String>, String>;
    fn clear_key(&self, provider: &str) -> Result<(), String>;
}

/// OS credential store (macOS Keychain / Windows Credential Manager /
/// Linux Secret Service). Service = the app bundle id; account = provider.
/// On Linux without a Secret Service this errors — callers surface the
/// message and the app keeps working with AI disabled.
pub struct OsKeyStore;

const KEYRING_SERVICE: &str = "org.opentune.desktop";

impl OsKeyStore {
    fn entry(provider: &str) -> Result<keyring::Entry, String> {
        validate_provider(provider)?;
        keyring::Entry::new(KEYRING_SERVICE, provider)
            .map_err(|e| format!("credential store unavailable: {e}"))
    }
}

impl KeyStore for OsKeyStore {
    fn set_key(&self, provider: &str, key: &str) -> Result<(), String> {
        Self::entry(provider)?
            .set_password(key)
            .map_err(|e| format!("could not store the API key: {e}"))
    }

    fn key_present(&self, provider: &str) -> Result<bool, String> {
        Ok(self.get_key(provider)?.is_some())
    }

    fn get_key(&self, provider: &str) -> Result<Option<String>, String> {
        match Self::entry(provider)?.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("could not read the API key: {e}")),
        }
    }

    fn clear_key(&self, provider: &str) -> Result<(), String> {
        match Self::entry(provider)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("could not clear the API key: {e}")),
        }
    }
}

/// In-memory store for tests (and any future headless fallback).
#[derive(Default)]
pub struct MemoryKeyStore {
    keys: Mutex<HashMap<String, String>>,
}

impl KeyStore for MemoryKeyStore {
    fn set_key(&self, provider: &str, key: &str) -> Result<(), String> {
        validate_provider(provider)?;
        self.keys.lock().unwrap().insert(provider.to_owned(), key.to_owned());
        Ok(())
    }

    fn key_present(&self, provider: &str) -> Result<bool, String> {
        validate_provider(provider)?;
        Ok(self.keys.lock().unwrap().contains_key(provider))
    }

    fn get_key(&self, provider: &str) -> Result<Option<String>, String> {
        validate_provider(provider)?;
        Ok(self.keys.lock().unwrap().get(provider).cloned())
    }

    fn clear_key(&self, provider: &str) -> Result<(), String> {
        validate_provider(provider)?;
        self.keys.lock().unwrap().remove(provider);
        Ok(())
    }
}

/// Atomic write, mirroring `layout.rs`: temp file + fsync + rename.
pub fn save_ai_settings_in(dir: &Path, settings: &AiSettings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string());
    let json = json?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{AI_SETTINGS_FILE}.tmp"));
    let target = dir.join(AI_SETTINGS_FILE);
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &target).map_err(|e| e.to_string())
}

/// Missing file → defaults (fresh install); unreadable/corrupt → Err.
pub fn load_ai_settings_in(dir: &Path) -> Result<AiSettings, String> {
    let path = dir.join(AI_SETTINGS_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("corrupt AI settings: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AiSettings::default()),
        Err(e) => Err(e.to_string()),
    }
}
```

NOTE: if `layout.rs` already has a reusable atomic-write helper, call it instead of duplicating (DRY) — check before writing your own. IMPORTANT: the `Default` model string above must be exactly `"claude-sonnet-5"` (plain ASCII — retype it, don't copy a rendered document).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo test ai_settings`
Expected: PASS (7 tests).

- [ ] **Step 6: Gates + commit**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green.

```bash
git add src-tauri/src/ai_settings.rs src-tauri/src/lib.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(ai): add key store and AI settings persistence"
```

---

### Task 2: AI settings IPC commands + regenerated bindings

**Files:**
- Create: `src-tauri/src/ai_commands.rs`
- Modify: `src-tauri/src/lib.rs` (module declaration; `collect_commands![]` additions; `app.manage(...)` in `run()`'s setup)
- Modify: `src-tauri/src/dto.rs` (new `AiSettingsDto`)
- Modify (generated): `src/ipc/bindings.ts` via `cargo test binding_gen`

**Interfaces:**
- Consumes: Task 1's `KeyStore`, `OsKeyStore`, `MemoryKeyStore`, `AiSettings`, `save_ai_settings_in`/`load_ai_settings_in`, `validate_provider`; the `layout.rs` pattern for `app.path().app_config_dir()`.
- Produces (frontend Task 6 calls these; slice 3 reads settings/keys backend-side):
  - Managed state `pub struct AiKeyStoreState(pub std::sync::Arc<dyn KeyStore>);`
  - Commands (all `#[tauri::command] #[specta::specta]`, registered): `get_ai_settings(app) -> Result<AiSettingsDto, String>`, `set_ai_settings(app, settings: AiSettingsDto) -> Result<(), String>` (validates provider; when `enabled`, requires non-empty `model`), `set_ai_key(provider: String, key: String, store: State<AiKeyStoreState>) -> Result<(), String>` (rejects empty/whitespace key; trims), `clear_ai_key(provider: String, store) -> Result<(), String>`, `ai_key_present(provider: String, store) -> Result<bool, String>`
  - `AiSettingsDto { enabled: bool, provider: String, model: String }` (serde camelCase + `specta::Type`) with `From<AiSettings>`/`Into<AiSettings>`

- [ ] **Step 1: Write the failing tests**

Command bodies must be thin wrappers over testable inner functions (the `layout.rs` style). Tests in `ai_commands.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_settings::MemoryKeyStore;

    #[test]
    fn set_key_trims_and_rejects_empty() {
        let store = MemoryKeyStore::default();
        set_ai_key_in(&store, "anthropic", "  test-key  ").expect("trimmed key stored");
        assert_eq!(store.get_key("anthropic").unwrap().as_deref(), Some("test-key"));
        let err = set_ai_key_in(&store, "anthropic", "   ").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn set_key_rejects_unknown_provider() {
        let store = MemoryKeyStore::default();
        assert!(set_ai_key_in(&store, "acme", "test-key").is_err());
    }

    #[test]
    fn settings_validation_requires_model_when_enabled() {
        let dir = std::env::temp_dir().join(format!("opentune-ai-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bad = AiSettingsDto { enabled: true, provider: "openai".into(), model: "  ".into() };
        assert!(set_ai_settings_in(&dir, bad).is_err());
        let ok = AiSettingsDto { enabled: true, provider: "openai".into(), model: "gpt-x".into() };
        set_ai_settings_in(&dir, ok).expect("valid settings persist");
        let read = get_ai_settings_in(&dir).expect("read back");
        assert!(read.enabled);
        assert_eq!(read.model, "gpt-x");
    }

    #[test]
    fn disabled_settings_do_not_require_model() {
        let dir = std::env::temp_dir().join(format!("opentune-ai-cmd2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = AiSettingsDto { enabled: false, provider: "anthropic".into(), model: String::new() };
        set_ai_settings_in(&dir, s).expect("disabled settings persist without model");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo test ai_commands`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`ai_commands.rs` above the tests:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
//! IPC commands for AI settings and BYOK key management (M7 slice 2).
//! Keys flow one way: UI → keyring. `ai_key_present` returns a bool only —
//! no command ever returns key material.

use std::path::Path;
use std::sync::Arc;

use tauri::State;

use crate::ai_settings::{
    load_ai_settings_in, save_ai_settings_in, validate_provider, AiSettings, KeyStore,
};
use crate::dto::AiSettingsDto;

/// Managed handle to the key store (OsKeyStore in production).
pub struct AiKeyStoreState(pub Arc<dyn KeyStore>);

fn config_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    app.path().app_config_dir().map_err(|e| e.to_string())
}

pub(crate) fn set_ai_key_in(store: &dyn KeyStore, provider: &str, key: &str) -> Result<(), String> {
    validate_provider(provider)?;
    let key = key.trim();
    if key.is_empty() {
        return Err("API key must not be empty".into());
    }
    store.set_key(provider, key)
}

pub(crate) fn set_ai_settings_in(dir: &Path, settings: AiSettingsDto) -> Result<(), String> {
    validate_provider(&settings.provider)?;
    if settings.enabled && settings.model.trim().is_empty() {
        return Err("model must be set when AI is enabled".into());
    }
    save_ai_settings_in(dir, &settings.into())
}

pub(crate) fn get_ai_settings_in(dir: &Path) -> Result<AiSettingsDto, String> {
    load_ai_settings_in(dir).map(AiSettingsDto::from)
}

#[tauri::command]
#[specta::specta]
pub async fn get_ai_settings(app: tauri::AppHandle) -> Result<AiSettingsDto, String> {
    get_ai_settings_in(&config_dir(&app)?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_ai_settings(
    app: tauri::AppHandle,
    settings: AiSettingsDto,
) -> Result<(), String> {
    set_ai_settings_in(&config_dir(&app)?, settings)
}

#[tauri::command]
#[specta::specta]
pub async fn set_ai_key(
    provider: String,
    key: String,
    store: State<'_, AiKeyStoreState>,
) -> Result<(), String> {
    set_ai_key_in(store.0.as_ref(), &provider, &key)
}

#[tauri::command]
#[specta::specta]
pub async fn clear_ai_key(
    provider: String,
    store: State<'_, AiKeyStoreState>,
) -> Result<(), String> {
    validate_provider(&provider)?;
    store.0.clear_key(&provider)
}

#[tauri::command]
#[specta::specta]
pub async fn ai_key_present(
    provider: String,
    store: State<'_, AiKeyStoreState>,
) -> Result<bool, String> {
    validate_provider(&provider)?;
    store.0.key_present(&provider)
}
```

`dto.rs` (next to the other DTOs):

```rust
/// AI settings as exposed over IPC (M7). Never carries key material.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiSettingsDto {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
}

impl From<AiSettings> for AiSettingsDto {
    fn from(s: AiSettings) -> Self {
        Self { enabled: s.enabled, provider: s.provider, model: s.model }
    }
}

impl From<AiSettingsDto> for AiSettings {
    fn from(s: AiSettingsDto) -> Self {
        Self { enabled: s.enabled, provider: s.provider, model: s.model }
    }
}
```

(Import `AiSettings` in dto.rs; place the `From` impls wherever dto.rs keeps its conversions.) In `lib.rs`: `pub mod ai_commands;`; add the five commands to `collect_commands![]` (`ai_commands::get_ai_settings, ai_commands::set_ai_settings, ai_commands::set_ai_key, ai_commands::clear_ai_key, ai_commands::ai_key_present`); in `run()`'s `.setup(...)`, next to the owner manage call: `app.manage(crate::ai_commands::AiKeyStoreState(std::sync::Arc::new(crate::ai_settings::OsKeyStore)));`

- [ ] **Step 4: Regenerate bindings and run tests**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo test binding_gen && cargo test ai_commands`
Expected: bindings test regenerates `src/ipc/bindings.ts` (now containing `getAiSettings`, `setAiSettings`, `setAiKey`, `clearAiKey`, `aiKeyPresent` and `AiSettingsDto`); ai_commands tests PASS (4). NOTE: the `binding_gen` test module in `src-tauri/src/lib.rs` asserts generated content against an expected-names list — if it fails listing missing/unexpected names, add the five new command names to that list (mirroring how existing commands are listed), then re-run.

- [ ] **Step 5: Gates + commit**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Run (repo root): `npx tsc --noEmit -p tsconfig.json`
Expected: all green (tsc confirms the regenerated bindings still typecheck).

```bash
git add src-tauri/src/ai_commands.rs src-tauri/src/lib.rs src-tauri/src/dto.rs src/ipc/bindings.ts
git commit -m "feat(ai): add AI settings and key management commands"
```

---

### Task 3: AiProvider abstraction + fake provider

**Files:**
- Create: `src-tauri/src/ai_provider.rs`
- Modify: `src-tauri/Cargo.toml` (add `reqwest`, `futures-util`)
- Modify: `src-tauri/src/lib.rs` (add `pub mod ai_provider;`)

**Interfaces:**
- Consumes: `opentune_ai::ToolSpec` (fields `name: &'static str`, `description: &'static str`, `input_schema: serde_json::Value`).
- Produces (Tasks 4/5 implement against these; slice 3 consumes):
  - `pub struct ToolDef { pub name: String, pub description: String, pub input_schema: serde_json::Value }` with `impl From<opentune_ai::ToolSpec> for ToolDef`
  - `pub enum ChatMessage { User { text: String }, Assistant { blocks: Vec<AssistantBlock> }, ToolResults { results: Vec<ToolResultMsg> } }`
  - `pub enum AssistantBlock { Text { text: String }, ToolUse { id: String, name: String, input: serde_json::Value } }`
  - `pub struct ToolResultMsg { pub tool_use_id: String, pub content: String, pub is_error: bool }`
  - `pub struct ChatRequest { pub system: String, pub messages: Vec<ChatMessage>, pub tools: Vec<ToolDef>, pub model: String, pub max_tokens: u32 }`
  - `pub enum StopReason { EndTurn, ToolUse, MaxTokens, Other(String) }`
  - `pub struct ChatTurn { pub blocks: Vec<AssistantBlock>, pub stop_reason: StopReason }`
  - `pub enum ProviderError { MissingKey, Http { status: u16, message: String }, Network(String), Protocol(String) }` (implements `Display`; `message` must never contain the key)
  - `pub type OnDelta<'a> = &'a mut (dyn FnMut(&str) + Send);`
  - `pub enum Provider { Anthropic(crate::ai_anthropic::AnthropicProvider), OpenAi(crate::ai_openai::OpenAiProvider), Fake(FakeProvider) }` with `pub async fn chat(&self, req: &ChatRequest, on_delta: OnDelta<'_>) -> Result<ChatTurn, ProviderError>` — **enum dispatch, no dyn/async_trait** (`// ponytail: enum over dyn — two real providers; revisit if a third party needs to plug in`). NOTE: the enum references Tasks 4/5 types that don't exist yet — in THIS task, declare the enum with only the `Fake` variant plus a `// Tasks 4/5 add Anthropic/OpenAi variants` comment; Tasks 4/5 each add their variant and arm.
  - `pub struct FakeProvider { pub turns: std::sync::Mutex<Vec<ChatTurn>> }` — pops scripted turns in order, emits each `Text` block through `on_delta` in two chunks (streaming shape for slice-3 tests), errors with `Protocol("fake provider script exhausted")` when empty.

- [ ] **Step 1: Add dependencies**

In `src-tauri/Cargo.toml` `[dependencies]`:

```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "stream", "rustls-tls"] }
futures-util = "0.3"
```

- [ ] **Step 2: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn scripted_turn() -> ChatTurn {
        ChatTurn {
            blocks: vec![
                AssistantBlock::Text { text: "hello world".into() },
                AssistantBlock::ToolUse {
                    id: "t1".into(),
                    name: "read_tune".into(),
                    input: serde_json::json!({ "names": ["reqFuel"] }),
                },
            ],
            stop_reason: StopReason::ToolUse,
        }
    }

    #[tokio::test]
    async fn fake_provider_pops_turns_and_streams_text() {
        let provider = Provider::Fake(FakeProvider::new(vec![scripted_turn()]));
        let req = ChatRequest {
            system: "s".into(),
            messages: vec![ChatMessage::User { text: "hi".into() }],
            tools: vec![],
            model: "fake".into(),
            max_tokens: 100,
        };
        let mut deltas = String::new();
        let mut on_delta = |d: &str| deltas.push_str(d);
        let turn = provider.chat(&req, &mut on_delta).await.expect("scripted turn");
        assert_eq!(turn.stop_reason, StopReason::ToolUse);
        assert_eq!(deltas, "hello world");
        assert_eq!(turn.blocks.len(), 2);
        let err = provider.chat(&req, &mut on_delta).await.unwrap_err();
        assert!(matches!(err, ProviderError::Protocol(_)));
    }

    #[test]
    fn tool_spec_converts_to_tool_def() {
        let spec = opentune_ai::registry()
            .into_iter()
            .find(|t| t.name == "read_tune")
            .expect("registry has read_tune");
        let def = ToolDef::from(spec);
        assert_eq!(def.name, "read_tune");
        assert_eq!(def.input_schema["type"], "object");
        assert!(!def.description.is_empty());
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo test ai_provider`
Expected: FAIL to compile. (Add `pub mod ai_provider;` to lib.rs first.)

- [ ] **Step 4: Implement**

Implement all the types from the Interfaces block verbatim, plus:

```rust
impl FakeProvider {
    pub fn new(turns: Vec<ChatTurn>) -> Self {
        // Reversed so pop() yields script order.
        let mut turns = turns;
        turns.reverse();
        Self { turns: std::sync::Mutex::new(turns) }
    }
}
```

`Provider::chat`'s Fake arm: pop the next turn; for each `AssistantBlock::Text`, split the text at `len/2` (char-boundary-safe: `text.char_indices().nth(text.chars().count() / 2)`) and call `on_delta` with each half (skip the second call when empty); return the turn. `From<ToolSpec>`: name/description via `.to_owned()`, schema moved. `ProviderError`'s `Display`: human-readable, no secrets. All types derive `Debug, Clone, PartialEq` where fields allow (Mutex-holding `FakeProvider` derives none; hand-write what tests need).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo test ai_provider`
Expected: PASS (2 tests).

- [ ] **Step 6: Gates + commit**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green (reqwest/futures-util may be flagged unused by nothing — they are used from Task 4 on; if clippy complains about unused deps it will not, that lint is allow-by-default).

```bash
git add src-tauri/src/ai_provider.rs src-tauri/src/lib.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(ai): add provider abstraction with fake provider"
```

---

### Task 4: Anthropic provider (request builder + SSE assembler + HTTP)

**Files:**
- Create: `src-tauri/src/ai_anthropic.rs`
- Modify: `src-tauri/src/ai_provider.rs` (add `Anthropic(crate::ai_anthropic::AnthropicProvider)` variant + chat arm)
- Modify: `src-tauri/src/lib.rs` (add `pub mod ai_anthropic;`)

**Interfaces:**
- Consumes: Task 3's types; `reqwest`, `futures_util::StreamExt`.
- Produces: `pub struct AnthropicProvider { pub api_key: String }` (hand-written `Debug` that redacts the key) with `pub async fn chat(&self, req: &ChatRequest, on_delta: OnDelta<'_>) -> Result<ChatTurn, ProviderError>`; pure internals `pub(crate) fn build_request_body(req: &ChatRequest) -> serde_json::Value` and `pub(crate) struct SseAssembler` with `pub(crate) fn feed(&mut self, event: &str, data: &str, on_delta: OnDelta<'_>) -> Result<(), ProviderError>` and `pub(crate) fn finish(self) -> Result<ChatTurn, ProviderError>`.

**Wire contract (verify against current docs before implementing; note drift in your report):** `POST https://api.anthropic.com/v1/messages`, headers `x-api-key: <key>`, `anthropic-version: 2023-06-01`, `content-type: application/json`. Body: `{"model", "max_tokens", "system", "stream": true, "tools": [{"name", "description", "input_schema"}], "messages": [...]}` where messages map: `User{text}` → `{"role":"user","content":text}`; `Assistant{blocks}` → `{"role":"assistant","content":[{"type":"text","text":...} | {"type":"tool_use","id":...,"name":...,"input":...}]}`; `ToolResults{results}` → `{"role":"user","content":[{"type":"tool_result","tool_use_id":...,"content":...,"is_error":...}]}`. Do NOT send `temperature`/`top_p`/`top_k` (rejected on current models). SSE events: `message_start`; `content_block_start` (`content_block.type` = `"text"` or `"tool_use"` with `id`/`name`); `content_block_delta` (`delta.type` = `"text_delta"` with `.text` → forward to `on_delta` AND accumulate, or `"input_json_delta"` with `.partial_json` → accumulate string); `content_block_stop` (parse accumulated tool-input JSON here); `message_delta` (carries `delta.stop_reason`: `"end_turn"`/`"tool_use"`/`"max_tokens"`/other); `message_stop`; `ping` (ignore); `error` (→ `ProviderError::Protocol` with the error message). Index blocks by the event's `index` field.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_provider::{AssistantBlock, ChatMessage, ChatRequest, StopReason, ToolDef, ToolResultMsg};

    fn req() -> ChatRequest {
        ChatRequest {
            system: "You are a tuner.".into(),
            messages: vec![
                ChatMessage::User { text: "check afr".into() },
                ChatMessage::Assistant {
                    blocks: vec![AssistantBlock::ToolUse {
                        id: "tu_1".into(),
                        name: "read_tune".into(),
                        input: serde_json::json!({ "names": ["reqFuel"] }),
                    }],
                },
                ChatMessage::ToolResults {
                    results: vec![ToolResultMsg {
                        tool_use_id: "tu_1".into(),
                        content: "{\"values\":[]}".into(),
                        is_error: false,
                    }],
                },
            ],
            tools: vec![ToolDef {
                name: "read_tune".into(),
                description: "d".into(),
                input_schema: serde_json::json!({ "type": "object", "properties": {} }),
            }],
            model: "claude-sonnet-5".into(),
            max_tokens: 4096,
        }
    }

    #[test]
    fn request_body_matches_wire_contract() {
        let body = build_request_body(&req());
        assert_eq!(body["model"], "claude-sonnet-5");
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["stream"], true);
        assert_eq!(body["system"], "You are a tuner.");
        assert_eq!(body["tools"][0]["name"], "read_tune");
        assert!(body.get("temperature").is_none());
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(body["messages"][2]["role"], "user");
        assert_eq!(body["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "tu_1");
    }

    #[test]
    fn sse_assembler_builds_text_and_tool_use_turn() {
        let mut asm = SseAssembler::default();
        let mut deltas = String::new();
        let mut on_delta = |d: &str| deltas.push_str(d);
        asm.feed("message_start", r#"{"type":"message_start"}"#, &mut on_delta).unwrap();
        asm.feed("content_block_start", r#"{"index":0,"content_block":{"type":"text","text":""}}"#, &mut on_delta).unwrap();
        asm.feed("content_block_delta", r#"{"index":0,"delta":{"type":"text_delta","text":"Lean at "}}"#, &mut on_delta).unwrap();
        asm.feed("content_block_delta", r#"{"index":0,"delta":{"type":"text_delta","text":"4500rpm"}}"#, &mut on_delta).unwrap();
        asm.feed("content_block_stop", r#"{"index":0}"#, &mut on_delta).unwrap();
        asm.feed("content_block_start", r#"{"index":1,"content_block":{"type":"tool_use","id":"tu_9","name":"run_ve_analyze","input":{}}}"#, &mut on_delta).unwrap();
        asm.feed("content_block_delta", r#"{"index":1,"delta":{"type":"input_json_delta","partial_json":"{\"table\":"}}"#, &mut on_delta).unwrap();
        asm.feed("content_block_delta", r#"{"index":1,"delta":{"type":"input_json_delta","partial_json":"\"veTable1Tbl\"}"}}"#, &mut on_delta).unwrap();
        asm.feed("content_block_stop", r#"{"index":1}"#, &mut on_delta).unwrap();
        asm.feed("message_delta", r#"{"delta":{"stop_reason":"tool_use"}}"#, &mut on_delta).unwrap();
        asm.feed("message_stop", r#"{"type":"message_stop"}"#, &mut on_delta).unwrap();
        let turn = asm.finish().expect("complete turn");
        assert_eq!(deltas, "Lean at 4500rpm");
        assert_eq!(turn.stop_reason, StopReason::ToolUse);
        assert_eq!(turn.blocks.len(), 2);
        match &turn.blocks[1] {
            AssistantBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tu_9");
                assert_eq!(name, "run_ve_analyze");
                assert_eq!(input["table"], "veTable1Tbl");
            }
            other => panic!("expected tool_use, got {other:?}"),
        }
    }

    #[test]
    fn sse_error_event_becomes_protocol_error() {
        let mut asm = SseAssembler::default();
        let mut on_delta = |_: &str| {};
        let err = asm
            .feed("error", r#"{"type":"error","error":{"type":"overloaded_error","message":"try later"}}"#, &mut on_delta)
            .unwrap_err();
        assert!(matches!(err, crate::ai_provider::ProviderError::Protocol(_)));
    }

    #[test]
    fn debug_redacts_key() {
        let p = AnthropicProvider { api_key: "test-key".into() };
        let dbg = format!("{p:?}");
        assert!(!dbg.contains("test-key"));
    }
}
```

Coercion note: `&mut on_delta` where `OnDelta` is expected relies on auto-unsizing of the closure reference — this works for local closures capturing locals. If the `Send` bound on `OnDelta` fights the tests at implementation time, relax Task 3's type to `&'a mut dyn FnMut(&str)` (both real providers only call it inline) and note the change in your report.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo test ai_anthropic`
Expected: FAIL to compile. (Add `pub mod ai_anthropic;` first.)

- [ ] **Step 3: Implement**

- `build_request_body` per the wire contract above (pure serde_json construction).
- `SseAssembler`: `blocks: std::collections::BTreeMap<u64, PendingBlock>` (BTreeMap for deterministic index order), `stop_reason: Option<StopReason>`, where `PendingBlock` is `Text(String)` or `ToolUse { id: String, name: String, json: String }`. `feed` matches the event name, parses `data` with `serde_json::from_str::<serde_json::Value>`, updates state; `text_delta` forwards to `on_delta` before accumulating. `content_block_stop` on a ToolUse parses the accumulated JSON (empty string → `{}`); parse failure → `ProviderError::Protocol`. `finish` requires a seen `stop_reason` (else Protocol error: "stream ended without message_delta") and maps `"end_turn"`→EndTurn, `"tool_use"`→ToolUse, `"max_tokens"`→MaxTokens, other→`Other(s)`.
- `AnthropicProvider::chat`: build client `reqwest::Client::new()`, POST with the three headers, `build_request_body`; on non-success status read the body text and return `ProviderError::Http { status, message }` (body may contain provider error JSON — pass it through; it cannot contain the key). On success: `resp.bytes_stream()`, accumulate a line buffer, split on `\n`, track the current `event:` name, on each `data:` line call `assembler.feed(current_event, data, on_delta)`; on stream end call `finish()`. SSE lines may arrive split across chunks — the buffer handles partials; blank line resets the current event name.
- `impl std::fmt::Debug for AnthropicProvider` printing `AnthropicProvider {{ api_key: "<redacted>" }}`.
- Add the `Anthropic` variant + arm to `Provider::chat` in ai_provider.rs.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo test ai_anthropic && cargo test ai_provider`
Expected: PASS (4 + 2 tests).

- [ ] **Step 5: Gates + commit**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green.

```bash
git add src-tauri/src/ai_anthropic.rs src-tauri/src/ai_provider.rs src-tauri/src/lib.rs
git commit -m "feat(ai): add Anthropic provider with streaming tool use"
```

---

### Task 5: OpenAI provider (request builder + SSE assembler + HTTP)

**Files:**
- Create: `src-tauri/src/ai_openai.rs`
- Modify: `src-tauri/src/ai_provider.rs` (add `OpenAi(crate::ai_openai::OpenAiProvider)` variant + chat arm)
- Modify: `src-tauri/src/lib.rs` (add `pub mod ai_openai;`)

**Interfaces:**
- Consumes: Task 3's types; `reqwest`, `futures_util::StreamExt`.
- Produces: `pub struct OpenAiProvider { pub api_key: String }` (redacting `Debug`) with the same `chat` signature as Anthropic; pure internals `pub(crate) fn build_request_body(req: &ChatRequest) -> serde_json::Value` and `pub(crate) struct SseAssembler { ... }` with `pub(crate) fn feed(&mut self, data: &str, on_delta: OnDelta<'_>) -> Result<bool, ProviderError>` (returns `true` when `data` was the `[DONE]` sentinel) and `pub(crate) fn finish(self) -> Result<ChatTurn, ProviderError>`.

**Wire contract (verify against current docs; note drift):** `POST https://api.openai.com/v1/chat/completions`, headers `Authorization: Bearer <key>`, `content-type: application/json`. Body: `{"model", "stream": true, "max_completion_tokens", "messages": [...], "tools": [{"type":"function","function":{"name","description","parameters"}}]}` where messages map: system prompt → leading `{"role":"system","content":system}`; `User{text}` → `{"role":"user","content":text}`; `Assistant{blocks}` → `{"role":"assistant","content": <joined text or null>, "tool_calls":[{"id","type":"function","function":{"name","arguments": input-as-STRING}}]}` (omit `tool_calls` when none, omit/null `content` when no text); `ToolResults{results}` → one `{"role":"tool","tool_call_id","content"}` message PER result. SSE: every line is `data: <json>` (no named events), chunks carry `choices[0].delta` with optional `content` (string → forward to `on_delta` + accumulate) and optional `tool_calls` array of fragments `{index, id?, function: {name?, arguments-fragment?}}` (accumulate by index; `id`/`name` arrive on the first fragment); `choices[0].finish_reason` (`"stop"`→EndTurn, `"tool_calls"`→ToolUse, `"length"`→MaxTokens, other→Other) arrives on the final content chunk; terminated by `data: [DONE]`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_provider::{AssistantBlock, ChatMessage, ChatRequest, StopReason, ToolDef, ToolResultMsg};

    fn req() -> ChatRequest {
        ChatRequest {
            system: "You are a tuner.".into(),
            messages: vec![
                ChatMessage::User { text: "check afr".into() },
                ChatMessage::Assistant {
                    blocks: vec![AssistantBlock::ToolUse {
                        id: "call_1".into(),
                        name: "read_tune".into(),
                        input: serde_json::json!({ "names": ["reqFuel"] }),
                    }],
                },
                ChatMessage::ToolResults {
                    results: vec![ToolResultMsg {
                        tool_use_id: "call_1".into(),
                        content: "{\"values\":[]}".into(),
                        is_error: false,
                    }],
                },
            ],
            tools: vec![ToolDef {
                name: "read_tune".into(),
                description: "d".into(),
                input_schema: serde_json::json!({ "type": "object", "properties": {} }),
            }],
            model: "gpt-x".into(),
            max_tokens: 4096,
        }
    }

    #[test]
    fn request_body_matches_wire_contract() {
        let body = build_request_body(&req());
        assert_eq!(body["model"], "gpt-x");
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_completion_tokens"], 4096);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][2]["tool_calls"][0]["function"]["name"], "read_tune");
        // Arguments must be a STRING of JSON, not an object.
        assert!(body["messages"][2]["tool_calls"][0]["function"]["arguments"].is_string());
        assert_eq!(body["messages"][3]["role"], "tool");
        assert_eq!(body["messages"][3]["tool_call_id"], "call_1");
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "read_tune");
    }

    #[test]
    fn sse_assembler_builds_text_and_tool_call_turn() {
        let mut asm = SseAssembler::default();
        let mut deltas = String::new();
        let mut on_delta = |d: &str| deltas.push_str(d);
        assert!(!asm.feed(r#"{"choices":[{"delta":{"content":"Lean at "}}]}"#, &mut on_delta).unwrap());
        assert!(!asm.feed(r#"{"choices":[{"delta":{"content":"4500rpm"}}]}"#, &mut on_delta).unwrap());
        assert!(!asm.feed(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_9","function":{"name":"run_ve_analyze","arguments":"{\"table\":"}}]}}]}"#, &mut on_delta).unwrap());
        assert!(!asm.feed(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"veTable1Tbl\"}"}}]},"finish_reason":null}]}"#, &mut on_delta).unwrap());
        assert!(!asm.feed(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#, &mut on_delta).unwrap());
        assert!(asm.feed("[DONE]", &mut on_delta).unwrap());
        let turn = asm.finish().expect("complete turn");
        assert_eq!(deltas, "Lean at 4500rpm");
        assert_eq!(turn.stop_reason, StopReason::ToolUse);
        match &turn.blocks[1] {
            AssistantBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_9");
                assert_eq!(name, "run_ve_analyze");
                assert_eq!(input["table"], "veTable1Tbl");
            }
            other => panic!("expected tool call, got {other:?}"),
        }
    }

    #[test]
    fn missing_finish_reason_is_protocol_error() {
        let mut asm = SseAssembler::default();
        let mut on_delta = |_: &str| {};
        asm.feed(r#"{"choices":[{"delta":{"content":"hi"}}]}"#, &mut on_delta).unwrap();
        asm.feed("[DONE]", &mut on_delta).unwrap();
        assert!(matches!(asm.finish(), Err(crate::ai_provider::ProviderError::Protocol(_))));
    }

    #[test]
    fn debug_redacts_key() {
        let p = OpenAiProvider { api_key: "test-key".into() };
        assert!(!format!("{p:?}").contains("test-key"));
    }
}
```

(If the second tool_calls fixture line's escaping fights you, restructure the JSON string literal — the semantic content is what matters: a second fragment for index 0 carrying the rest of `arguments`.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo test ai_openai`
Expected: FAIL to compile. (Add `pub mod ai_openai;` first.)

- [ ] **Step 3: Implement**

Mirror Task 4's structure: pure `build_request_body`; `SseAssembler` with `text: String`, `calls: BTreeMap<u64, PendingCall { id, name, arguments: String }>`, `finish_reason: Option<StopReason>`, `done: bool`; `feed` returns `Ok(true)` for the literal `[DONE]`, otherwise parses JSON and folds `delta.content` / `delta.tool_calls` / `finish_reason`; `finish` errors without a seen finish_reason, else builds `ChatTurn` with the text block first (when non-empty) followed by tool calls in index order (arguments parsed with `serde_json::from_str`, empty → `{}`, parse failure → Protocol). `OpenAiProvider::chat`: POST with Bearer header, non-success → `Http { status, message }`, success → line-buffered `bytes_stream` loop feeding `data:` payloads until `[DONE]` or stream end, then `finish()`. Redacting `Debug`. Add the `OpenAi` variant + arm in ai_provider.rs.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo test ai_openai && cargo test ai_provider && cargo test ai_anthropic`
Expected: PASS (4 + 2 + 4 tests).

- [ ] **Step 5: Gates + commit**

Run: `cd src-tauri && . "$HOME/.cargo/env" && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green.

```bash
git add src-tauri/src/ai_openai.rs src-tauri/src/ai_provider.rs src-tauri/src/lib.rs
git commit -m "feat(ai): add OpenAI provider with streaming tool calls"
```

---

### Task 6: AI settings UI section

**Files:**
- Create: `src/components/ai/AiSettingsPanel.tsx`
- Create: `src/components/ai/ai.css`
- Create: `src/components/ai/AiSettingsPanel.test.tsx`
- Modify: `src/i18n/en.ts`, `src/i18n/pl.ts` (new `ai.*` keys, both files — parity is type-enforced)
- Modify: `src/App.tsx` (render `<AiSettingsPanel locale={locale} />` after `<DatalogPanel …>`)

**Interfaces:**
- Consumes: generated `commands.getAiSettings`, `commands.setAiSettings`, `commands.setAiKey`, `commands.clearAiKey`, `commands.aiKeyPresent` and type `AiSettingsDto` from `src/ipc/bindings.ts` (Task 2); `t(key, locale)` from `src/i18n`; design tokens from `src/styles/tokens.css`.
- Produces: `AiSettingsPanel({ locale }: { locale: Locale })` — a stacked `<section>` (house pattern; M8 is the UX/UI milestone, keep this minimal).

**Behavior:**
- On mount: load settings + key-present for the selected provider. Result-object convention: every command resolves `{status:"ok",data}|{status:"error",error}` — branch on `status`, surface errors via `<p role="alert">`.
- Controls: (1) enable checkbox — label + one consent sentence (i18n `ai.consent`: data is sent to the selected provider only when enabled; off by default); (2) provider `<select>` (anthropic/openai); (3) model text `<input>`; (4) "Save settings" button calling `setAiSettings`; (5) key `<input type="password">` (value held only in local state, cleared after save) + "Save key" (`setAiKey`, then re-query `aiKeyPresent`, clear the field) + "Clear key" (`clearAiKey`, re-query); (6) key status line: i18n `ai.keyPresent` / `ai.keyMissing` for the CURRENT provider (re-query on provider change).
- All labels/status/errors via i18n. Keys to add (en + pl): `ai.title`, `ai.enable`, `ai.consent`, `ai.provider`, `ai.model`, `ai.saveSettings`, `ai.apiKey`, `ai.saveKey`, `ai.clearKey`, `ai.keyPresent`, `ai.keyMissing`, `ai.saved`. Polish translations must be real Polish (e.g. `ai.title: "Asystent AI"`, `ai.enable: "Włącz AI (opt-in)"`, `ai.consent: "Po włączeniu dane strojenia są wysyłane do wybranego dostawcy AI. Domyślnie wyłączone."`, `ai.apiKey: "Klucz API"`, `ai.keyPresent: "Klucz zapisany"`, `ai.keyMissing: "Brak klucza"`, `ai.saveSettings: "Zapisz ustawienia"`, `ai.saveKey: "Zapisz klucz"`, `ai.clearKey: "Usuń klucz"`, `ai.provider: "Dostawca"`, `ai.model: "Model"`, `ai.saved: "Zapisano"`).
- CSS: feature-prefixed classes (`.ai-settings`, `.ai-field`, …) using existing tokens only; both themes work automatically.

- [ ] **Step 1: Write the failing tests**

`AiSettingsPanel.test.tsx` — mock the bindings module the way existing panel tests do (see `src/components/datalog/DatalogPanel.test.tsx` for the house mocking pattern; mock `../../ipc/bindings`' `commands` object):

```tsx
// SPDX-License-Identifier: GPL-3.0-or-later
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getAiSettings: vi.fn(),
  setAiSettings: vi.fn(),
  setAiKey: vi.fn(),
  clearAiKey: vi.fn(),
  aiKeyPresent: vi.fn(),
}));

vi.mock("../../ipc/bindings", () => ({ commands: mocks }));

import { AiSettingsPanel } from "./AiSettingsPanel";

const ok = (data: unknown) => Promise.resolve({ status: "ok", data });

beforeEach(() => {
  vi.clearAllMocks();
  mocks.getAiSettings.mockReturnValue(
    ok({ enabled: false, provider: "anthropic", model: "claude-sonnet-5" }),
  );
  mocks.aiKeyPresent.mockReturnValue(ok(false));
  mocks.setAiSettings.mockReturnValue(ok(null));
  mocks.setAiKey.mockReturnValue(ok(null));
  mocks.clearAiKey.mockReturnValue(ok(null));
});

describe("AiSettingsPanel", () => {
  it("loads settings and shows key-missing status", async () => {
    render(<AiSettingsPanel locale="en" />);
    await waitFor(() => expect(mocks.getAiSettings).toHaveBeenCalled());
    expect(await screen.findByText("No API key saved")).toBeInTheDocument();
    expect(screen.getByLabelText("Enable AI (opt-in)")).not.toBeChecked();
  });

  it("saves the key write-only and clears the field", async () => {
    const user = userEvent.setup();
    mocks.aiKeyPresent.mockReturnValueOnce(ok(false)).mockReturnValue(ok(true));
    render(<AiSettingsPanel locale="en" />);
    const field = await screen.findByLabelText("API key");
    await user.type(field, "test-key");
    await user.click(screen.getByRole("button", { name: "Save key" }));
    await waitFor(() =>
      expect(mocks.setAiKey).toHaveBeenCalledWith("anthropic", "test-key"),
    );
    expect((field as HTMLInputElement).value).toBe("");
    expect(await screen.findByText("API key saved")).toBeInTheDocument();
  });

  it("persists settings changes", async () => {
    const user = userEvent.setup();
    render(<AiSettingsPanel locale="en" />);
    await screen.findByLabelText("Enable AI (opt-in)");
    await user.click(screen.getByLabelText("Enable AI (opt-in)"));
    await user.click(screen.getByRole("button", { name: "Save settings" }));
    await waitFor(() =>
      expect(mocks.setAiSettings).toHaveBeenCalledWith(
        expect.objectContaining({ enabled: true, provider: "anthropic" }),
      ),
    );
  });

  it("surfaces command errors via role=alert", async () => {
    mocks.getAiSettings.mockReturnValue(
      Promise.resolve({ status: "error", error: "boom" }),
    );
    render(<AiSettingsPanel locale="en" />);
    expect(await screen.findByRole("alert")).toHaveTextContent("boom");
  });
});
```

(Exact English strings above come from the en.ts keys you add — keep them in sync: `ai.keyMissing: "No API key saved"`, `ai.keyPresent: "API key saved"`, `ai.enable: "Enable AI (opt-in)"`, `ai.apiKey: "API key"`, `ai.saveKey: "Save key"`, `ai.saveSettings: "Save settings"`.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm test -- AiSettingsPanel`
Expected: FAIL — component does not exist.

- [ ] **Step 3: Implement the component, CSS, i18n keys, App wiring**

Follow the house panel pattern (a `<section>` with `<h2>{t("ai.title", locale)}</h2>`, labeled controls, `aria-busy` on in-flight buttons per `UpdateNotice.tsx`, errors via `<p role="alert">`, status via `role="status"`). Import `./ai.css`. Wire into `App.tsx` after DatalogPanel. Add ALL i18n keys to both en.ts and pl.ts (build breaks on missing parity — that is the enforcement working).

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test -- AiSettingsPanel`
Expected: PASS (4 tests).

- [ ] **Step 5: Full frontend + Rust gates + commit**

Run (repo root): `npm run lint && npm run format:check && npx tsc --noEmit -p tsconfig.json && npm test`
Run (src-tauri): `. "$HOME/.cargo/env" && cargo test --workspace`
Expected: all green (i18n parity test included).

```bash
git add src/components/ai src/i18n/en.ts src/i18n/pl.ts src/App.tsx
git commit -m "feat(ai): add AI settings panel with BYOK key entry"
```

---

### Task 7: Docs + whole-slice gates

**Files:**
- Modify: `docs/ARCHITECTURE.md` (§5.10 note: provider layer + settings exist as of slice 2; assistant UI and MCP server remain)

**Interfaces:** none — documentation only.

- [ ] **Step 1: Update the architecture doc**

Extend the §5.10 sentence updated in slice 1: as of M7 slice 2 the app side adds the BYOK provider layer (`ai_provider.rs` abstraction with Anthropic/OpenAI streaming implementations, `ai_settings.rs` keyring-backed key storage + persisted opt-in settings, `ai_commands.rs` IPC). The embedded assistant and MCP server remain for later M7 slices. Match surrounding style/width.

- [ ] **Step 2: Full gates, both stacks**

Run from `src-tauri/`: `. "$HOME/.cargo/env" && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Run from repo root: `npm run lint && npm run format:check && npx tsc --noEmit -p tsconfig.json && npm test`
Expected: everything green.

- [ ] **Step 3: Commit**

```bash
git add docs/ARCHITECTURE.md
git commit -m "docs(architecture): record BYOK provider layer"
```

---

## Final: whole-branch review

Dispatch the final code reviewer (most capable model) with a review package over `main..HEAD`: this plan, the diff, and the minor-findings ledger from task-level reviews. Security focus: key handling paths (no logging/echo/serialization of keys anywhere), CSP still intact, consent copy accurate. Fix wave (single fixer) for anything Critical/Important. Then: gates, push `-u origin m7-provider-byok`, PR to `main` titled `feat(m7): add BYOK provider layer (Anthropic, OpenAI, key storage, settings)`, body mapping deliverables to commits and naming deferrals (chat loop + assistant panel = slice 3; MCP server = slice 4).
