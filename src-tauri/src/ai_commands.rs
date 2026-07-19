// SPDX-License-Identifier: GPL-3.0-or-later
//! IPC commands for AI settings and BYOK key management (M7 slice 2).
//! Keys flow one way: UI → keyring. `ai_key_present` returns a bool only —
//! no command ever returns key material.

use std::path::Path;
use std::sync::Arc;

use tauri::State;

use crate::ai_settings::{load_ai_settings_in, save_ai_settings_in, validate_provider, KeyStore};
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
pub async fn set_ai_settings(app: tauri::AppHandle, settings: AiSettingsDto) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_settings::MemoryKeyStore;

    #[test]
    fn set_key_trims_and_rejects_empty() {
        let store = MemoryKeyStore::default();
        set_ai_key_in(&store, "anthropic", "  test-key  ").expect("trimmed key stored");
        assert_eq!(
            store.get_key("anthropic").unwrap().as_deref(),
            Some("test-key")
        );
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
        let bad = AiSettingsDto {
            enabled: true,
            provider: "openai".into(),
            model: "  ".into(),
        };
        assert!(set_ai_settings_in(&dir, bad).is_err());
        let ok = AiSettingsDto {
            enabled: true,
            provider: "openai".into(),
            model: "gpt-x".into(),
        };
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
        let s = AiSettingsDto {
            enabled: false,
            provider: "anthropic".into(),
            model: String::new(),
        };
        set_ai_settings_in(&dir, s).expect("disabled settings persist without model");
    }
}
