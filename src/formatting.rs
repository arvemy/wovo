use crate::codex_api::{QuotaEvent, QuotaEventKind, QuotaEventSeverity};
use wasm_bindgen::prelude::JsValue;

pub(crate) fn is_auth_failure_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("401")
        || message.contains("403")
        || message.contains("unauthorized")
        || message.contains("invalid_grant")
        || message.contains("auth.json was not found")
        || message.contains("does not contain oauth tokens")
}

pub(crate) fn quota_event_kind_label(kind: &QuotaEventKind) -> &'static str {
    match kind {
        QuotaEventKind::Warning => "Warning",
        QuotaEventKind::Reset => "Reset",
    }
}

pub(crate) fn quota_event_class(severity: &QuotaEventSeverity) -> &'static str {
    match severity {
        QuotaEventSeverity::Info => {
            "flex items-start gap-3 rounded-md border border-[var(--success)] bg-[var(--success-muted)] p-3 text-foreground shadow-xs"
        }
        QuotaEventSeverity::Warning => {
            "flex items-start gap-3 rounded-md border border-[var(--warning)] bg-[var(--warning-muted)] p-3 text-[var(--warning-foreground)] shadow-xs"
        }
        QuotaEventSeverity::Critical => {
            "flex items-start gap-3 rounded-md border border-[var(--critical)] bg-[var(--critical-muted)] p-3 text-[var(--critical-foreground)] shadow-xs"
        }
    }
}

pub(crate) fn quota_event_body_suffix(event: &QuotaEvent) -> String {
    match event.kind {
        QuotaEventKind::Warning => format!(
            ": {} is {:.0}% used.",
            event.window_label,
            event.used_percent.clamp(0.0, 100.0)
        ),
        QuotaEventKind::Reset => format!(
            ": {} dropped to {:.0}% used.",
            event.window_label,
            event.used_percent.clamp(0.0, 100.0)
        ),
    }
}

pub(crate) fn quota_event_meta_suffix(event: &QuotaEvent) -> String {
    let threshold = event
        .threshold_percent
        .map(|percent| format!(" - threshold {:.0}%", percent))
        .unwrap_or_default();
    format!(
        " - {} - {:.0}% used{} - {}",
        event.window_label,
        event.used_percent.clamp(0.0, 100.0),
        threshold,
        format_time_ago(event.generated_at)
    )
}

pub(crate) fn usage_meter_fill_class(used_percent: f64) -> &'static str {
    if used_percent >= 100.0 {
        "h-full min-w-0.5 rounded-full bg-[var(--critical)] transition-colors duration-300 ease-in-out"
    } else if used_percent >= 80.0 {
        "h-full min-w-0.5 rounded-full bg-[var(--warning)] transition-colors duration-300 ease-in-out"
    } else {
        "h-full min-w-0.5 rounded-full bg-[var(--success)] transition-colors duration-300 ease-in-out"
    }
}

pub(crate) fn format_cost(value: Option<f64>) -> String {
    match value {
        Some(cost) if cost < 0.005 && cost > 0.0 => format!("${cost:.4}"),
        Some(cost) => format!("${cost:.2}"),
        None => "Unpriced".to_string(),
    }
}

pub(crate) fn format_tokens(value: i64) -> String {
    let value = value.max(0);
    if value >= 1_000_000 {
        format!("{:.1}M tokens", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K tokens", value as f64 / 1_000.0)
    } else {
        format!("{value} tokens")
    }
}

pub(crate) fn utc_day_key(value: i64) -> String {
    let date = js_sys::Date::new(&JsValue::from_f64(value as f64 * 1000.0));
    format!(
        "{:04}-{:02}-{:02}",
        date.get_utc_full_year(),
        date.get_utc_month() + 1,
        date.get_utc_date()
    )
}

pub(crate) fn format_time_ago(value: i64) -> String {
    let now_seconds = js_sys::Date::now() / 1000.0;
    let elapsed = (now_seconds - (value as f64)).max(0.0).round() as i64;

    if elapsed < 5 {
        return "just now".to_string();
    }

    let (amount, unit) = if elapsed < 60 {
        (elapsed, "second")
    } else if elapsed < 3_600 {
        (elapsed / 60, "minute")
    } else if elapsed < 86_400 {
        (elapsed / 3_600, "hour")
    } else if elapsed < 2_592_000 {
        (elapsed / 86_400, "day")
    } else if elapsed < 31_536_000 {
        (elapsed / 2_592_000, "month")
    } else {
        (elapsed / 31_536_000, "year")
    };

    if amount == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{amount} {unit}s ago")
    }
}

pub(crate) fn format_remaining_time(reset_at: i64) -> String {
    let now_seconds = js_sys::Date::now() / 1000.0;
    let remaining = ((reset_at as f64) - now_seconds).max(0.0).round() as i64;

    if remaining <= 0 {
        return "resets now".to_string();
    }

    let days = remaining / 86_400;
    let hours = (remaining % 86_400) / 3_600;
    let minutes = (remaining % 3_600) / 60;

    if days > 0 {
        format!("{days}d {hours}h left")
    } else if hours > 0 {
        format!("{hours}h {minutes}m left")
    } else {
        format!("{minutes}m left")
    }
}
