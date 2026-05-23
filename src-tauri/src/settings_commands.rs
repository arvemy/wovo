use crate::claude::settings::{self as claude_settings, ClaudeSettings, ClaudeUsageSourceMode};
use crate::codex::settings::{self, CodexSettings, CodexUsageSourceMode};
use crate::error::AppError;
use crate::notifications::{NotificationSettingsOpenResult, NotificationStatus};
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

#[tauri::command]
pub(crate) fn get_codex_settings() -> Result<CodexSettings, AppError> {
    settings::load_settings()
}

#[tauri::command]
pub(crate) fn get_claude_settings() -> Result<ClaudeSettings, AppError> {
    claude_settings::load_settings()
}

#[tauri::command]
pub(crate) fn set_codex_usage_source_mode(
    app: AppHandle,
    usage_source_mode: CodexUsageSourceMode,
) -> Result<CodexSettings, AppError> {
    let settings = settings::save_usage_source_mode(usage_source_mode)?;
    crate::tray::publish_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_claude_usage_source_mode(
    app: AppHandle,
    usage_source_mode: ClaudeUsageSourceMode,
) -> Result<ClaudeSettings, AppError> {
    let settings = claude_settings::save_usage_source_mode(usage_source_mode)?;
    crate::tray::publish_claude_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_codex_cost_usage_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<CodexSettings, AppError> {
    let settings = settings::save_cost_usage_enabled(enabled)?;
    crate::tray::publish_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_claude_cost_usage_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<ClaudeSettings, AppError> {
    let settings = claude_settings::save_cost_usage_enabled(enabled)?;
    crate::tray::publish_claude_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_codex_notifications_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<CodexSettings, AppError> {
    let settings = settings::save_notifications_enabled(enabled)?;
    crate::tray::publish_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_claude_notifications_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<ClaudeSettings, AppError> {
    let settings = claude_settings::save_notifications_enabled(enabled)?;
    crate::tray::publish_claude_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn get_codex_notification_status(app: AppHandle) -> NotificationStatus {
    crate::notifications::notification_status(&app)
}

#[tauri::command]
pub(crate) async fn send_codex_test_notification(
    app: AppHandle,
) -> Result<NotificationStatus, AppError> {
    if !tauri::is_dev() {
        return Err(AppError::Notification(
            "notification tests are only available in Tauri dev mode".to_string(),
        ));
    }

    Ok(crate::notifications::send_test_notification(&app).await)
}

#[tauri::command]
pub(crate) fn open_notification_settings() -> NotificationSettingsOpenResult {
    crate::notifications::open_notification_settings()
}

#[tauri::command]
pub(crate) fn set_codex_auto_account_switching_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<CodexSettings, AppError> {
    let settings = settings::save_auto_account_switching_enabled(enabled)?;
    crate::tray::publish_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_claude_auto_account_switching_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<ClaudeSettings, AppError> {
    let settings = claude_settings::save_auto_account_switching_enabled(enabled)?;
    crate::tray::publish_claude_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_codex_auto_switch_threshold_percent(
    app: AppHandle,
    threshold: f64,
) -> Result<CodexSettings, AppError> {
    let settings = settings::save_auto_switch_threshold_percent(threshold)?;
    crate::tray::publish_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_claude_auto_switch_threshold_percent(
    app: AppHandle,
    threshold: f64,
) -> Result<ClaudeSettings, AppError> {
    let settings = claude_settings::save_auto_switch_threshold_percent(threshold)?;
    crate::tray::publish_claude_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_codex_weekly_penalty_threshold_percent(
    app: AppHandle,
    threshold: f64,
) -> Result<CodexSettings, AppError> {
    let settings = settings::save_weekly_penalty_threshold_percent(threshold)?;
    crate::tray::publish_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_claude_weekly_penalty_threshold_percent(
    app: AppHandle,
    threshold: f64,
) -> Result<ClaudeSettings, AppError> {
    let settings = claude_settings::save_weekly_penalty_threshold_percent(threshold)?;
    crate::tray::publish_claude_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_codex_cost_usage_range_days(
    app: AppHandle,
    range_days: u16,
) -> Result<CodexSettings, AppError> {
    let settings = settings::save_cost_usage_range_days(range_days)?;
    crate::tray::publish_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_claude_cost_usage_range_days(
    app: AppHandle,
    range_days: u16,
) -> Result<ClaudeSettings, AppError> {
    let settings = claude_settings::save_cost_usage_range_days(range_days)?;
    crate::tray::publish_claude_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_codex_hide_account_credentials(
    app: AppHandle,
    enabled: bool,
) -> Result<CodexSettings, AppError> {
    let settings = settings::save_hide_account_credentials(enabled)?;
    crate::tray::publish_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_claude_hide_account_credentials(
    app: AppHandle,
    enabled: bool,
) -> Result<ClaudeSettings, AppError> {
    let settings = claude_settings::save_hide_account_credentials(enabled)?;
    crate::tray::publish_claude_settings_update(&app, &settings);
    Ok(settings)
}

pub(crate) fn apply_launch_on_login_registration(
    app: &AppHandle,
    enabled: bool,
) -> Result<(), AppError> {
    let autolaunch = app.autolaunch();
    let operation = if enabled { "enable" } else { "disable" };
    let result = if enabled {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    };
    result.map_err(|error| AppError::LaunchOnLogin(format!("failed to {operation}: {error}")))
}

pub(crate) fn save_launch_on_login_with_registration(
    app: &AppHandle,
    previous_enabled: bool,
    enabled: bool,
) -> Result<CodexSettings, AppError> {
    apply_launch_on_login_registration(app, enabled)?;
    match settings::save_launch_on_login(enabled) {
        Ok(settings) => Ok(settings),
        Err(error) => {
            if let Err(rollback_error) = apply_launch_on_login_registration(app, previous_enabled) {
                return Err(AppError::LaunchOnLogin(format!(
                    "failed to save preference ({error}); failed to roll back registration ({rollback_error})"
                )));
            }
            Err(error)
        }
    }
}

#[tauri::command]
pub(crate) fn set_codex_launch_on_login(
    app: AppHandle,
    enabled: bool,
) -> Result<CodexSettings, AppError> {
    let previous_enabled = settings::load_settings()?.launch_on_login;
    let settings = save_launch_on_login_with_registration(&app, previous_enabled, enabled)?;
    crate::tray::publish_settings_update(&app, &settings);
    Ok(settings)
}
