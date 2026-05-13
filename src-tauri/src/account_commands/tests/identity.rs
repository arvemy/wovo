use super::*;

#[test]
fn account_identity_match_uses_provider_account_id_first() {
    let account = summary(Some("same@example.com"), Some("account-1"));
    assert!(account_matches_identity(
        &account,
        Some("different@example.com"),
        Some("account-1"),
        None
    ));
    assert!(!account_matches_identity(
        &account,
        Some("same@example.com"),
        Some("account-2"),
        None
    ));
}
#[test]
fn account_identity_match_falls_back_to_email() {
    let account = summary(Some("USER@example.com"), None);
    assert!(account_matches_identity(
        &account,
        Some("user@example.com"),
        None,
        None
    ));
}
#[test]
fn account_identity_match_does_not_merge_same_email_different_provider_accounts() {
    let account = summary(Some("same@example.com"), Some("account-1"));
    assert!(!account_matches_identity(
        &account,
        Some("same@example.com"),
        Some("account-2"),
        None
    ));
}
#[test]
fn account_identity_match_does_not_pairwise_match_provider_to_email_only_account() {
    let account = summary(Some("same@example.com"), None);
    assert!(!account_matches_identity(
        &account,
        Some("same@example.com"),
        Some("account-1"),
        None
    ));
}
#[test]
fn managed_record_matches_live_credentials_by_provider_id() {
    let id = Uuid::new_v4();
    let record = ManagedCodexAccountRecord {
        id,
        email: Some("different@example.com".to_string()),
        provider_account_id: Some("account-1".to_string()),
        workspace_account_id: None,
        workspace_label: None,
        home_path: "/tmp/home".to_string(),
        created_at: 1,
        updated_at: 2,
        last_authenticated_at: Some(3),
    };
    let credentials = CodexOAuthCredentials {
        access_token: "access".to_string(),
        refresh_token: "refresh".to_string(),
        id_token: None,
        account_id: Some("account-1".to_string()),
        last_refresh: None,
        home_path: PathBuf::from("/tmp/codex"),
    };

    assert_eq!(
        live_system_account_id_for_credentials(&[record], &credentials),
        Some(id)
    );
}
#[test]
fn managed_record_matches_workspace_record_by_provider_only_credentials() {
    let id = Uuid::new_v4();
    let record = ManagedCodexAccountRecord {
        id,
        email: Some("different@example.com".to_string()),
        provider_account_id: Some("account-1".to_string()),
        workspace_account_id: Some("workspace-1".to_string()),
        workspace_label: Some("Team".to_string()),
        home_path: "/tmp/home".to_string(),
        created_at: 1,
        updated_at: 2,
        last_authenticated_at: Some(3),
    };
    let credentials = CodexOAuthCredentials {
        access_token: "access".to_string(),
        refresh_token: "refresh".to_string(),
        id_token: None,
        account_id: Some("account-1".to_string()),
        last_refresh: None,
        home_path: PathBuf::from("/tmp/codex"),
    };

    assert!(managed_record_matches_credentials(&record, &credentials));
    assert_eq!(
        live_system_account_id_for_credentials(&[record], &credentials),
        Some(id)
    );
}
#[test]
fn live_credentials_do_not_fall_back_to_email_when_provider_id_exists() {
    let id = Uuid::new_v4();
    let record = ManagedCodexAccountRecord {
        id,
        email: Some("user@example.com".to_string()),
        provider_account_id: None,
        workspace_account_id: None,
        workspace_label: None,
        home_path: "/tmp/home".to_string(),
        created_at: 1,
        updated_at: 2,
        last_authenticated_at: Some(3),
    };
    let credentials = CodexOAuthCredentials {
        access_token: "access".to_string(),
        refresh_token: "refresh".to_string(),
        id_token: Some("header.eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20ifQ.signature".to_string()),
        account_id: Some("account-1".to_string()),
        last_refresh: None,
        home_path: PathBuf::from("/tmp/codex"),
    };

    assert_eq!(
        live_system_account_id_for_credentials(&[record], &credentials),
        None
    );
}
#[test]
fn live_matching_managed_account_uses_ambient_home_as_refresh_mirror() {
    let id = Uuid::new_v4();
    let account = ManagedCodexAccountRecord {
        id,
        email: Some("user@example.com".to_string()),
        provider_account_id: Some("account-1".to_string()),
        workspace_account_id: None,
        workspace_label: None,
        home_path: "/tmp/managed".to_string(),
        created_at: 1,
        updated_at: 2,
        last_authenticated_at: Some(3),
    };
    let ambient = CodexOAuthCredentials {
        access_token: "access".to_string(),
        refresh_token: "refresh".to_string(),
        id_token: Some("header.eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20ifQ.signature".to_string()),
        account_id: Some("account-1".to_string()),
        last_refresh: None,
        home_path: PathBuf::from("/tmp/codex"),
    };

    assert!(live_credential_mirror_home_for_account_with_ambient(
        &account, &ambient
    ));
}
#[test]
fn non_live_managed_account_does_not_use_ambient_home_as_refresh_mirror() {
    let account = ManagedCodexAccountRecord {
        id: Uuid::new_v4(),
        email: Some("other@example.com".to_string()),
        provider_account_id: Some("account-2".to_string()),
        workspace_account_id: None,
        workspace_label: None,
        home_path: "/tmp/managed".to_string(),
        created_at: 1,
        updated_at: 2,
        last_authenticated_at: Some(3),
    };
    let ambient = CodexOAuthCredentials {
        access_token: "access".to_string(),
        refresh_token: "refresh".to_string(),
        id_token: Some("header.eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20ifQ.signature".to_string()),
        account_id: Some("account-1".to_string()),
        last_refresh: None,
        home_path: PathBuf::from("/tmp/codex"),
    };

    assert!(!live_credential_mirror_home_for_account_with_ambient(
        &account, &ambient
    ));
}
