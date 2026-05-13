use crate::codex::account_store::default_wovo_codex_root;
use crate::codex::auth_store::system_codex_home_path;
use crate::codex::cost_usage;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const SETTINGS_FILE_NAME: &str = "codex-settings.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodexUsageSourceMode {
    Auto,
    Oauth,
    Cli,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSettings {
    pub usage_source_mode: CodexUsageSourceMode,
    pub cost_usage_enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredCodexSettings {
    #[serde(default)]
    usage_source_mode: Option<CodexUsageSourceMode>,
    #[serde(default)]
    cost_usage_enabled: Option<bool>,
}

impl Default for CodexSettings {
    fn default() -> Self {
        Self {
            usage_source_mode: CodexUsageSourceMode::Auto,
            cost_usage_enabled: false,
        }
    }
}

pub fn load_settings() -> Result<CodexSettings, AppError> {
    load_settings_from_path_with_codex_home(&settings_path(), &system_codex_home_path())
}

pub fn save_usage_source_mode(mode: CodexUsageSourceMode) -> Result<CodexSettings, AppError> {
    let mut settings = load_settings()?;
    settings.usage_source_mode = mode;
    save_settings_to_path(&settings_path(), &settings)?;
    Ok(settings)
}

pub fn save_cost_usage_enabled(enabled: bool) -> Result<CodexSettings, AppError> {
    let mut settings = load_settings()?;
    settings.cost_usage_enabled = enabled;
    save_settings_to_path(&settings_path(), &settings)?;
    Ok(settings)
}

fn settings_path() -> PathBuf {
    default_wovo_codex_root().join(SETTINGS_FILE_NAME)
}

fn load_settings_from_path_with_codex_home(
    path: &Path,
    codex_home: &Path,
) -> Result<CodexSettings, AppError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(CodexSettings {
                cost_usage_enabled: cost_usage::local_codex_logs_exist(codex_home),
                ..CodexSettings::default()
            })
        }
        Err(error) => return Err(AppError::AccountStore(error.to_string())),
    };

    let stored: StoredCodexSettings = serde_json::from_str(&contents)
        .map_err(|error| AppError::AccountStore(error.to_string()))?;
    Ok(CodexSettings {
        usage_source_mode: stored
            .usage_source_mode
            .unwrap_or(CodexUsageSourceMode::Auto),
        cost_usage_enabled: stored
            .cost_usage_enabled
            .unwrap_or_else(|| cost_usage::local_codex_logs_exist(codex_home)),
    })
}

fn save_settings_to_path(path: &Path, settings: &CodexSettings) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| AppError::AccountStore(error.to_string()))?;
    }
    let contents = serde_json::to_string_pretty(settings)
        .map_err(|error| AppError::AccountStore(error.to_string()))?;
    fs::write(path, contents).map_err(|error| AppError::AccountStore(error.to_string()))?;
    apply_secure_file_permissions(path)
}

#[cfg(unix)]
fn apply_secure_file_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions)
        .map_err(|error| AppError::AccountStore(error.to_string()))
}

#[cfg(not(unix))]
fn apply_secure_file_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn temp_settings_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("wovo-{name}-{}", Uuid::new_v4()))
            .join(SETTINGS_FILE_NAME)
    }

    #[test]
    fn missing_settings_default_to_auto() {
        let path = temp_settings_path("missing-settings");

        let settings =
            load_settings_from_path_with_codex_home(&path, path.parent().unwrap()).unwrap();

        assert_eq!(settings.usage_source_mode, CodexUsageSourceMode::Auto);
        assert!(!settings.cost_usage_enabled);
    }

    #[test]
    fn missing_settings_enable_cost_when_logs_exist() {
        let path = temp_settings_path("missing-settings-with-logs");
        let codex_home = path.parent().unwrap().join("codex-home");
        fs::create_dir_all(codex_home.join("sessions")).unwrap();
        fs::write(codex_home.join("sessions").join("session.jsonl"), "{}\n").unwrap();

        let settings = load_settings_from_path_with_codex_home(&path, &codex_home).unwrap();

        assert!(settings.cost_usage_enabled);
    }

    #[test]
    fn saves_and_loads_usage_source_mode() {
        let path = temp_settings_path("save-settings");
        let settings = CodexSettings {
            usage_source_mode: CodexUsageSourceMode::Cli,
            cost_usage_enabled: true,
        };

        save_settings_to_path(&path, &settings).unwrap();
        let loaded =
            load_settings_from_path_with_codex_home(&path, path.parent().unwrap()).unwrap();

        assert_eq!(loaded, settings);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn old_settings_json_loads_with_cost_default() {
        let path = temp_settings_path("old-settings");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"usageSourceMode":"oauth"}"#).unwrap();

        let loaded =
            load_settings_from_path_with_codex_home(&path, path.parent().unwrap()).unwrap();

        assert_eq!(loaded.usage_source_mode, CodexUsageSourceMode::Oauth);
        assert!(!loaded.cost_usage_enabled);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
