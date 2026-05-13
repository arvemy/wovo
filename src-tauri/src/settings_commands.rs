use crate::codex::settings::{self, CodexSettings, CodexUsageSourceMode};
use crate::error::AppError;

#[tauri::command]
pub(crate) fn get_codex_settings() -> Result<CodexSettings, AppError> {
    settings::load_settings()
}

#[tauri::command]
pub(crate) fn set_codex_usage_source_mode(
    usage_source_mode: CodexUsageSourceMode,
) -> Result<CodexSettings, AppError> {
    settings::save_usage_source_mode(usage_source_mode)
}

#[tauri::command]
pub(crate) fn set_codex_cost_usage_enabled(enabled: bool) -> Result<CodexSettings, AppError> {
    settings::save_cost_usage_enabled(enabled)
}

#[tauri::command]
pub(crate) fn set_codex_notifications_enabled(enabled: bool) -> Result<CodexSettings, AppError> {
    settings::save_notifications_enabled(enabled)
}

#[tauri::command]
pub(crate) fn set_codex_auto_account_switching_enabled(
    enabled: bool,
) -> Result<CodexSettings, AppError> {
    settings::save_auto_account_switching_enabled(enabled)
}

#[tauri::command]
pub(crate) fn set_codex_hide_account_credentials(enabled: bool) -> Result<CodexSettings, AppError> {
    settings::save_hide_account_credentials(enabled)
}

#[tauri::command]
pub(crate) fn set_codex_auto_switch_threshold_percent(
    value: f64,
) -> Result<CodexSettings, AppError> {
    settings::save_auto_switch_threshold_percent(value)
}

#[tauri::command]
pub(crate) fn set_codex_weekly_penalty_threshold(value: f64) -> Result<CodexSettings, AppError> {
    settings::save_weekly_penalty_threshold(value)
}
