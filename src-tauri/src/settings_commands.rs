use crate::codex::settings::{self, CodexSettings, CodexUsageSourceMode};
use crate::error::AppError;
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

#[tauri::command]
pub(crate) fn get_codex_settings() -> Result<CodexSettings, AppError> {
    settings::load_settings()
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
pub(crate) fn set_codex_cost_usage_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<CodexSettings, AppError> {
    let settings = settings::save_cost_usage_enabled(enabled)?;
    crate::tray::publish_settings_update(&app, &settings);
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
pub(crate) fn set_codex_auto_account_switching_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<CodexSettings, AppError> {
    let settings = settings::save_auto_account_switching_enabled(enabled)?;
    crate::tray::publish_settings_update(&app, &settings);
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
pub(crate) fn set_codex_auto_switch_threshold_percent(
    app: AppHandle,
    value: f64,
) -> Result<CodexSettings, AppError> {
    let settings = settings::save_auto_switch_threshold_percent(value)?;
    crate::tray::publish_settings_update(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_codex_weekly_penalty_threshold(
    app: AppHandle,
    value: f64,
) -> Result<CodexSettings, AppError> {
    let settings = settings::save_weekly_penalty_threshold(value)?;
    crate::tray::publish_settings_update(&app, &settings);
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
