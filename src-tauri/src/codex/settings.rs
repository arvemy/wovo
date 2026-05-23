use crate::codex::account_store::default_wovo_codex_root;
use crate::codex::atomic_file::{replace_file, temporary_file_path, write_new_file};
use crate::codex::auth_store::system_codex_home_path;
use crate::codex::cost_usage;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const SETTINGS_FILE_NAME: &str = "codex-settings.json";
const SETTINGS_SCHEMA_VERSION: u16 = 2;
pub(crate) const DEFAULT_AUTO_SWITCH_THRESHOLD_PERCENT: f64 = 90.0;
pub(crate) const DEFAULT_WEEKLY_PENALTY_THRESHOLD_PERCENT: f64 = 20.0;
pub(crate) const DEFAULT_COST_USAGE_RANGE_DAYS: u16 = 30;
static SETTINGS_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

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
    pub schema_version: u16,
    pub usage_source_mode: CodexUsageSourceMode,
    pub cost_usage_enabled: bool,
    pub notifications_enabled: bool,
    pub auto_account_switching_enabled: bool,
    pub auto_switch_threshold_percent: f64,
    pub weekly_penalty_threshold_percent: f64,
    pub cost_usage_range_days: u16,
    pub hide_account_credentials: bool,
    pub launch_on_login: bool,
    #[serde(default)]
    pub config_warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredCodexSettings {
    #[serde(default)]
    schema_version: Option<u16>,
    #[serde(default)]
    usage_source_mode: Option<CodexUsageSourceMode>,
    #[serde(default)]
    cost_usage_enabled: Option<bool>,
    #[serde(default)]
    notifications_enabled: Option<bool>,
    #[serde(default)]
    auto_account_switching_enabled: Option<bool>,
    #[serde(default)]
    auto_switch_threshold_percent: Option<f64>,
    #[serde(default)]
    weekly_penalty_threshold_percent: Option<f64>,
    #[serde(default)]
    cost_usage_range_days: Option<u16>,
    #[serde(default)]
    hide_account_credentials: Option<bool>,
    #[serde(default)]
    launch_on_login: Option<bool>,
}

impl Default for CodexSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            usage_source_mode: CodexUsageSourceMode::Auto,
            cost_usage_enabled: false,
            notifications_enabled: true,
            auto_account_switching_enabled: false,
            auto_switch_threshold_percent: DEFAULT_AUTO_SWITCH_THRESHOLD_PERCENT,
            weekly_penalty_threshold_percent: DEFAULT_WEEKLY_PENALTY_THRESHOLD_PERCENT,
            cost_usage_range_days: DEFAULT_COST_USAGE_RANGE_DAYS,
            hide_account_credentials: false,
            launch_on_login: false,
            config_warnings: Vec::new(),
        }
    }
}

pub fn load_settings() -> Result<CodexSettings, AppError> {
    load_settings_from_path_with_codex_home(&settings_path(), &system_codex_home_path())
}

pub fn save_usage_source_mode(mode: CodexUsageSourceMode) -> Result<CodexSettings, AppError> {
    update_settings(|settings| {
        settings.usage_source_mode = mode;
    })
}

pub fn save_cost_usage_enabled(enabled: bool) -> Result<CodexSettings, AppError> {
    update_settings(|settings| {
        settings.cost_usage_enabled = enabled;
    })
}

pub fn save_notifications_enabled(enabled: bool) -> Result<CodexSettings, AppError> {
    update_settings(|settings| {
        settings.notifications_enabled = enabled;
    })
}

pub fn save_auto_account_switching_enabled(enabled: bool) -> Result<CodexSettings, AppError> {
    update_settings(|settings| {
        settings.auto_account_switching_enabled = enabled;
    })
}

pub fn save_auto_switch_threshold_percent(threshold: f64) -> Result<CodexSettings, AppError> {
    update_settings(|settings| {
        settings.auto_switch_threshold_percent =
            normalize_percent_threshold(threshold, DEFAULT_AUTO_SWITCH_THRESHOLD_PERCENT);
    })
}

pub fn save_weekly_penalty_threshold_percent(threshold: f64) -> Result<CodexSettings, AppError> {
    update_settings(|settings| {
        settings.weekly_penalty_threshold_percent =
            normalize_percent_threshold(threshold, DEFAULT_WEEKLY_PENALTY_THRESHOLD_PERCENT);
    })
}

