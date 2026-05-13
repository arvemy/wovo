use super::*;

#[test]
fn credential_hiding_redacts_system_notification_text() {
    let event = QuotaEvent {
        id: "event".to_string(),
        kind: QuotaEventKind::Warning,
        severity: crate::domain::usage::QuotaEventSeverity::Critical,
        account_id: "account-1".to_string(),
        account_label: "user@example.com".to_string(),
        window_key: "primary".to_string(),
        window_label: "5h limit".to_string(),
        used_percent: 100.0,
        threshold_percent: Some(100.0),
        title: "Codex quota exhausted".to_string(),
        body: "user@example.com: 5h limit is 100% used.".to_string(),
        generated_at: 1,
    };
    let switch = AutoSwitchNotification {
        current_account_label: "user@example.com".to_string(),
        target_account_label: "other@example.com".to_string(),
        window_label: "5h limit".to_string(),
        threshold_percent: 90.0,
        target_primary_remaining: 40.0,
        target_weekly_remaining: 75.0,
    };

    let visible_quota = quota_event_notification(&event, false);
    let hidden_quota = quota_event_notification(&event, true);
    let visible_switch = auto_switch_notification(&switch, false);
    let hidden_switch = auto_switch_notification(&switch, true);

    assert!(visible_quota.body.contains("user@example.com"));
    assert!(!hidden_quota.body.contains("user@example.com"));
    assert!(hidden_quota.body.contains("A Codex account"));
    assert!(visible_switch.body.contains("user@example.com"));
    assert!(visible_switch.body.contains("other@example.com"));
    assert!(visible_switch.body.contains("90% auto-switch threshold"));
    assert!(!visible_switch.body.contains("was exhausted"));
    assert!(!hidden_switch.body.contains("user@example.com"));
    assert!(!hidden_switch.body.contains("other@example.com"));
    assert!(hidden_switch.body.contains("another Codex account"));
    assert!(hidden_switch.body.contains("90% auto-switch threshold"));
    assert!(!hidden_switch.body.contains("was exhausted"));
}
