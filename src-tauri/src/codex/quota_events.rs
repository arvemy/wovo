use crate::domain::account::AccountSummary;
use crate::domain::usage::{
    CodexOverviewSnapshot, QuotaEvent, QuotaEventKind, QuotaEventSeverity, UsageSnapshot,
    UsageWindow,
};

const WARNING_THRESHOLDS: [f64; 3] = [80.0, 90.0, 100.0];
const RESET_USED_PERCENT: f64 = 1.0;

pub fn detect_quota_events(
    previous: Option<&CodexOverviewSnapshot>,
    current: &CodexOverviewSnapshot,
) -> Vec<QuotaEvent> {
    let mut events = Vec::new();
    for account in &current.accounts {
        let Some(current_usage) = current.usage_by_account_id.get(&account.id) else {
            continue;
        };
        let previous_usage =
            previous.and_then(|snapshot| snapshot.usage_by_account_id.get(&account.id));

        collect_window_events(
            &mut events,
            account,
            current_usage,
            previous_usage,
            "primary",
            current.generated_at,
        );
        collect_window_events(
            &mut events,
            account,
            current_usage,
            previous_usage,
            "secondary",
            current.generated_at,
        );
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
        "Codex quota exhausted".to_string()
    } else {
        format!("Codex quota at {:.0}%", threshold_percent)
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
        title: "Codex quota reset".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::AccountSourceKind;
    use crate::domain::usage::{CostUsageSnapshot, CreditsSnapshot};
    use std::collections::HashMap;

    fn account() -> AccountSummary {
        AccountSummary {
            id: "account-1".to_string(),
            label: "user@example.com".to_string(),
            email: Some("user@example.com".to_string()),
            provider_account_id: None,
            workspace_account_id: None,
            workspace_label: None,
            home_path: "/tmp/codex".to_string(),
            source: AccountSourceKind::Managed,
            authenticated: true,
            is_live_system: false,
            can_set_system: true,
            can_remove: true,
            created_at: Some(1),
            updated_at: Some(1),
            last_authenticated_at: Some(1),
        }
    }

    fn usage(primary_used: f64, secondary_used: f64) -> UsageSnapshot {
        UsageSnapshot {
            account_id: "account-1".to_string(),
            source: "oauth".to_string(),
            plan_type: Some("pro".to_string()),
            primary: Some(window("5h limit", primary_used)),
            secondary: Some(window("Weekly limit", secondary_used)),
            tertiary: None,
            credits: Some(CreditsSnapshot {
                balance: None,
                has_credits: false,
                unlimited: false,
            }),
            updated_at: 1,
        }
    }

    fn window(label: &str, used_percent: f64) -> UsageWindow {
        UsageWindow {
            label: label.to_string(),
            used_percent,
            remaining_percent: 100.0 - used_percent,
            reset_at: Some(1_770_000_000),
            window_seconds: Some(18_000),
        }
    }

    fn snapshot(
        primary_used: f64,
        secondary_used: f64,
        generated_at: i64,
    ) -> CodexOverviewSnapshot {
        let account = account();
        let mut usage_by_account_id = HashMap::new();
        usage_by_account_id.insert(account.id.clone(), usage(primary_used, secondary_used));

        CodexOverviewSnapshot {
            accounts: vec![account],
            usage_by_account_id,
            errors_by_account_id: HashMap::new(),
            quota_events: Vec::new(),
            cost_usage: None::<CostUsageSnapshot>,
            cost_error: None,
            generated_at,
            stale: false,
        }
    }

    #[test]
    fn warning_thresholds_emit_on_upward_crossing() {
        for (previous_used, current_used, threshold) in
            [(79.0, 80.0, 80.0), (89.0, 90.0, 90.0), (99.0, 100.0, 100.0)]
        {
            let previous = snapshot(previous_used, 0.0, 1);
            let current = snapshot(current_used, 0.0, 2);

            let events = detect_quota_events(Some(&previous), &current);

            assert_eq!(events.len(), 1);
            assert_eq!(events[0].kind, QuotaEventKind::Warning);
            assert_eq!(events[0].threshold_percent, Some(threshold));
        }
    }

    #[test]
    fn multi_threshold_warning_jump_emits_only_highest_threshold() {
        let previous = snapshot(79.0, 0.0, 1);
        let current = snapshot(100.0, 0.0, 2);

        let events = detect_quota_events(Some(&previous), &current);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].threshold_percent, Some(100.0));
        assert_eq!(events[0].severity, QuotaEventSeverity::Critical);
    }

    #[test]
    fn reset_emits_when_usage_drops_to_zero_or_one_percent() {
        for current_used in [0.0, 1.0] {
            let previous = snapshot(95.0, 0.0, 1);
            let current = snapshot(current_used, 0.0, 2);

            let events = detect_quota_events(Some(&previous), &current);

            assert_eq!(events.len(), 1);
            assert_eq!(events[0].kind, QuotaEventKind::Reset);
            assert_eq!(events[0].severity, QuotaEventSeverity::Info);
        }
    }

    #[test]
    fn reset_at_change_without_usage_drop_does_not_emit_reset() {
        let mut previous = snapshot(95.0, 0.0, 1);
        let mut current = snapshot(96.0, 0.0, 2);
        previous
            .usage_by_account_id
            .get_mut("account-1")
            .unwrap()
            .primary
            .as_mut()
            .unwrap()
            .reset_at = Some(1_770_000_000);
        current
            .usage_by_account_id
            .get_mut("account-1")
            .unwrap()
            .primary
            .as_mut()
            .unwrap()
            .reset_at = Some(1_770_100_000);

        let events = detect_quota_events(Some(&previous), &current);

        assert!(events
            .iter()
            .all(|event| event.kind != QuotaEventKind::Reset));
    }

    #[test]
    fn missing_previous_snapshot_emits_current_warning_once() {
        let current = snapshot(100.0, 0.0, 2);

        let events = detect_quota_events(None, &current);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, QuotaEventKind::Warning);
        assert_eq!(events[0].threshold_percent, Some(100.0));
    }

    #[test]
    fn already_warned_state_does_not_emit_again() {
        let previous = snapshot(82.0, 0.0, 1);
        let current = snapshot(83.0, 0.0, 2);

        let events = detect_quota_events(Some(&previous), &current);

        assert!(events.is_empty());
    }
}