pub fn save_cost_usage_range_days(range_days: u16) -> Result<CodexSettings, AppError> {
    update_settings(|settings| {
        settings.cost_usage_range_days = normalize_cost_range_days(range_days);
    })
}

pub fn save_hide_account_credentials(enabled: bool) -> Result<CodexSettings, AppError> {
    update_settings(|settings| {
        settings.hide_account_credentials = enabled;
    })
}

pub fn save_launch_on_login(enabled: bool) -> Result<CodexSettings, AppError> {
    update_settings(|settings| {
        settings.launch_on_login = enabled;
    })
}

fn settings_path() -> PathBuf {
    default_wovo_codex_root().join(SETTINGS_FILE_NAME)
}

fn update_settings(update: impl FnOnce(&mut CodexSettings)) -> Result<CodexSettings, AppError> {
    update_settings_at_path(&settings_path(), &system_codex_home_path(), update)
}

fn update_settings_at_path(
    path: &Path,
    codex_home: &Path,
    update: impl FnOnce(&mut CodexSettings),
) -> Result<CodexSettings, AppError> {
    let _guard = settings_write_lock()
        .lock()
        .map_err(|_| AppError::AccountStore("settings write lock was poisoned".to_string()))?;
    let mut settings = load_settings_from_path_with_codex_home(path, codex_home)?;
    update(&mut settings);
    save_settings_to_path(path, &settings)?;
    Ok(settings)
}

fn settings_write_lock() -> &'static Mutex<()> {
    SETTINGS_WRITE_LOCK.get_or_init(|| Mutex::new(()))
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

    let stored: StoredCodexSettings = match serde_json::from_str(&contents) {
        Ok(stored) => stored,
        Err(error) => {
            backup_malformed_settings(path)?;
            return Ok(CodexSettings {
                cost_usage_enabled: cost_usage::local_codex_logs_exist(codex_home),
                config_warnings: vec![format!(
                    "Codex settings were malformed and have been reset to defaults: {error}"
                )],
                ..CodexSettings::default()
            });
        }
    };
    if let Some(stored_version) = stored.schema_version {
        if stored_version > SETTINGS_SCHEMA_VERSION {
            backup_malformed_settings(path)?;
            return Ok(CodexSettings {
                cost_usage_enabled: cost_usage::local_codex_logs_exist(codex_home),
                config_warnings: vec![format!(
                    "Codex settings schema {stored_version} is newer than this build supports ({SETTINGS_SCHEMA_VERSION}); resetting to defaults."
                )],
                ..CodexSettings::default()
            });
        }
    }
    Ok(CodexSettings {
        schema_version: stored.schema_version.unwrap_or(SETTINGS_SCHEMA_VERSION),
        usage_source_mode: stored
            .usage_source_mode
            .unwrap_or(CodexUsageSourceMode::Auto),
        cost_usage_enabled: stored
            .cost_usage_enabled
            .unwrap_or_else(|| cost_usage::local_codex_logs_exist(codex_home)),
        notifications_enabled: stored.notifications_enabled.unwrap_or(true),
        auto_account_switching_enabled: stored.auto_account_switching_enabled.unwrap_or(false),
        auto_switch_threshold_percent: normalize_percent_threshold(
            stored
                .auto_switch_threshold_percent
                .unwrap_or(DEFAULT_AUTO_SWITCH_THRESHOLD_PERCENT),
            DEFAULT_AUTO_SWITCH_THRESHOLD_PERCENT,
        ),
        weekly_penalty_threshold_percent: normalize_percent_threshold(
            stored
                .weekly_penalty_threshold_percent
                .unwrap_or(DEFAULT_WEEKLY_PENALTY_THRESHOLD_PERCENT),
            DEFAULT_WEEKLY_PENALTY_THRESHOLD_PERCENT,
        ),
        cost_usage_range_days: normalize_cost_range_days(
            stored
                .cost_usage_range_days
                .unwrap_or(DEFAULT_COST_USAGE_RANGE_DAYS),
        ),
        hide_account_credentials: stored.hide_account_credentials.unwrap_or(false),
        launch_on_login: stored.launch_on_login.unwrap_or(false),
        config_warnings: Vec::new(),
    })
}

fn backup_malformed_settings(path: &Path) -> Result<(), AppError> {
    if !path.exists() {
        return Ok(());
    }
    let timestamp = time::OffsetDateTime::now_utc().unix_timestamp();
    let backup = path.with_extension(format!("json.bad-{timestamp}"));
    fs::copy(path, backup)
        .map(|_| ())
        .map_err(|error| AppError::AccountStore(error.to_string()))
}

