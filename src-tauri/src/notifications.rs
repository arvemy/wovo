use crate::auto_switch::AutoSwitchNotification;
use crate::domain::usage::{QuotaEvent, QuotaEventKind};
use tauri::AppHandle;

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexNotification {
    title: String,
    body: String,
}

pub(crate) fn send_codex_notifications(
    app: &AppHandle,
    events: &[QuotaEvent],
    auto_switch: Option<&AutoSwitchNotification>,
    enabled: bool,
    hide_credentials: bool,
) {
    if !enabled || (events.is_empty() && auto_switch.is_none()) {
        return;
    }

    use tauri_plugin_notification::{NotificationExt, PermissionState};

    let notification = app.notification();
    let permission_granted = match notification.permission_state() {
        Ok(PermissionState::Granted) => true,
        Ok(PermissionState::Prompt | PermissionState::PromptWithRationale) => {
            matches!(
                notification.request_permission(),
                Ok(PermissionState::Granted)
            )
        }
        Ok(PermissionState::Denied) | Err(_) => false,
    };

    if !permission_granted {
        return;
    }

    for event in events {
        let payload = quota_event_notification(event, hide_credentials);
        let _ = notification
            .builder()
            .title(payload.title)
            .body(payload.body)
            .show();
    }

    if let Some(auto_switch) = auto_switch {
        let payload = auto_switch_notification(auto_switch, hide_credentials);
        let _ = notification
            .builder()
            .title(payload.title)
            .body(payload.body)
            .show();
    }
}

fn quota_event_notification(event: &QuotaEvent, hide_credentials: bool) -> CodexNotification {
    if !hide_credentials {
        return CodexNotification {
            title: event.title.clone(),
            body: event.body.clone(),
        };
    }

    let body = match event.kind {
        QuotaEventKind::Warning => format!(
            "A Codex account's {} is {:.0}% used.",
            event.window_label,
            event.used_percent.clamp(0.0, 100.0)
        ),
        QuotaEventKind::Reset => format!(
            "A Codex account's {} dropped to {:.0}% used.",
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
    event: &AutoSwitchNotification,
    hide_credentials: bool,
) -> CodexNotification {
    let body = if hide_credentials {
        format!(
            "Wovo switched to another Codex account because {} reached the {:.0}% auto-switch threshold.",
            event.window_label,
            event.threshold_percent.clamp(0.0, 100.0),
        )
    } else {
        format!(
            "Wovo switched from {} to {} because {} reached the {:.0}% auto-switch threshold. Target account: {:.0}% 5h, {:.0}% weekly remaining.",
            event.current_account_label,
            event.target_account_label,
            event.window_label,
            event.threshold_percent.clamp(0.0, 100.0),
            event.target_primary_remaining,
            event.target_weekly_remaining,
        )
    };

    CodexNotification {
        title: "Codex account switched".to_string(),
        body,
    }
}

#[cfg(test)]
mod tests;
