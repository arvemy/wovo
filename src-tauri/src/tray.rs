use crate::codex::settings::{self, CodexSettings};
use crate::snapshot::CodexSnapshotCoordinator;
use std::sync::Arc;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

#[cfg(desktop)]
const WOVO_TRAY_ICON: &[u8] = include_bytes!("../icons/icon.png");

const ID_SHOW_HIDE: &str = "tray_show_hide";
const ID_REFRESH: &str = "tray_refresh_accounts";
const ID_NOTIFICATIONS: &str = "tray_toggle_notifications";
const ID_AUTO_SWITCH: &str = "tray_toggle_auto_switch";
const ID_COST_TRACKER: &str = "tray_toggle_cost_tracker";
const ID_HIDE_CREDENTIALS: &str = "tray_toggle_hide_credentials";
const ID_LAUNCH_ON_LOGIN: &str = "tray_toggle_launch_on_login";
const ID_QUIT: &str = "tray_quit";
const SETTINGS_EVENT: &str = "codex:settings-updated";

pub(crate) struct TrayMenuState {
    notifications: CheckMenuItem<Wry>,
    auto_switch: CheckMenuItem<Wry>,
    cost_tracker: CheckMenuItem<Wry>,
    hide_credentials: CheckMenuItem<Wry>,
    launch_on_login: CheckMenuItem<Wry>,
}

/// Creates and shows the system tray the first time it is called.
/// Subsequent calls are no-ops if the tray already exists.
/// Call this when the main window is hidden (minimize or close).
#[cfg(desktop)]
pub(crate) fn ensure_tray_visible(app: &AppHandle) -> tauri::Result<()> {
    if app.tray_by_id("wovo").is_some() {
        return Ok(());
    }
    create_tray(app)
}

#[cfg(not(desktop))]
pub(crate) fn ensure_tray_visible(_app: &AppHandle) -> tauri::Result<()> {
    Ok(())
}

#[cfg(desktop)]
fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let settings = settings::load_settings().unwrap_or_default();
    let show_hide = MenuItem::with_id(app, ID_SHOW_HIDE, "Show/Hide Wovo", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, ID_REFRESH, "Refresh Accounts", true, None::<&str>)?;
    let notifications = CheckMenuItem::with_id(
        app,
        ID_NOTIFICATIONS,
        "Toggle Notifications",
        true,
        settings.notifications_enabled,
        None::<&str>,
    )?;
    let auto_switch = CheckMenuItem::with_id(
        app,
        ID_AUTO_SWITCH,
        "Auto-Switch",
        true,
        settings.auto_account_switching_enabled,
        None::<&str>,
    )?;
    let cost_tracker = CheckMenuItem::with_id(
        app,
        ID_COST_TRACKER,
        "Cost Tracker",
        true,
        settings.cost_usage_enabled,
        None::<&str>,
    )?;
    let hide_credentials = CheckMenuItem::with_id(
        app,
        ID_HIDE_CREDENTIALS,
        "Hide Credentials",
        true,
        settings.hide_account_credentials,
        None::<&str>,
    )?;
    let launch_on_login = CheckMenuItem::with_id(
        app,
        ID_LAUNCH_ON_LOGIN,
        "Launch at Login",
        true,
        settings.launch_on_login,
        None::<&str>,
    )?;
    let separator_top = PredefinedMenuItem::separator(app)?;
    let separator_bottom = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, ID_QUIT, "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &show_hide,
            &refresh,
            &separator_top,
            &notifications,
            &auto_switch,
            &cost_tracker,
            &hide_credentials,
            &launch_on_login,
            &separator_bottom,
            &quit,
        ],
    )?;

    let icon = tauri::image::Image::from_bytes(WOVO_TRAY_ICON)?;
    TrayIconBuilder::with_id("wovo")
        .icon(icon)
        .tooltip("Wovo")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .on_menu_event(handle_menu_event)
        .build(app)?;

    app.manage(TrayMenuState {
        notifications,
        auto_switch,
        cost_tracker,
        hide_credentials,
        launch_on_login,
    });

    Ok(())
}

pub(crate) fn publish_settings_update(app: &AppHandle, settings: &CodexSettings) {
    sync_tray_settings(app, settings);
    let _ = app.emit(SETTINGS_EVENT, settings);
}

fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        ID_SHOW_HIDE => toggle_main_window(app),
        ID_REFRESH => refresh_accounts(app),
        ID_NOTIFICATIONS => update_bool_setting(app, |settings| {
            settings::save_notifications_enabled(!settings.notifications_enabled)
        }),
        ID_AUTO_SWITCH => {
            update_bool_setting(app, |settings| {
                settings::save_auto_account_switching_enabled(
                    !settings.auto_account_switching_enabled,
                )
            });
            refresh_accounts(app);
        }
        ID_COST_TRACKER => {
            update_bool_setting(app, |settings| {
                settings::save_cost_usage_enabled(!settings.cost_usage_enabled)
            });
            refresh_accounts(app);
        }
        ID_HIDE_CREDENTIALS => update_bool_setting(app, |settings| {
            settings::save_hide_account_credentials(!settings.hide_account_credentials)
        }),
        ID_LAUNCH_ON_LOGIN => update_bool_setting(app, |settings| {
            let enabled = !settings.launch_on_login;
            crate::settings_commands::save_launch_on_login_with_registration(
                app,
                settings.launch_on_login,
                enabled,
            )
        }),
        ID_QUIT => app.exit(0),
        _ => {}
    }
}

fn update_bool_setting(
    app: &AppHandle,
    save: impl FnOnce(CodexSettings) -> Result<CodexSettings, crate::error::AppError>,
) {
    let Ok(current) = settings::load_settings() else {
        return;
    };
    match save(current.clone()) {
        Ok(settings) => publish_settings_update(app, &settings),
        Err(_) => sync_tray_settings(app, &current),
    }
}

fn refresh_accounts(app: &AppHandle) {
    let coordinator = app.state::<Arc<CodexSnapshotCoordinator>>().inner().clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = coordinator.refresh_manual(app, true).await;
    });
}

fn sync_tray_settings(app: &AppHandle, settings: &CodexSettings) {
    if let Some(state) = app.try_state::<TrayMenuState>() {
        let _ = state
            .notifications
            .set_checked(settings.notifications_enabled);
        let _ = state
            .auto_switch
            .set_checked(settings.auto_account_switching_enabled);
        let _ = state.cost_tracker.set_checked(settings.cost_usage_enabled);
        let _ = state
            .hide_credentials
            .set_checked(settings.hide_account_credentials);
        let _ = state.launch_on_login.set_checked(settings.launch_on_login);
    }
}

fn toggle_main_window(app: &AppHandle) {
    let Some(window) = main_window(app) else {
        return;
    };
    match window.is_visible() {
        Ok(true) => {
            let _ = window.hide();
        }
        _ => show_main_window(app),
    }
}

fn show_main_window(app: &AppHandle) {
    let Some(window) = main_window(app) else {
        return;
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

fn main_window(app: &AppHandle) -> Option<tauri::WebviewWindow<Wry>> {
    app.get_webview_window("main")
        .or_else(|| app.webview_windows().into_values().next())
}
