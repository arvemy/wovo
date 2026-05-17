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

#[test]
fn diagnostics_state_recovers_from_poisoned_mutex() {
    let state = NotificationDiagnosticsState::default();
    let state_ref = &state;
    let _ = std::panic::catch_unwind(move || {
        let _guard = state_ref.latest.lock().unwrap();
        panic!("poison diagnostics mutex");
    });

    let diagnostics = NotificationDiagnostics {
        last_attempt_at: Some(1),
        last_status: Some("ok".to_string()),
        last_error: None,
        last_title: Some("Recovered".to_string()),
    };
    state.record(diagnostics.clone());

    assert_eq!(state.current(), diagnostics);
}

#[cfg(target_os = "linux")]
#[test]
fn command_available_requires_executable_path_entry() {
    use std::os::unix::fs::PermissionsExt;

    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!(
        "wovo-notification-path-{}-{unique}",
        std::process::id()
    ));
    let bin_dir = temp_dir.join("bin");
    let command = bin_dir.join(LINUX_NOTIFICATION_SETTINGS_COMMAND);

    std::fs::create_dir_all(&bin_dir).expect("create test bin directory");
    std::fs::write(&command, "#!/bin/sh\n").expect("write test command");

    assert!(!command_available_in_paths(
        LINUX_NOTIFICATION_SETTINGS_COMMAND,
        [bin_dir.clone()]
    ));

    let mut permissions = std::fs::metadata(&command)
        .expect("read test command metadata")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&command, permissions).expect("make test command executable");

    assert!(command_available_in_paths(
        LINUX_NOTIFICATION_SETTINGS_COMMAND,
        [bin_dir]
    ));

    let _ = std::fs::remove_dir_all(temp_dir);
}
