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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSettings {
    pub usage_source_mode: CodexUsageSourceMode,
    pub cost_usage_enabled: bool,
    pub notifications_enabled: bool,
    pub auto_account_switching_enabled: bool,
    pub hide_account_credentials: bool,
    pub auto_switch_threshold_percent: f64,
    pub weekly_penalty_threshold: f64,
    pub launch_on_login: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredCodexSettings {
    #[serde(default)]
    usage_source_mode: Option<CodexUsageSourceMode>,
    #[serde(default)]
    cost_usage_enabled: Option<bool>,
    #[serde(default)]
    notifications_enabled: Option<bool>,
    #[serde(default)]
    auto_account_switching_enabled: Option<bool>,
    #[serde(default)]
    hide_account_credentials: Option<bool>,
    #[serde(default)]
    auto_switch_threshold_percent: Option<f64>,
    #[serde(default)]
    weekly_penalty_threshold: Option<f64>,
    #[serde(default)]
    launch_on_login: Option<bool>,
}

impl Default for CodexSettings {
    fn default() -> Self {
        Self {
            usage_source_mode: CodexUsageSourceMode::Auto,
            cost_usage_enabled: false,
            notifications_enabled: true,
            auto_account_switching_enabled: false,
            hide_account_credentials: false,
            auto_switch_threshold_percent: 90.0,
            weekly_penalty_threshold: 20.0,
            launch_on_login: false,
        }
    }
}

fn clean_percent(value: f64, default: f64, min: f64, max: f64) -> f64 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        default
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

pub fn save_notifications_enabled(enabled: bool) -> Result<CodexSettings, AppError> {
    let mut settings = load_settings()?;
    settings.notifications_enabled = enabled;
    save_settings_to_path(&settings_path(), &settings)?;
    Ok(settings)
}

pub fn save_auto_account_switching_enabled(enabled: bool) -> Result<CodexSettings, AppError> {
    let mut settings = load_settings()?;
    settings.auto_account_switching_enabled = enabled;
    save_settings_to_path(&settings_path(), &settings)?;
    Ok(settings)
}

pub fn save_hide_account_credentials(enabled: bool) -> Result<CodexSettings, AppError> {
    let mut settings = load_settings()?;
    settings.hide_account_credentials = enabled;
    save_settings_to_path(&settings_path(), &settings)?;
    Ok(settings)
}

pub fn save_auto_switch_threshold_percent(value: f64) -> Result<CodexSettings, AppError> {
    let mut settings = load_settings()?;
    settings.auto_switch_threshold_percent = clean_percent(value, 90.0, 50.0, 100.0);
    save_settings_to_path(&settings_path(), &settings)?;
    Ok(settings)
}

pub fn save_weekly_penalty_threshold(value: f64) -> Result<CodexSettings, AppError> {
    let mut settings = load_settings()?;
    settings.weekly_penalty_threshold = clean_percent(value, 20.0, 0.0, 50.0);
    save_settings_to_path(&settings_path(), &settings)?;
    Ok(settings)
}

pub fn save_launch_on_login(enabled: bool) -> Result<CodexSettings, AppError> {
    let mut settings = load_settings()?;
    settings.launch_on_login = enabled;
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
        notifications_enabled: stored.notifications_enabled.unwrap_or(true),
        auto_account_switching_enabled: stored.auto_account_switching_enabled.unwrap_or(false),
        hide_account_credentials: stored.hide_account_credentials.unwrap_or(false),
        auto_switch_threshold_percent: clean_percent(
            stored.auto_switch_threshold_percent.unwrap_or(90.0),
            90.0,
            50.0,
            100.0,
        ),
        weekly_penalty_threshold: clean_percent(
            stored.weekly_penalty_threshold.unwrap_or(20.0),
            20.0,
            0.0,
            50.0,
        ),
        launch_on_login: stored.launch_on_login.unwrap_or(false),
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
        assert!(!settings.auto_account_switching_enabled);
        assert!(!settings.hide_account_credentials);
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
            notifications_enabled: false,
            auto_account_switching_enabled: true,
            hide_account_credentials: true,
            auto_switch_threshold_percent: 80.0,
            weekly_penalty_threshold: 15.0,
            launch_on_login: false,
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
        assert!(!loaded.auto_account_switching_enabled);
        assert!(!loaded.hide_account_credentials);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
