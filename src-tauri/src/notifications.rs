use crate::auto_switch::AutoSwitchNotification;
use crate::domain::usage::{QuotaEvent, QuotaEventKind};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

#[cfg(target_os = "linux")]
const LINUX_NOTIFICATION_SETTINGS_COMMAND: &str = "gnome-control-center";

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexNotification {
    title: String,
    body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotificationMode {
    Normal,
    Test,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotificationDiagnostics {
    pub(crate) last_attempt_at: Option<i64>,
    pub(crate) last_status: Option<String>,
    pub(crate) last_error: Option<String>,
    pub(crate) last_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotificationStatus {
    pub(crate) diagnostics: NotificationDiagnostics,
    pub(crate) test_available: bool,
    pub(crate) permission_state: NotificationPermissionState,
    pub(crate) rationale_required: bool,
    pub(crate) settings_action_available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum NotificationPermissionState {
    Unknown,
    Granted,
    Prompt,
    Denied,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotificationSettingsOpenResult {
    pub(crate) opened: bool,
    pub(crate) user_message: String,
}

#[derive(Default)]
pub(crate) struct NotificationDiagnosticsState {
    latest: Mutex<NotificationDiagnostics>,
}

impl NotificationDiagnosticsState {
    fn current(&self) -> NotificationDiagnostics {
        match self.latest.lock() {
            Ok(latest) => latest.clone(),
            Err(poisoned) => {
                eprintln!("notification diagnostics mutex was poisoned; recovering current value");
                poisoned.into_inner().clone()
            }
        }
    }

    fn record(&self, diagnostics: NotificationDiagnostics) {
        match self.latest.lock() {
            Ok(mut latest) => {
                *latest = diagnostics;
            }
            Err(poisoned) => {
                eprintln!("notification diagnostics mutex was poisoned; recovering record path");
                *poisoned.into_inner() = diagnostics;
            }
        }
    }
}

pub(crate) fn notification_status(app: &AppHandle) -> NotificationStatus {
    let permission_state = notification_permission_state(app);
    NotificationStatus {
        diagnostics: current_diagnostics(app),
        test_available: tauri::is_dev(),
        permission_state,
        rationale_required: permission_state == NotificationPermissionState::Prompt,
        settings_action_available: notification_settings_action_available(),
    }
}

pub(crate) async fn send_test_notification(app: &AppHandle) -> NotificationStatus {
    let surface = prepare_test_notification_surface(app).await;
    let diagnostics = show_notification_with_mode(
        app,
        "WoVo notification test".to_string(),
        "Native notifications are reachable from this Tauri dev session.".to_string(),
        NotificationMode::Test,
    );
    restore_test_notification_surface(surface);
    NotificationStatus {
        diagnostics,
        test_available: tauri::is_dev(),
        permission_state: notification_permission_state(app),
        rationale_required: false,
        settings_action_available: notification_settings_action_available(),
    }
}

pub(crate) fn open_notification_settings() -> NotificationSettingsOpenResult {
    platform_open_notification_settings()
}

#[cfg(target_os = "linux")]
struct TestNotificationSurface {
    restore_windows: Vec<TestNotificationWindow>,
}

#[cfg(target_os = "linux")]
struct TestNotificationWindow {
    window: tauri::WebviewWindow,
    was_focused: bool,
}

#[cfg(target_os = "linux")]
async fn prepare_test_notification_surface(app: &AppHandle) -> TestNotificationSurface {
    let mut restore_windows = Vec::new();
    for window in app.webview_windows().into_values() {
        let was_visible = window.is_visible().unwrap_or(true);
        let was_minimized = window.is_minimized().unwrap_or(false);
        let was_focused = window.is_focused().unwrap_or(false);

        if was_visible && !was_minimized && window.minimize().is_ok() {
            restore_windows.push(TestNotificationWindow {
                window,
                was_focused,
            });
        }
    }
    tokio::time::sleep(std::time::Duration::from_millis(900)).await;
    TestNotificationSurface { restore_windows }
}

#[cfg(target_os = "linux")]
fn restore_test_notification_surface(surface: TestNotificationSurface) {
    let mut focused_window = None;

    for entry in surface.restore_windows {
        let _ = entry.window.show();
        let _ = entry.window.unminimize();
        if entry.was_focused {
            focused_window = Some(entry.window);
        }
    }

    if let Some(window) = focused_window {
        let _ = window.set_focus();
    }
}

#[cfg(not(target_os = "linux"))]
struct TestNotificationSurface;

#[cfg(not(target_os = "linux"))]
async fn prepare_test_notification_surface(_app: &AppHandle) -> TestNotificationSurface {
    TestNotificationSurface
}

#[cfg(not(target_os = "linux"))]
fn restore_test_notification_surface(_surface: TestNotificationSurface) {}

#[cfg(target_os = "linux")]
fn platform_show_notification(
    _app: &AppHandle,
    title: &str,
    body: &str,
    mode: NotificationMode,
) -> Result<(), String> {
    use notify_rust::{Hint, Notification, Timeout, Urgency};

    let mut notification = Notification::new();
    notification
        .appname("wovo")
        .summary(title)
        .body(body)
        .icon("wovo");

    notification
        .hint(Hint::DesktopEntry("wovo".to_string()))
        .timeout(match mode {
            NotificationMode::Normal => Timeout::Milliseconds(5_000),
            NotificationMode::Test => Timeout::Never,
        });

    if mode == NotificationMode::Test {
        notification.urgency(Urgency::Critical);
    }

    notification
        .show()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(not(target_os = "linux"))]
fn platform_show_notification(
    app: &AppHandle,
    title: &str,
    body: &str,
    _mode: NotificationMode,
) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;

    app.notification()
        .builder()
        .title(title.to_string())
        .body(body.to_string())
        .show()
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
fn notification_success_status() -> String {
    "sent".to_string()
}

#[cfg(not(target_os = "linux"))]
fn notification_success_status() -> String {
    "queued".to_string()
}

pub(crate) fn send_codex_notifications(
    app: &AppHandle,
    events: &[QuotaEvent],
    auto_switch: Option<&AutoSwitchNotification>,
    enabled: bool,
    hide_credentials: bool,
) {
    send_provider_notifications(app, "Codex", events, auto_switch, enabled, hide_credentials)
}

pub(crate) fn send_claude_notifications(
    app: &AppHandle,
    events: &[QuotaEvent],
    auto_switch: Option<&AutoSwitchNotification>,
    enabled: bool,
    hide_credentials: bool,
) {
    send_provider_notifications(
        app,
        "Claude Code",
        events,
        auto_switch,
        enabled,
        hide_credentials,
    )
}

fn send_provider_notifications(
    app: &AppHandle,
    provider_label: &str,
    events: &[QuotaEvent],
    auto_switch: Option<&AutoSwitchNotification>,
    enabled: bool,
    hide_credentials: bool,
) {
    if !enabled || (events.is_empty() && auto_switch.is_none()) {
        return;
    }

    for event in events {
        let payload = quota_event_notification(provider_label, event, hide_credentials);
        let _ = show_notification(app, payload.title, payload.body);
    }

    if let Some(auto_switch) = auto_switch {
        let payload = auto_switch_notification(provider_label, auto_switch, hide_credentials);
        let _ = show_notification(app, payload.title, payload.body);
    }
}

fn show_notification(app: &AppHandle, title: String, body: String) -> NotificationDiagnostics {
    show_notification_with_mode(app, title, body, NotificationMode::Normal)
}

fn show_notification_with_mode(
    app: &AppHandle,
    title: String,
    body: String,
    mode: NotificationMode,
) -> NotificationDiagnostics {
    use tauri_plugin_notification::{NotificationExt, PermissionState};

    let attempted_at = time::OffsetDateTime::now_utc().unix_timestamp();
    if requires_tauri_permission_check(mode) {
        let notification = app.notification();
        let permission_granted = match notification.permission_state() {
            Ok(PermissionState::Granted) => true,
            Ok(PermissionState::Prompt | PermissionState::PromptWithRationale) => {
                matches!(
                    notification.request_permission(),
                    Ok(PermissionState::Granted)
                )
            }
            Ok(PermissionState::Denied) => false,
            Err(error) => {
                let diagnostics = NotificationDiagnostics {
                    last_attempt_at: Some(attempted_at),
                    last_status: Some("failed".to_string()),
                    last_error: Some(format!("permission check failed: {error}")),
                    last_title: Some(title),
                };
                record_diagnostics(app, diagnostics.clone());
                return diagnostics;
            }
        };

        if !permission_granted {
            let diagnostics = NotificationDiagnostics {
                last_attempt_at: Some(attempted_at),
                last_status: Some("permissionDenied".to_string()),
                last_error: Some("notification permission was denied".to_string()),
                last_title: Some(title),
            };
            record_diagnostics(app, diagnostics.clone());
            return diagnostics;
        }
    }

    let result = platform_show_notification(app, &title, &body, mode);
    let diagnostics = match result {
        Ok(()) => NotificationDiagnostics {
            last_attempt_at: Some(attempted_at),
            last_status: Some(notification_success_status()),
            last_error: None,
            last_title: Some(title),
        },
        Err(error) => NotificationDiagnostics {
            last_attempt_at: Some(attempted_at),
            last_status: Some("failed".to_string()),
            last_error: Some(error.to_string()),
            last_title: Some(title),
        },
    };
    record_diagnostics(app, diagnostics.clone());
    diagnostics
}

fn notification_permission_state(app: &AppHandle) -> NotificationPermissionState {
    if !requires_tauri_permission_check(NotificationMode::Normal) {
        return NotificationPermissionState::Unsupported;
    }

    use tauri_plugin_notification::{NotificationExt, PermissionState};
    match app.notification().permission_state() {
        Ok(PermissionState::Granted) => NotificationPermissionState::Granted,
        Ok(PermissionState::Prompt | PermissionState::PromptWithRationale) => {
            NotificationPermissionState::Prompt
        }
        Ok(PermissionState::Denied) => NotificationPermissionState::Denied,
        Err(error) => {
            eprintln!("notification permission state could not be read: {error}");
            NotificationPermissionState::Unknown
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn notification_settings_action_available() -> bool {
    true
}

#[cfg(target_os = "linux")]
fn notification_settings_action_available() -> bool {
    command_available(LINUX_NOTIFICATION_SETTINGS_COMMAND)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn notification_settings_action_available() -> bool {
    false
}

#[cfg(target_os = "linux")]
fn command_available(command: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };

    command_available_in_paths(command, std::env::split_paths(&paths))
}

#[cfg(target_os = "linux")]
fn command_available_in_paths(
    command: &str,
    paths: impl IntoIterator<Item = std::path::PathBuf>,
) -> bool {
    paths.into_iter().any(|directory| {
        let candidate = directory.join(command);
        std::fs::metadata(candidate)
            .map(|metadata| {
                use std::os::unix::fs::PermissionsExt;

                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
            .unwrap_or(false)
    })
}

#[cfg(target_os = "windows")]
fn platform_open_notification_settings() -> NotificationSettingsOpenResult {
    let opened = Command::new("cmd")
        .args(["/C", "start", "", "ms-settings:notifications"])
        .spawn()
        .is_ok();
    notification_settings_result(opened)
}

#[cfg(target_os = "macos")]
fn platform_open_notification_settings() -> NotificationSettingsOpenResult {
    let opened = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.notifications")
        .spawn()
        .is_ok();
    notification_settings_result(opened)
}

#[cfg(target_os = "linux")]
fn platform_open_notification_settings() -> NotificationSettingsOpenResult {
    let opened = Command::new(LINUX_NOTIFICATION_SETTINGS_COMMAND)
        .arg("notifications")
        .spawn()
        .is_ok();
    notification_settings_result(opened)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_open_notification_settings() -> NotificationSettingsOpenResult {
    NotificationSettingsOpenResult {
        opened: false,
        user_message: "Open your system notification settings manually.".to_string(),
    }
}

fn notification_settings_result(opened: bool) -> NotificationSettingsOpenResult {
    NotificationSettingsOpenResult {
        opened,
        user_message: if opened {
            "Opened system notification settings.".to_string()
        } else {
            "Open your system notification settings manually.".to_string()
        },
    }
}

#[cfg(target_os = "linux")]
fn requires_tauri_permission_check(mode: NotificationMode) -> bool {
    mode != NotificationMode::Test
}

#[cfg(not(target_os = "linux"))]
fn requires_tauri_permission_check(_mode: NotificationMode) -> bool {
    true
}

fn current_diagnostics(app: &AppHandle) -> NotificationDiagnostics {
    app.try_state::<NotificationDiagnosticsState>()
        .map(|state| state.current())
        .unwrap_or_default()
}

fn record_diagnostics(app: &AppHandle, diagnostics: NotificationDiagnostics) {
    if let Some(state) = app.try_state::<NotificationDiagnosticsState>() {
        state.record(diagnostics);
    }
}

fn quota_event_notification(
    provider_label: &str,
    event: &QuotaEvent,
    hide_credentials: bool,
) -> CodexNotification {
    if !hide_credentials {
        return CodexNotification {
            title: event.title.clone(),
            body: event.body.clone(),
        };
    }

    let body = match event.kind {
        QuotaEventKind::Warning => format!(
            "A {provider_label} account's {} is {:.0}% used.",
            event.window_label,
            event.used_percent.clamp(0.0, 100.0)
        ),
        QuotaEventKind::Reset => format!(
            "A {provider_label} account's {} dropped to {:.0}% used.",
            event.window_label,
            event.used_percent.clamp(0.0, 100.0)
        ),
    };

    CodexNotification {
        title: event.title.clone(),
        body,
    }
}

fn auto_switch_notification(
    provider_label: &str,
    event: &AutoSwitchNotification,
    hide_credentials: bool,
) -> CodexNotification {
    let body = if hide_credentials {
        format!(
            "Wovo switched to another {provider_label} account because {} reached the {:.0}% auto-switch threshold.",
            event.window_label,
            event.threshold_percent.clamp(0.0, 100.0),
        )
    } else {
        let target_remaining = match event.target_primary_remaining {
            Some(primary_remaining) => format!(
                "Target account: {:.0}% 5h, {:.0}% weekly remaining.",
                primary_remaining, event.target_weekly_remaining,
            ),
            None => format!(
                "Target account: {:.0}% weekly remaining.",
                event.target_weekly_remaining,
            ),
        };
        format!(
            "Wovo switched from {} to {} because {} reached the {:.0}% auto-switch threshold. {}",
            event.current_account_label,
            event.target_account_label,
            event.window_label,
            event.threshold_percent.clamp(0.0, 100.0),
            target_remaining,
        )
    };

    CodexNotification {
        title: format!("{provider_label} account switched"),
        body,
    }
}

#[cfg(test)]
mod tests;
