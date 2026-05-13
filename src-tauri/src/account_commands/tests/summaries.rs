use super::*;

#[test]
fn live_account_matching_existing_record_is_summarized_once() {
    let id = Uuid::new_v4();
    let record = ManagedCodexAccountRecord {
        id,
        email: Some("user@example.com".to_string()),
        provider_account_id: Some("account-1".to_string()),
        workspace_account_id: None,
        workspace_label: None,
        home_path: "/tmp/home".to_string(),
        created_at: 1,
        updated_at: 2,
        last_authenticated_at: Some(3),
    };
    let live = LiveCodexIdentity {
        email: Some("USER@example.com".to_string()),
        provider_account_id: Some("account-1".to_string()),
        workspace_account_id: None,
        record: Some(record.clone()),
    };

    let summaries = summarize_accounts(vec![record], Some(&live));

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, id.to_string());
    assert!(summaries[0].is_live_system);
    assert!(!summaries[0].can_set_system);
    assert!(!summaries[0].can_remove);
}
#[test]
fn live_system_account_is_not_removable_or_settable() {
    let id = Uuid::new_v4();
    let record = ManagedCodexAccountRecord {
        id,
        email: Some("user@example.com".to_string()),
        provider_account_id: Some("account-1".to_string()),
        workspace_account_id: None,
        workspace_label: None,
        home_path: "/tmp/home".to_string(),
        created_at: 1,
        updated_at: 2,
        last_authenticated_at: Some(3),
    };
    let live = LiveCodexIdentity {
        email: Some("user@example.com".to_string()),
        provider_account_id: Some("account-1".to_string()),
        workspace_account_id: None,
        record: Some(record.clone()),
    };

    let summaries = summarize_accounts(vec![record], Some(&live));

    assert_eq!(summaries.len(), 1);
    assert!(summaries[0].is_live_system);
    assert!(!summaries[0].can_set_system);
    assert!(!summaries[0].can_remove);
}
#[test]
fn provider_live_identity_does_not_mark_legacy_email_only_record() {
    let legacy_id = Uuid::new_v4();
    let provider_id = Uuid::new_v4();
    let legacy_record = ManagedCodexAccountRecord {
        id: legacy_id,
        email: Some("user@example.com".to_string()),
        provider_account_id: None,
        workspace_account_id: None,
        workspace_label: None,
        home_path: "/tmp/legacy".to_string(),
        created_at: 1,
        updated_at: 2,
        last_authenticated_at: Some(3),
    };
    let provider_record = ManagedCodexAccountRecord {
        id: provider_id,
        email: Some("user@example.com".to_string()),
        provider_account_id: Some("account-1".to_string()),
        workspace_account_id: None,
        workspace_label: None,
        home_path: "/tmp/provider".to_string(),
        created_at: 1,
        updated_at: 2,
        last_authenticated_at: Some(3),
    };
    let live = LiveCodexIdentity {
        email: Some("user@example.com".to_string()),
        provider_account_id: Some("account-1".to_string()),
        workspace_account_id: None,
        record: Some(provider_record.clone()),
    };

    let summaries = summarize_accounts(vec![legacy_record, provider_record], Some(&live));
    let legacy_summary = summaries
        .iter()
        .find(|summary| summary.id == legacy_id.to_string())
        .unwrap();
    let provider_summary = summaries
        .iter()
        .find(|summary| summary.id == provider_id.to_string())
        .unwrap();

    assert!(!legacy_summary.is_live_system);
    assert!(legacy_summary.can_remove);
    assert!(legacy_summary.can_set_system);
    assert!(provider_summary.is_live_system);
    assert!(!provider_summary.can_remove);
    assert!(!provider_summary.can_set_system);
}
#[test]
fn same_email_workspace_accounts_get_disambiguated_labels() {
    let personal_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();
    let records = vec![
        ManagedCodexAccountRecord {
            id: personal_id,
            email: Some("user@example.com".to_string()),
            provider_account_id: Some("provider-personal".to_string()),
            workspace_account_id: Some("account-personal123".to_string()),
            workspace_label: Some("Personal".to_string()),
            home_path: "/tmp/personal".to_string(),
            created_at: 1,
            updated_at: 2,
            last_authenticated_at: Some(3),
        },
        ManagedCodexAccountRecord {
            id: team_id,
            email: Some("user@example.com".to_string()),
            provider_account_id: Some("provider-team".to_string()),
            workspace_account_id: Some("account-team123".to_string()),
            workspace_label: Some("Team Workspace".to_string()),
            home_path: "/tmp/team".to_string(),
            created_at: 1,
            updated_at: 2,
            last_authenticated_at: Some(3),
        },
    ];

    let summaries = summarize_accounts(records, None);
    let personal = summaries
        .iter()
        .find(|summary| summary.id == personal_id.to_string())
        .unwrap();
    let team = summaries
        .iter()
        .find(|summary| summary.id == team_id.to_string())
        .unwrap();

    assert_eq!(personal.label, "user@example.com - Personal");
    assert_eq!(team.label, "user@example.com - Team Workspace");
}
#[test]
fn non_personal_workspace_label_is_shown_without_duplicate_email() {
    let id = Uuid::new_v4();
    let record = ManagedCodexAccountRecord {
        id,
        email: Some("user@example.com".to_string()),
        provider_account_id: Some("provider-team".to_string()),
        workspace_account_id: Some("account-team123".to_string()),
        workspace_label: Some("Team Workspace".to_string()),
        home_path: "/tmp/team".to_string(),
        created_at: 1,
        updated_at: 2,
        last_authenticated_at: Some(3),
    };

    let summaries = summarize_accounts(vec![record], None);

    assert_eq!(summaries[0].label, "user@example.com - Team Workspace");
}
#[test]
fn token_only_ambient_account_remains_listed() {
    let ambient = AccountSummary::ambient("/tmp/codex".to_string(), None, None, None, None);

    let summaries = summarize_account_list(Vec::new(), None, Some(ambient));

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, "ambient");
    assert!(matches!(
        summaries[0].source.clone(),
        AccountSourceKind::Ambient
    ));
    assert!(summaries[0].authenticated);
}
#[test]
fn live_system_account_sorts_first() {
    let root = temp_root("list-autoswitch-current-dir");
    let shared = temp_root("list-autoswitch-shared");
    let store = ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
    let first_id = Uuid::new_v4();
    let second_id = Uuid::new_v4();
    let first_home = store.create_home(first_id).unwrap();
    let second_home = store.create_home(second_id).unwrap();
    store
        .upsert_authenticated_account(
            first_id,
            Some("aaa@example.com".to_string()),
            Some("account-aaa".to_string()),
            first_home,
        )
        .unwrap();
    let second = store
        .upsert_authenticated_account(
            second_id,
            Some("zzz@example.com".to_string()),
            Some("account-system".to_string()),
            second_home,
        )
        .unwrap();
    let records = store.load_accounts().unwrap();
    let live = LiveCodexIdentity {
        email: Some("zzz@example.com".to_string()),
        provider_account_id: Some("account-system".to_string()),
        workspace_account_id: None,
        record: Some(second.0),
    };

    let summaries = summarize_accounts(records, Some(&live));

    assert_eq!(summaries[0].id, second_id.to_string());
    assert!(summaries[0].is_live_system);

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(shared);
}
