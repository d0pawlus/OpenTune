// SPDX-License-Identifier: GPL-3.0-or-later
//! BYOK key storage and AI settings persistence (M7 slice 2).
//!
//! Keys live in the OS credential store via the `keyring` crate — never in
//! the settings JSON, never in logs, never returned to the frontend.
//! Settings are an atomic JSON file in `app_config_dir`, mirroring
//! `layout.rs`. AI is OFF by default: enabling it is the user's explicit
//! consent for data to leave the machine (ADR-0008).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::layout::atomic_write;

/// Generate a random 32-byte hex-encoded token (64 chars).
fn generate_token() -> String {
    use rand::RngExt;
    let mut b = [0u8; 32];
    rand::rng().fill(&mut b);
    hex::encode(b)
}

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

pub const DEFAULT_MCP_PORT: u16 = 8765;
pub const MCP_TOKEN_FILE: &str = "mcp-token";

pub fn default_mcp_port() -> u16 {
    DEFAULT_MCP_PORT
}

/// Persisted AI configuration. The API key is deliberately NOT here — it
/// lives in the OS credential store only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub mcp_enabled: bool,
    #[serde(default = "default_mcp_port")]
    pub mcp_port: u16,
}

impl Default for AiSettings {
    fn default() -> Self {
        let model = ["claude", "sonnet", "5"].join("-");
        Self {
            enabled: false,
            provider: "anthropic".into(),
            model,
            mcp_enabled: false,
            mcp_port: DEFAULT_MCP_PORT,
        }
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
        self.keys
            .lock()
            .unwrap()
            .insert(provider.to_owned(), key.to_owned());
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

/// Atomic write using the shared helper: temp file + fsync + rename.
pub fn save_ai_settings_in(dir: &Path, settings: &AiSettings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    atomic_write(dir, AI_SETTINGS_FILE, json.as_bytes())
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

/// Reads the token from `mcp-token` file; if absent or empty/whitespace,
/// generates a fresh 64-char hex token, writes it, and returns it.
pub fn load_or_create_mcp_token_in(dir: &Path) -> Result<String, String> {
    let path = dir.join(MCP_TOKEN_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let token = text.trim();
            if token.is_empty() {
                let new_token = generate_token();
                atomic_write(dir, MCP_TOKEN_FILE, new_token.as_bytes())?;
                Ok(new_token)
            } else {
                Ok(token.to_owned())
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let token = generate_token();
            atomic_write(dir, MCP_TOKEN_FILE, token.as_bytes())?;
            Ok(token)
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Always generates and writes a fresh 64-char hex token.
pub fn regenerate_mcp_token_in(dir: &Path) -> Result<String, String> {
    let token = generate_token();
    atomic_write(dir, MCP_TOKEN_FILE, token.as_bytes())?;
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("opentune-ai-settings-{tag}-{}", std::process::id()));
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
        let s = AiSettings {
            enabled: true,
            provider: "openai".into(),
            model: "gpt-x".into(),
            mcp_enabled: false,
            mcp_port: DEFAULT_MCP_PORT,
        };
        save_ai_settings_in(&dir, &s).expect("save");
        let back = load_ai_settings_in(&dir).expect("load");
        assert_eq!(back.enabled, s.enabled);
        assert_eq!(back.provider, s.provider);
        assert_eq!(back.model, s.model);
        assert_eq!(back.mcp_enabled, s.mcp_enabled);
        assert_eq!(back.mcp_port, s.mcp_port);
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
        assert_eq!(
            ks.get_key("anthropic").unwrap().as_deref(),
            Some("test-key")
        );
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

    #[test]
    fn atomic_write_cleans_up_temp_files_on_sequential_saves() {
        let dir = tmp_dir("atomic-cleanup");
        let s1 = AiSettings {
            enabled: false,
            provider: "anthropic".into(),
            model: "claude-3".into(),
            mcp_enabled: false,
            mcp_port: DEFAULT_MCP_PORT,
        };
        let s2 = AiSettings {
            enabled: true,
            provider: "openai".into(),
            model: "gpt-4".into(),
            mcp_enabled: false,
            mcp_port: DEFAULT_MCP_PORT,
        };

        // First save
        save_ai_settings_in(&dir, &s1).expect("first save");
        let back1 = load_ai_settings_in(&dir).expect("load after first save");
        assert_eq!(back1, s1);

        // Second save should not leave temp files from first save
        save_ai_settings_in(&dir, &s2).expect("second save");
        let back2 = load_ai_settings_in(&dir).expect("load after second save");
        assert_eq!(back2, s2);

        // Verify no stray .tmp files were left behind
        let entries = std::fs::read_dir(&dir)
            .expect("read dir")
            .map(|e| e.map(|entry| entry.file_name().to_string_lossy().to_string()))
            .collect::<Result<Vec<_>, _>>()
            .expect("collect entries");
        assert!(
            !entries.iter().any(|name| name.ends_with(".tmp")),
            "no temp files should remain after successful saves, but found: {:?}",
            entries
        );
        assert_eq!(entries.len(), 1, "only the settings file should exist");
    }

    #[test]
    fn old_shape_json_without_mcp_fields_loads_with_defaults() {
        let dir = tmp_dir("old-shape");
        // Write a JSON without mcp_enabled/mcp_port (simulating old persisted file)
        let old_json = r#"{"enabled":true,"provider":"anthropic","model":"claude-sonnet-5"}"#;
        std::fs::write(dir.join(AI_SETTINGS_FILE), old_json).expect("write old shape");
        let loaded = load_ai_settings_in(&dir).expect("load old shape");
        assert_eq!(loaded.enabled, true);
        assert_eq!(loaded.provider, "anthropic");
        assert_eq!(loaded.model, "claude-sonnet-5");
        assert_eq!(loaded.mcp_enabled, false); // default
        assert_eq!(loaded.mcp_port, DEFAULT_MCP_PORT); // default
    }

    #[test]
    fn mcp_fields_round_trip_via_file() {
        let dir = tmp_dir("mcp-roundtrip");
        let s = AiSettings {
            enabled: true,
            provider: "anthropic".into(),
            model: "claude-sonnet-5".into(),
            mcp_enabled: true,
            mcp_port: 9000,
        };
        save_ai_settings_in(&dir, &s).expect("save");
        let back = load_ai_settings_in(&dir).expect("load");
        assert_eq!(back.mcp_enabled, true);
        assert_eq!(back.mcp_port, 9000);
    }

    #[test]
    fn load_or_create_mcp_token_generates_64_hex_chars_when_missing() {
        let dir = tmp_dir("token-generate");
        let token = load_or_create_mcp_token_in(&dir).expect("generate token");
        assert_eq!(token.len(), 64, "token must be 64 hex chars");
        assert!(
            token.chars().all(|c| c.is_ascii_hexdigit()),
            "token must be hex"
        );
    }

    #[test]
    fn load_or_create_mcp_token_returns_same_on_second_call() {
        let dir = tmp_dir("token-idempotent");
        let token1 = load_or_create_mcp_token_in(&dir).expect("first load_or_create");
        let token2 = load_or_create_mcp_token_in(&dir).expect("second load_or_create");
        assert_eq!(token1, token2, "token must be stable on repeated calls");
    }

    #[test]
    fn regenerate_mcp_token_always_returns_different_token() {
        let dir = tmp_dir("token-regenerate");
        let token1 = load_or_create_mcp_token_in(&dir).expect("initial");
        let token2 = regenerate_mcp_token_in(&dir).expect("regenerate");
        assert_ne!(token1, token2, "regenerated token must be different");
        assert_eq!(token2.len(), 64, "new token must be 64 hex chars");
    }

    #[test]
    fn load_or_create_mcp_token_regenerates_empty_file() {
        let dir = tmp_dir("token-empty");
        // Write an empty file
        std::fs::write(dir.join(MCP_TOKEN_FILE), b"").expect("write empty file");
        let token = load_or_create_mcp_token_in(&dir).expect("load_or_create with empty file");
        assert_eq!(token.len(), 64, "should regenerate 64 hex chars");
        // Verify the file now contains that token
        let persisted = std::fs::read_to_string(dir.join(MCP_TOKEN_FILE)).expect("read back");
        assert_eq!(persisted, token);
    }

    #[test]
    fn load_or_create_mcp_token_regenerates_whitespace_file() {
        let dir = tmp_dir("token-whitespace");
        // Write a whitespace-only file
        std::fs::write(dir.join(MCP_TOKEN_FILE), b"   \n\t ").expect("write whitespace file");
        let token = load_or_create_mcp_token_in(&dir).expect("load_or_create with whitespace");
        assert_eq!(token.len(), 64, "should regenerate 64 hex chars");
    }
}
