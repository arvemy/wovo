use crate::codex::login_runner::LoginRunnerState;
use crate::snapshot::{start_codex_snapshot_tasks, CodexSnapshotCoordinator};
use std::sync::Arc;
use tauri::Manager;

#[cfg(desktop)]
const WOVO_LIGHT_WINDOW_ICON: &[u8] = include_bytes!("../icons/wovo-window-light.png");
#[cfg(desktop)]
const WOVO_DARK_WINDOW_ICON: &[u8] = include_bytes!("../icons/wovo-window-dark.png");

pub(crate) fn run() {
    let snapshot_coordinator = Arc::new(CodexSnapshotCoordinator::default());
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(LoginRunnerState::default())
        .manage(snapshot_coordinator.clone())
        .setup(move |app| {
            configure_window_icon(app);
            start_codex_snapshot_tasks(app.handle().clone(), snapshot_coordinator.clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crate::account_commands::add_codex_account,
            crate::account_commands::cancel_codex_account_login,
            crate::snapshot::get_cached_codex_snapshot,
            crate::settings_commands::get_codex_settings,
            crate::account_commands::get_detected_codex_account,
            crate::account_commands::list_codex_accounts,
            crate::account_commands::reauthenticate_codex_account,
            crate::account_commands::remove_codex_account,
            crate::snapshot::refresh_codex_snapshot,
            crate::account_commands::set_system_codex_account,
            crate::settings_commands::set_codex_auto_account_switching_enabled,
            crate::settings_commands::set_codex_auto_switch_threshold_percent,
            crate::settings_commands::set_codex_cost_usage_enabled,
            crate::settings_commands::set_codex_hide_account_credentials,
            crate::settings_commands::set_codex_notifications_enabled,
            crate::settings_commands::set_codex_usage_source_mode,
            crate::settings_commands::set_codex_weekly_penalty_threshold,
            crate::usage_commands::refresh_codex_usage,
            crate::usage_commands::refresh_all_usage
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(desktop)]
fn configure_window_icon<R: tauri::Runtime>(app: &tauri::App<R>) {
    for window in app.webview_windows().values().cloned() {
        configure_webview_window_icon(window);
    }
}

#[cfg(desktop)]
fn configure_webview_window_icon<R: tauri::Runtime>(window: tauri::WebviewWindow<R>) {
    apply_webview_window_icon_for_current_theme(&window);

    let window_for_event = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::ThemeChanged(theme) = event {
            apply_webview_window_icon_for_theme(&window_for_event, *theme);
        }
    });
}

#[cfg(not(desktop))]
fn configure_window_icon<R: tauri::Runtime>(_app: &tauri::App<R>) {}

#[cfg(desktop)]
fn apply_webview_window_icon_for_current_theme<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) {
    let theme = window.theme().unwrap_or(tauri::Theme::Light);
    apply_webview_window_icon_for_theme(window, theme);
}

#[cfg(desktop)]
fn icon_for_theme(theme: tauri::Theme) -> Option<tauri::image::Image<'static>> {
    let icon_bytes = match theme {
        tauri::Theme::Dark => WOVO_DARK_WINDOW_ICON,
        _ => WOVO_LIGHT_WINDOW_ICON,
    };

    tauri::image::Image::from_bytes(icon_bytes).ok()
}

#[cfg(desktop)]
fn apply_webview_window_icon_for_theme<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    theme: tauri::Theme,
) {
    if let Some(icon) = icon_for_theme(theme) {
        let _ = window.set_icon(icon);
    }
}
