use crate::claude::account_store::default_wovo_claude_root;
use crate::claude::auth_store::system_claude_home_path;
use crate::claude::cost_usage;
use crate::codex::atomic_file::{replace_file, temporary_file_path, write_new_file};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const SETTINGS_FILE_NAME: &str = "claude-settings.json";
static SETTINGS_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaudeUsageSourceMode {
    Auto,
    Oauth,
    Cli,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSettings {
    pub usage_source_mode: ClaudeUsageSourceMode,
    pub cost_usage_enabled: bool,
    pub notifications_enabled: bool,
    pub auto_account_switching_enabled: bool,
    pub hide_account_credentials: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredClaudeSettings {
    #[serde(default)]
    usage_source_mode: Option<ClaudeUsageSourceMode>,
    #[serde(default)]
    cost_usage_enabled: Option<bool>,
    #[serde(default)]
    notifications_enabled: Option<bool>,
    #[serde(default)]
    auto_account_switching_enabled: Option<bool>,
    #[serde(default)]
    hide_account_credentials: Option<bool>,
}

impl Default for ClaudeSettings {
    fn default() -> Self {
        Self {
            usage_source_mode: ClaudeUsageSourceMode::Auto,
            cost_usage_enabled: false,
            notifications_enabled: true,
            auto_account_switching_enabled: false,
            hide_account_credentials: false,
        }
    }
}

pub fn load_settings() -> Result<ClaudeSettings, AppError> {
    load_settings_from_path_with_claude_home(&settings_path(), &system_claude_home_path())
}

pub fn save_usage_source_mode(mode: ClaudeUsageSourceMode) -> Result<ClaudeSettings, AppError> {
    update_settings(|settings| {
        settings.usage_source_mode = mode;
    })
}

pub fn save_cost_usage_enabled(enabled: bool) -> Result<ClaudeSettings, AppError> {
    update_settings(|settings| {
        settings.cost_usage_enabled = enabled;
    })
}

pub fn save_notifications_enabled(enabled: bool) -> Result<ClaudeSettings, AppError> {
    update_settings(|settings| {
        settings.notifications_enabled = enabled;
    })
}

pub fn save_auto_account_switching_enabled(enabled: bool) -> Result<ClaudeSettings, AppError> {
    update_settings(|settings| {
        settings.auto_account_switching_enabled = enabled;
    })
}

pub fn save_hide_account_credentials(enabled: bool) -> Result<ClaudeSettings, AppError> {
    update_settings(|settings| {
        settings.hide_account_credentials = enabled;
    })
}

fn settings_path() -> PathBuf {
    default_wovo_claude_root().join(SETTINGS_FILE_NAME)
}

fn update_settings(update: impl FnOnce(&mut ClaudeSettings)) -> Result<ClaudeSettings, AppError> {
    update_settings_at_path(&settings_path(), &system_claude_home_path(), update)
}

fn update_settings_at_path(
    path: &Path,
    claude_home: &Path,
    update: impl FnOnce(&mut ClaudeSettings),
) -> Result<ClaudeSettings, AppError> {
    let _guard = settings_write_lock().lock().map_err(|_| {
        AppError::ClaudeAccountStore("settings write lock was poisoned".to_string())
    })?;
    let mut settings = load_settings_from_path_with_claude_home(path, claude_home)?;
    update(&mut settings);
    save_settings_to_path(path, &settings)?;
    Ok(settings)
}

fn settings_write_lock() -> &'static Mutex<()> {
    SETTINGS_WRITE_LOCK.get_or_init(|| Mutex::new(()))
}

fn load_settings_from_path_with_claude_home(
    path: &Path,
    claude_home: &Path,
) -> Result<ClaudeSettings, AppError> {
    load_settings_from_path_with_cost_probe(path, claude_home, cost_usage::local_claude_logs_exist)
}

fn load_settings_from_path_with_cost_probe(
    path: &Path,
    claude_home: &Path,
    cost_logs_exist: impl Fn(&Path) -> bool,
) -> Result<ClaudeSettings, AppError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(ClaudeSettings {
                cost_usage_enabled: cost_logs_exist(claude_home),
                ..ClaudeSettings::default()
            })
        }
        Err(error) => return Err(AppError::ClaudeAccountStore(error.to_string())),
    };

    let stored: StoredClaudeSettings = serde_json::from_str(&contents)
        .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
    Ok(ClaudeSettings {
        usage_source_mode: stored
            .usage_source_mode
            .unwrap_or(ClaudeUsageSourceMode::Auto),
        cost_usage_enabled: stored
            .cost_usage_enabled
            .unwrap_or_else(|| cost_logs_exist(claude_home)),
        notifications_enabled: stored.notifications_enabled.unwrap_or(true),
        auto_account_switching_enabled: stored.auto_account_switching_enabled.unwrap_or(false),
        hide_account_credentials: stored.hide_account_credentials.unwrap_or(false),
    })
}

fn save_settings_to_path(path: &Path, settings: &ClaudeSettings) -> Result<(), AppError> {
    let contents = serde_json::to_string_pretty(settings)
        .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
    write_settings_file(path, contents.as_bytes())
}

fn write_settings_file(path: &Path, contents: &[u8]) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(|| {
        AppError::ClaudeAccountStore(format!(
            "settings path has no parent: {}",
            path.to_string_lossy()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;

    let tmp = temporary_file_path(parent, SETTINGS_FILE_NAME);
    write_new_file(&tmp, contents)
        .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
    if let Err(error) = apply_secure_file_permissions(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    replace_file(&tmp, path).map_err(|error| AppError::ClaudeAccountStore(error.to_string()))
}

#[cfg(unix)]
fn apply_secure_file_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions)
        .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))
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
            .join(format!("wovo-claude-{name}-{}", Uuid::new_v4()))
            .join(SETTINGS_FILE_NAME)
    }

    #[test]
    fn missing_settings_default_to_auto() {
        let path = temp_settings_path("missing-settings");

        let settings =
            load_settings_from_path_with_cost_probe(&path, path.parent().unwrap(), |_| false)
                .unwrap();

        assert_eq!(settings.usage_source_mode, ClaudeUsageSourceMode::Auto);
        assert!(!settings.cost_usage_enabled);
        assert!(!settings.auto_account_switching_enabled);
        assert!(!settings.hide_account_credentials);
    }

    #[test]
    fn missing_settings_enable_cost_when_logs_exist() {
        let path = temp_settings_path("missing-settings-with-logs");

        let settings =
            load_settings_from_path_with_cost_probe(&path, path.parent().unwrap(), |_| true)
                .unwrap();

        assert!(settings.cost_usage_enabled);
    }
}
