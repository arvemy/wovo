use crate::codex::account_store::default_wovo_codex_root;
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
}

impl Default for CodexSettings {
    fn default() -> Self {
        Self {
            usage_source_mode: CodexUsageSourceMode::Auto,
        }
    }
}

pub fn load_settings() -> Result<CodexSettings, AppError> {
    load_settings_from_path(&settings_path())
}

pub fn save_usage_source_mode(mode: CodexUsageSourceMode) -> Result<CodexSettings, AppError> {
    let settings = CodexSettings {
        usage_source_mode: mode,
    };
    save_settings_to_path(&settings_path(), &settings)?;
    Ok(settings)
}

fn settings_path() -> PathBuf {
    default_wovo_codex_root().join(SETTINGS_FILE_NAME)
}

fn load_settings_from_path(path: &Path) -> Result<CodexSettings, AppError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(CodexSettings::default()),
        Err(error) => return Err(AppError::AccountStore(error.to_string())),
    };

    serde_json::from_str(&contents).map_err(|error| AppError::AccountStore(error.to_string()))
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

        let settings = load_settings_from_path(&path).unwrap();

        assert_eq!(settings.usage_source_mode, CodexUsageSourceMode::Auto);
    }

    #[test]
    fn saves_and_loads_usage_source_mode() {
        let path = temp_settings_path("save-settings");
        let settings = CodexSettings {
            usage_source_mode: CodexUsageSourceMode::Cli,
        };

        save_settings_to_path(&path, &settings).unwrap();
        let loaded = load_settings_from_path(&path).unwrap();

        assert_eq!(loaded, settings);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
