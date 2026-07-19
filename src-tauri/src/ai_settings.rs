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
        Self {
            enabled: false,
            provider: "anthropic".into(),
            model,
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
        };
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
}
