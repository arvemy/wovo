use super::*;

fn default_switch_settings() -> CodexSettings {
    CodexSettings::default()
}
fn switch_account(
    id: &str,
    label: &str,
    source: AccountSourceKind,
    is_live_system: bool,
) -> AccountSummary {
    AccountSummary {
        id: id.to_string(),
        label: label.to_string(),
        email: Some(label.to_string()),
        provider_account_id: Some(format!("provider-{id}")),
        workspace_account_id: None,
        workspace_label: None,
        home_path: format!("/tmp/{id}"),
        source,
        authenticated: true,
        is_live_system,
        can_set_system: !is_live_system,
        can_remove: !is_live_system,
        created_at: None,
        updated_at: None,
        last_authenticated_at: None,
    }
}
fn usage_for_account(
    account_id: &str,
    primary_used: f64,
    primary_remaining: f64,
    secondary_used: f64,
    secondary_remaining: f64,
) -> UsageSnapshot {
    UsageSnapshot {
        account_id: account_id.to_string(),
        source: "oauth".to_string(),
        plan_type: Some("pro".to_string()),
        primary: Some(UsageWindow {
            label: "5h limit".to_string(),
            used_percent: primary_used,
            remaining_percent: primary_remaining,
            reset_at: Some(1),
            window_seconds: Some(18_000),
        }),
        secondary: Some(UsageWindow {
            label: "Weekly limit".to_string(),
            used_percent: secondary_used,
            remaining_percent: secondary_remaining,
            reset_at: Some(1),
            window_seconds: Some(604_800),
        }),
        credits: None,
        updated_at: 1,
    }
}
fn usage_map(usages: Vec<UsageSnapshot>) -> HashMap<String, UsageSnapshot> {
    usages
        .into_iter()
        .map(|usage| (usage.account_id.clone(), usage))
        .collect()
}
fn no_usage_errors() -> HashMap<String, AccountIssue> {
    HashMap::new()
}
#[test]
fn auto_switch_candidate_requires_exhausted_live_managed_account() {
    let accounts = vec![
        switch_account(
            "current",
            "current@example.com",
            AccountSourceKind::Managed,
            true,
        ),
        switch_account(
            "target",
            "target@example.com",
            AccountSourceKind::Managed,
            false,
        ),
    ];
    let usage = usage_map(vec![
        usage_for_account("current", 89.0, 11.0, 0.0, 100.0),
        usage_for_account("target", 10.0, 90.0, 0.0, 100.0),
    ]);

    assert!(auto_switch_candidate(
        &accounts,
        &usage,
        &no_usage_errors(),
        &default_switch_settings()
    )
    .is_none());

    let ambient_accounts = vec![
        switch_account(
            "ambient",
            "ambient@example.com",
            AccountSourceKind::Ambient,
            true,
        ),
        switch_account(
            "target",
            "target@example.com",
            AccountSourceKind::Managed,
            false,
        ),
    ];
    let ambient_usage = usage_map(vec![
        usage_for_account("ambient", 100.0, 0.0, 0.0, 100.0),
        usage_for_account("target", 10.0, 90.0, 0.0, 100.0),
    ]);

    assert!(auto_switch_candidate(
        &ambient_accounts,
        &ambient_usage,
        &no_usage_errors(),
        &default_switch_settings()
    )
    .is_none());
}
#[test]
fn auto_switch_candidate_uses_most_remaining_for_same_exhausted_window() {
    let accounts = vec![
        switch_account(
            "current",
            "current@example.com",
            AccountSourceKind::Managed,
            true,
        ),
        switch_account("low", "low@example.com", AccountSourceKind::Managed, false),
        switch_account(
            "high",
            "high@example.com",
            AccountSourceKind::Managed,
            false,
        ),
    ];
    let usage = usage_map(vec![
        usage_for_account("current", 0.0, 100.0, 100.0, 0.0),
        usage_for_account("low", 0.0, 100.0, 20.0, 80.0),
        usage_for_account("high", 0.0, 100.0, 10.0, 90.0),
    ]);

    let candidate = auto_switch_candidate(
        &accounts,
        &usage,
        &no_usage_errors(),
        &default_switch_settings(),
    )
    .unwrap();

    assert_eq!(candidate.current_account_id, "current");
    assert_eq!(candidate.target_account_id, "high");
    assert_eq!(candidate.notification.window_label, "Weekly limit");
}
#[test]
fn auto_switch_candidate_returns_none_without_eligible_target() {
    let accounts = vec![
        switch_account(
            "current",
            "current@example.com",
            AccountSourceKind::Managed,
            true,
        ),
        switch_account(
            "exhausted",
            "exhausted@example.com",
            AccountSourceKind::Managed,
            false,
        ),
    ];
    let usage = usage_map(vec![
        usage_for_account("current", 100.0, 0.0, 0.0, 100.0),
        usage_for_account("exhausted", 100.0, 0.0, 0.0, 100.0),
    ]);

    assert!(auto_switch_candidate(
        &accounts,
        &usage,
        &no_usage_errors(),
        &default_switch_settings()
    )
    .is_none());
}
#[test]
fn auto_switch_candidate_ignores_accounts_with_refresh_errors() {
    let accounts = vec![
        switch_account(
            "current",
            "current@example.com",
            AccountSourceKind::Managed,
            true,
        ),
        switch_account(
            "stale",
            "stale@example.com",
            AccountSourceKind::Managed,
            false,
        ),
        switch_account(
            "healthy",
            "healthy@example.com",
            AccountSourceKind::Managed,
            false,
        ),
    ];
    let usage = usage_map(vec![
        usage_for_account("current", 100.0, 0.0, 0.0, 100.0),
        usage_for_account("stale", 10.0, 90.0, 0.0, 100.0),
        usage_for_account("healthy", 50.0, 50.0, 0.0, 100.0),
    ]);
    let mut target_errors = HashMap::new();
    target_errors.insert(
        "stale".to_string(),
        AccountIssue::new("refresh_failed", "Refresh failed.", false),
    );

    let candidate = auto_switch_candidate(
        &accounts,
        &usage,
        &target_errors,
        &default_switch_settings(),
    )
    .unwrap();

    assert_eq!(candidate.target_account_id, "healthy");

    let mut current_errors = HashMap::new();
    current_errors.insert(
        "current".to_string(),
        AccountIssue::new("refresh_failed", "Refresh failed.", false),
    );

    assert!(auto_switch_candidate(
        &accounts,
        &usage,
        &current_errors,
        &default_switch_settings()
    )
    .is_none());
}
#[test]
fn auto_switch_candidate_rejects_weekly_exhausted_targets() {
    let accounts = vec![
        switch_account(
            "current",
            "current@example.com",
            AccountSourceKind::Managed,
            true,
        ),
        switch_account(
            "weekly-exhausted",
            "weekly-exhausted@example.com",
            AccountSourceKind::Managed,
            false,
        ),
    ];
    let usage = usage_map(vec![
        usage_for_account("current", 100.0, 0.0, 0.0, 100.0),
        usage_for_account("weekly-exhausted", 10.0, 90.0, 100.0, 0.0),
    ]);

    assert!(auto_switch_candidate(
        &accounts,
        &usage,
        &no_usage_errors(),
        &default_switch_settings()
    )
    .is_none());
}
#[test]
fn auto_switch_candidate_ties_follow_displayed_order() {
    let accounts = vec![
        switch_account(
            "current",
            "current@example.com",
            AccountSourceKind::Managed,
            true,
        ),
        switch_account(
            "first",
            "first@example.com",
            AccountSourceKind::Managed,
            false,
        ),
        switch_account(
            "second",
            "second@example.com",
            AccountSourceKind::Managed,
            false,
        ),
    ];
    let usage = usage_map(vec![
        usage_for_account("current", 100.0, 0.0, 0.0, 100.0),
        usage_for_account("first", 50.0, 50.0, 0.0, 100.0),
        usage_for_account("second", 50.0, 50.0, 0.0, 100.0),
    ]);

    let candidate = auto_switch_candidate(
        &accounts,
        &usage,
        &no_usage_errors(),
        &default_switch_settings(),
    )
    .unwrap();

    assert_eq!(candidate.target_account_id, "first");
}
#[test]
fn auto_switch_candidate_skips_non_managed_targets() {
    let accounts = vec![
        switch_account(
            "current",
            "current@example.com",
            AccountSourceKind::Managed,
            true,
        ),
        switch_account(
            "ambient",
            "ambient@example.com",
            AccountSourceKind::Ambient,
            false,
        ),
        switch_account(
            "managed",
            "managed@example.com",
            AccountSourceKind::Managed,
            false,
        ),
    ];
    let usage = usage_map(vec![
        usage_for_account("current", 100.0, 0.0, 0.0, 100.0),
        usage_for_account("ambient", 5.0, 95.0, 0.0, 100.0),
        usage_for_account("managed", 50.0, 50.0, 0.0, 100.0),
    ]);

    let candidate = auto_switch_candidate(
        &accounts,
        &usage,
        &no_usage_errors(),
        &default_switch_settings(),
    )
    .unwrap();

    assert_eq!(candidate.target_account_id, "managed");
}
