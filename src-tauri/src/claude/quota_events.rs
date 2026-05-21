use crate::domain::account::AccountSummary;
use crate::domain::usage::{
    ClaudeOverviewSnapshot, QuotaEvent, QuotaEventKind, QuotaEventSeverity, UsageSnapshot,
    UsageWindow,
};

const WARNING_THRESHOLDS: [f64; 3] = [80.0, 90.0, 100.0];
const RESET_USED_PERCENT: f64 = 1.0;

pub fn detect_quota_events(
    previous: Option<&ClaudeOverviewSnapshot>,
    current: &ClaudeOverviewSnapshot,
) -> Vec<QuotaEvent> {
    let mut events = Vec::new();
    for account in &current.accounts {
        let Some(current_usage) = current.usage_by_account_id.get(&account.id) else {
            continue;
        };
        let previous_usage =
            previous.and_then(|snapshot| snapshot.usage_by_account_id.get(&account.id));

        for window_key in ["primary", "secondary", "tertiary"] {
            collect_window_events(
                &mut events,
                account,
                current_usage,
                previous_usage,
                window_key,
                current.generated_at,
            );
        }
    }

    events
}

fn collect_window_events(
    events: &mut Vec<QuotaEvent>,
    account: &AccountSummary,
    current_usage: &UsageSnapshot,
    previous_usage: Option<&UsageSnapshot>,
    window_key: &str,
    generated_at: i64,
) {
    let Some(current_window) = window_by_key(current_usage, window_key) else {
        return;
    };

    let current_used = normalized_percent(current_window.used_percent);

    if let Some(previous_window) = previous_usage.and_then(|usage| window_by_key(usage, window_key))
    {
        let previous_used = normalized_percent(previous_window.used_percent);

        if previous_used > RESET_USED_PERCENT && current_used <= RESET_USED_PERCENT {
            events.push(reset_event(
                account,
                window_key,
                current_window,
                current_used,
                generated_at,
            ));
            return;
        }

        if let Some(threshold) = crossed_warning_threshold(previous_used, current_used) {
            events.push(warning_event(
                account,
                window_key,
                current_window,
                current_used,
                threshold,
                generated_at,
            ));
        }

        return;
    }

    if let Some(threshold) = current_warning_threshold(current_used) {
        events.push(warning_event(
            account,
            window_key,
            current_window,
            current_used,
            threshold,
            generated_at,
        ));
    }
}

fn current_warning_threshold(current_used: f64) -> Option<f64> {
    WARNING_THRESHOLDS
        .iter()
        .rev()
        .copied()
        .find(|threshold| current_used >= *threshold)
}

fn window_by_key<'a>(usage: &'a UsageSnapshot, window_key: &str) -> Option<&'a UsageWindow> {
    match window_key {
        "primary" => usage.primary.as_ref(),
        "secondary" => usage.secondary.as_ref(),
        "tertiary" => usage.tertiary.as_ref(),
        _ => None,
    }
}

fn crossed_warning_threshold(previous_used: f64, current_used: f64) -> Option<f64> {
    WARNING_THRESHOLDS
        .iter()
        .rev()
        .copied()
        .find(|threshold| previous_used < *threshold && current_used >= *threshold)
}

fn warning_event(
    account: &AccountSummary,
    window_key: &str,
    window: &UsageWindow,
    used_percent: f64,
    threshold_percent: f64,
    generated_at: i64,
) -> QuotaEvent {
    let severity = if threshold_percent >= 100.0 {
        QuotaEventSeverity::Critical
    } else {
        QuotaEventSeverity::Warning
    };
    let title = if threshold_percent >= 100.0 {
        "Claude Code quota exhausted".to_string()
    } else {
        format!("Claude Code quota at {:.0}%", threshold_percent)
    };
    let body = format!(
        "{}: {} is {:.0}% used.",
        account.label, window.label, used_percent
    );

    QuotaEvent {
        id: event_id(
            generated_at,
            &account.id,
            window_key,
            QuotaEventKind::Warning,
            Some(threshold_percent),
        ),
        kind: QuotaEventKind::Warning,
        severity,
        account_id: account.id.clone(),
        account_label: account.label.clone(),
        window_key: window_key.to_string(),
        window_label: window.label.clone(),
        used_percent,
        threshold_percent: Some(threshold_percent),
        title,
        body,
        generated_at,
    }
}

fn reset_event(
    account: &AccountSummary,
    window_key: &str,
    window: &UsageWindow,
    used_percent: f64,
    generated_at: i64,
) -> QuotaEvent {
    QuotaEvent {
        id: event_id(
            generated_at,
            &account.id,
            window_key,
            QuotaEventKind::Reset,
            None,
        ),
        kind: QuotaEventKind::Reset,
        severity: QuotaEventSeverity::Info,
        account_id: account.id.clone(),
        account_label: account.label.clone(),
        window_key: window_key.to_string(),
        window_label: window.label.clone(),
        used_percent,
        threshold_percent: None,
        title: "Claude Code quota reset".to_string(),
        body: format!(
            "{}: {} dropped to {:.0}% used.",
            account.label, window.label, used_percent
        ),
        generated_at,
    }
}

fn event_id(
    generated_at: i64,
    account_id: &str,
    window_key: &str,
    kind: QuotaEventKind,
    threshold_percent: Option<f64>,
) -> String {
    let kind = match kind {
        QuotaEventKind::Warning => "warning",
        QuotaEventKind::Reset => "reset",
    };
    let threshold = threshold_percent.unwrap_or_default().round() as i64;
    format!("{generated_at}:{account_id}:{window_key}:{kind}:{threshold}")
}

fn normalized_percent(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        0.0
    }
}
