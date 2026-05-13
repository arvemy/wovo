use super::*;

#[test]
fn setting_managed_account_as_system_writes_auth_json() {
    let root = temp_root("set-system-root");
    let shared = temp_root("set-system-shared");
    let system_home = temp_root("set-system-home");
    let store = ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
    let id = Uuid::new_v4();
    let home = store.create_home(id).unwrap();
    write_auth(&home, "target-access", "account-target");
    store
        .upsert_authenticated_account(
            id,
            Some("target@example.com".to_string()),
            Some("account-target".to_string()),
            home,
        )
        .unwrap();

    let summary = set_system_codex_account_in_store(&store, &id.to_string(), &system_home).unwrap();
    let system_credentials = load_credentials_from_home(&system_home).unwrap();

    assert_eq!(system_credentials.access_token, "target-access");
    assert!(summary.is_live_system);
    assert!(!summary.can_set_system);
    assert!(!summary.can_remove);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(system_home.join("auth.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(shared);
    let _ = fs::remove_dir_all(system_home);
}
#[test]
fn previous_system_account_is_imported_before_overwrite() {
    let root = temp_root("set-system-preserve-root");
    let shared = temp_root("set-system-preserve-shared");
    let system_home = temp_root("set-system-preserve-home");
    write_auth(&system_home, "old-access", "account-old");
    let store = ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
    let id = Uuid::new_v4();
    let home = store.create_home(id).unwrap();
    write_auth(&home, "target-access", "account-target");
    store
        .upsert_authenticated_account(
            id,
            Some("target@example.com".to_string()),
            Some("account-target".to_string()),
            home,
        )
        .unwrap();

    set_system_codex_account_in_store(&store, &id.to_string(), &system_home).unwrap();
    let accounts = store.load_accounts().unwrap();
    let preserved = accounts
        .iter()
        .find(|account| account.provider_account_id.as_deref() == Some("account-old"))
        .unwrap();
    let preserved_credentials =
        load_credentials_from_home(&PathBuf::from(&preserved.home_path)).unwrap();
    let system_credentials = load_credentials_from_home(&system_home).unwrap();

    assert_eq!(accounts.len(), 2);
    assert_eq!(preserved_credentials.access_token, "old-access");
    assert_eq!(system_credentials.access_token, "target-access");

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(shared);
    let _ = fs::remove_dir_all(system_home);
}
#[test]
fn tokenless_system_auth_blocks_overwrite() {
    let root = temp_root("set-system-tokenless-root");
    let shared = temp_root("set-system-tokenless-shared");
    let system_home = temp_root("set-system-tokenless-home");
    fs::write(
        system_home.join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-test"}"#,
    )
    .unwrap();
    let store = ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
    let id = Uuid::new_v4();
    let home = store.create_home(id).unwrap();
    write_auth(&home, "target-access", "account-target");
    store
        .upsert_authenticated_account(
            id,
            Some("target@example.com".to_string()),
            Some("account-target".to_string()),
            home,
        )
        .unwrap();

    let error =
        set_system_codex_account_in_store(&store, &id.to_string(), &system_home).unwrap_err();

    assert!(matches!(error, AppError::MissingTokens));
    assert!(fs::read_to_string(system_home.join("auth.json"))
        .unwrap()
        .contains("sk-test"));

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(shared);
    let _ = fs::remove_dir_all(system_home);
}
#[test]
fn setting_already_system_identity_does_not_duplicate_accounts() {
    let root = temp_root("set-system-same-root");
    let shared = temp_root("set-system-same-shared");
    let system_home = temp_root("set-system-same-home");
    write_auth(&system_home, "system-access", "account-target");
    let store = ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
    let id = Uuid::new_v4();
    let home = store.create_home(id).unwrap();
    write_auth(&home, "target-access", "account-target");
    store
        .upsert_authenticated_account(
            id,
            Some("target@example.com".to_string()),
            Some("account-target".to_string()),
            home,
        )
        .unwrap();

    set_system_codex_account_in_store(&store, &id.to_string(), &system_home).unwrap();
    let accounts = store.load_accounts().unwrap();

    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, id);

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(shared);
    let _ = fs::remove_dir_all(system_home);
}
#[test]
fn removing_system_account_is_blocked() {
    let root = temp_root("remove-system-root");
    let shared = temp_root("remove-system-shared");
    let store = ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
    let id = Uuid::new_v4();
    let home = store.create_home(id).unwrap();
    store
        .upsert_authenticated_account(
            id,
            Some("target@example.com".to_string()),
            Some("account-target".to_string()),
            home,
        )
        .unwrap();
    let credentials = auth_credentials(Path::new("/tmp/codex"), "account-target");

    let error =
        remove_codex_account_from_store(&store, &id.to_string(), Some(&credentials)).unwrap_err();

    assert!(matches!(error, AppError::LiveAccountRemovalBlocked));
    assert_eq!(store.load_accounts().unwrap().len(), 1);

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(shared);
}
#[test]
fn removing_non_system_managed_account_works() {
    let root = temp_root("remove-non-system-root");
    let shared = temp_root("remove-non-system-shared");
    let store = ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
    let id = Uuid::new_v4();
    let home = store.create_home(id).unwrap();
    store
        .upsert_authenticated_account(
            id,
            Some("target@example.com".to_string()),
            Some("account-target".to_string()),
            home.clone(),
        )
        .unwrap();
    let credentials = auth_credentials(Path::new("/tmp/codex"), "account-other");

    remove_codex_account_from_store(&store, &id.to_string(), Some(&credentials)).unwrap();

    assert!(store.load_accounts().unwrap().is_empty());
    assert!(!home.exists());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(shared);
}
#[test]
fn refreshed_credentials_are_saved_to_mirror_home() {
    let source_home = temp_root("refresh-source-home");
    let mirror_home = temp_root("refresh-mirror-home");
    fs::write(
        source_home.join("auth.json"),
        r#"{"tokens":{"access_token":"old-access","refresh_token":"old-refresh"}}"#,
    )
    .unwrap();
    fs::write(
        mirror_home.join("auth.json"),
        r#"{"tokens":{"access_token":"ambient-access","refresh_token":"ambient-refresh"}}"#,
    )
    .unwrap();
    let credentials = CodexOAuthCredentials {
        access_token: "new-access".to_string(),
        refresh_token: "new-refresh".to_string(),
        id_token: Some("header.eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20ifQ.signature".to_string()),
        account_id: Some("account-1".to_string()),
        last_refresh: Some(time::OffsetDateTime::now_utc()),
        home_path: source_home.clone(),
    };

    save_credentials_to_home(&credentials, mirror_home.clone()).unwrap();
    let mirrored = load_credentials_from_home(&mirror_home).unwrap();
    let source = load_credentials_from_home(&source_home).unwrap();

    assert_eq!(mirrored.access_token, "new-access");
    assert_eq!(mirrored.refresh_token, "new-refresh");
    assert_eq!(mirrored.provider_account_id().as_deref(), Some("account-1"));
    assert_eq!(source.access_token, "old-access");
    assert_eq!(source.refresh_token, "old-refresh");

    let _ = fs::remove_dir_all(source_home);
    let _ = fs::remove_dir_all(mirror_home);
}
#[test]
fn system_reauth_mirror_failure_rolls_back_account_record() {
    let root = temp_root("reauth-mirror-rollback-root");
    let shared = temp_root("reauth-mirror-rollback-shared");
    let mirror_parent = temp_root("reauth-mirror-rollback-mirror");
    let mirror_home = mirror_parent.join("not-a-directory");
    fs::write(&mirror_home, "not a directory").unwrap();
    let store = ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
    let account_id = Uuid::new_v4();
    let old_home = store.create_home(account_id).unwrap();
    write_auth(&old_home, "old-access", "account-1");
    store
        .upsert_authenticated_account(
            account_id,
            Some("user@example.com".to_string()),
            Some("account-1".to_string()),
            old_home.clone(),
        )
        .unwrap();
    let new_home = store.create_home(Uuid::new_v4()).unwrap();
    write_auth(&new_home, "new-access", "account-1");

    let error = upsert_authenticated_account_and_mirror_system_if_needed(
        &store,
        account_id,
        Some("user@example.com".to_string()),
        Some("account-1".to_string()),
        None,
        new_home.clone(),
        Some(&mirror_home),
    )
    .unwrap_err();
    let loaded = store.find_account(&account_id.to_string()).unwrap();

    assert!(matches!(error, AppError::AuthRead(_)));
    assert_eq!(loaded.home_path, old_home.to_string_lossy().to_string());
    assert_eq!(
        load_credentials_from_home(&old_home).unwrap().access_token,
        "old-access"
    );
    assert!(new_home.join("auth.json").exists());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(shared);
    let _ = fs::remove_dir_all(mirror_parent);
}