pub(crate) fn normalize_percent_threshold(value: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        fallback
    }
}

pub(crate) fn normalize_cost_range_days(value: u16) -> u16 {
    match value {
        7 | 30 | 90 => value,
        _ => DEFAULT_COST_USAGE_RANGE_DAYS,
    }
}

fn save_settings_to_path(path: &Path, settings: &CodexSettings) -> Result<(), AppError> {
    let contents = serde_json::to_string_pretty(settings)
        .map_err(|error| AppError::AccountStore(error.to_string()))?;
    write_settings_file(path, contents.as_bytes())
}

fn write_settings_file(path: &Path, contents: &[u8]) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(|| {
        AppError::AccountStore(format!(
            "settings path has no parent: {}",
            path.to_string_lossy()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| AppError::AccountStore(error.to_string()))?;

    let tmp = temporary_file_path(parent, SETTINGS_FILE_NAME);
    write_new_file(&tmp, contents).map_err(|error| AppError::AccountStore(error.to_string()))?;
    if let Err(error) = apply_secure_file_permissions(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    replace_file(&tmp, path).map_err(|error| AppError::AccountStore(error.to_string()))
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
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::Duration;
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
            schema_version: SETTINGS_SCHEMA_VERSION,
            usage_source_mode: CodexUsageSourceMode::Cli,
            cost_usage_enabled: true,
            notifications_enabled: false,
            auto_account_switching_enabled: true,
            auto_switch_threshold_percent: DEFAULT_AUTO_SWITCH_THRESHOLD_PERCENT,
            weekly_penalty_threshold_percent: DEFAULT_WEEKLY_PENALTY_THRESHOLD_PERCENT,
            cost_usage_range_days: DEFAULT_COST_USAGE_RANGE_DAYS,
            hide_account_credentials: true,
            launch_on_login: false,
            config_warnings: Vec::new(),
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

    #[test]
    fn schema_version_from_the_future_is_backed_up_and_reset() {
        let path = temp_settings_path("future-schema");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            format!(
                r#"{{"schemaVersion":{},"usageSourceMode":"oauth"}}"#,
                SETTINGS_SCHEMA_VERSION + 7
            ),
        )
        .unwrap();

        let loaded =
            load_settings_from_path_with_codex_home(&path, path.parent().unwrap()).unwrap();

        assert_eq!(loaded.schema_version, SETTINGS_SCHEMA_VERSION);
        assert_eq!(loaded.usage_source_mode, CodexUsageSourceMode::Auto);
        assert_eq!(loaded.config_warnings.len(), 1);
        assert!(loaded.config_warnings[0].contains("newer than this build supports"));

        let parent = path.parent().unwrap();
        let backup_present = fs::read_dir(parent).unwrap().any(|entry| {
            entry
                .map(|entry| {
                    entry
                        .path()
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(|ext| ext.starts_with("bad-"))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        });
        assert!(backup_present, "expected a .bad-<ts> backup file");
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn concurrent_settings_updates_preserve_independent_fields() {
        let path = temp_settings_path("concurrent-settings");
        let codex_home = path.parent().unwrap().join("codex-home");
        save_settings_to_path(&path, &CodexSettings::default()).unwrap();

        let first_update_started = Arc::new(AtomicBool::new(false));
        std::thread::scope(|scope| {
            let marker = first_update_started.clone();
            let first_path = path.clone();
            let first_codex_home = codex_home.clone();
            let first = scope.spawn(move || {
                update_settings_at_path(&first_path, &first_codex_home, |settings| {
                    marker.store(true, Ordering::Release);
                    std::thread::sleep(Duration::from_millis(50));
                    settings.notifications_enabled = false;
                })
                .unwrap();
            });

            while !first_update_started.load(Ordering::Acquire) {
                std::thread::yield_now();
            }

            let second = scope.spawn(|| {
                update_settings_at_path(&path, &codex_home, |settings| {
                    settings.hide_account_credentials = true;
                })
                .unwrap();
            });

            first.join().unwrap();
            second.join().unwrap();
        });

        let loaded =
            load_settings_from_path_with_codex_home(&path, path.parent().unwrap()).unwrap();
        assert!(!loaded.notifications_enabled);
        assert!(loaded.hide_account_credentials);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
