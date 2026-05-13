use crate::codex::login_runner::LoginRunnerState;
use crate::codex::settings;
use crate::snapshot::{start_codex_snapshot_tasks, CodexSnapshotCoordinator};
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;

#[cfg(desktop)]
const WOVO_LIGHT_WINDOW_ICON: &[u8] = include_bytes!("../icons/wovo-window-light.png");
#[cfg(desktop)]
const WOVO_DARK_WINDOW_ICON: &[u8] = include_bytes!("../icons/wovo-window-dark.png");

pub(crate) fn run() {
    let snapshot_coordinator = Arc::new(CodexSnapshotCoordinator::default());
    let launched_minimized = std::env::args().any(|a| a == "--minimized");
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            handle_single_instance_launch,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .manage(LoginRunnerState::default())
        .manage(snapshot_coordinator.clone())
        .setup(move |app| {
            configure_window_icon(app);
            sync_autostart_state(app.handle());
            if launched_minimized && crate::tray::ensure_tray_visible(app.handle()).is_ok() {
                for window in app.webview_windows().values() {
                    let _ = window.hide();
                }
            }
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
            crate::settings_commands::set_codex_launch_on_login,
            crate::usage_commands::refresh_codex_usage,
            crate::usage_commands::refresh_all_usage
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn handle_single_instance_launch(app: &tauri::AppHandle, args: Vec<String>, _cwd: String) {
    if args.iter().any(|arg| arg == "--minimized") {
        return;
    }
    show_existing_main_window(app);
}

fn show_existing_main_window(app: &tauri::AppHandle) {
    let Some(window) = app
        .get_webview_window("main")
        .or_else(|| app.webview_windows().into_values().next())
    else {
        return;
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

fn sync_autostart_state(app: &tauri::AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    let Ok(settings) = settings::load_settings() else {
        return;
    };
    let autolaunch = app.autolaunch();
    if settings.launch_on_login {
        let _ = autolaunch.enable();
    } else {
        let _ = autolaunch.disable();
    }
}

#[cfg(desktop)]
fn configure_window_icon(app: &tauri::App) {
    for window in app.webview_windows().values().cloned() {
        configure_webview_window_icon(window);
    }
}

#[cfg(desktop)]
fn configure_webview_window_icon(window: tauri::WebviewWindow) {
    apply_webview_window_icon_for_current_theme(&window);

    let window_for_event = window.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::ThemeChanged(theme) => {
            apply_webview_window_icon_for_theme(&window_for_event, *theme);
        }
        tauri::WindowEvent::CloseRequested { api, .. }
            if crate::tray::ensure_tray_visible(window_for_event.app_handle()).is_ok() =>
        {
            api.prevent_close();
            let _ = window_for_event.hide();
        }
        tauri::WindowEvent::Focused(false) if window_for_event.is_minimized().unwrap_or(false) => {
            if crate::tray::ensure_tray_visible(window_for_event.app_handle()).is_ok() {
                let _ = window_for_event.hide();
            } else {
                let _ = window_for_event.show();
                let _ = window_for_event.unminimize();
            }
        }
        _ => {}
    });
}

#[cfg(not(desktop))]
fn configure_window_icon(_app: &tauri::App) {}

#[cfg(desktop)]
fn apply_webview_window_icon_for_current_theme(window: &tauri::WebviewWindow) {
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
fn apply_webview_window_icon_for_theme(window: &tauri::WebviewWindow, theme: tauri::Theme) {
    if let Some(icon) = icon_for_theme(theme) {
        let _ = window.set_icon(icon);
    }
}
