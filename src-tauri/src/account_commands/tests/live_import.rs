use super::*;

#[test]
fn existing_live_account_is_synced_before_returning() {
    let root = temp_root("live-sync-root");
    let source_home = temp_root("live-sync-source");
    let shared = temp_root("live-sync-shared");
    fs::write(
            source_home.join("auth.json"),
            r#"{"tokens":{"access_token":"new-access","refresh_token":"new-refresh","id_token":"header.eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20ifQ.signature","account_id":"account-1"}}"#,
        )
        .unwrap();
    let store = ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
    let legacy_id = Uuid::new_v4();
    let legacy_home = store.create_home(legacy_id).unwrap();
    fs::write(
        legacy_home.join("auth.json"),
        r#"{"tokens":{"access_token":"old-access","refresh_token":"old-refresh"}}"#,
    )
    .unwrap();
    store
        .upsert_authenticated_account(
            legacy_id,
            Some("user@example.com".to_string()),
            Some("account-1".to_string()),
            legacy_home.clone(),
        )
        .unwrap();
    let credentials = CodexOAuthCredentials {
        access_token: "new-access".to_string(),
        refresh_token: "new-refresh".to_string(),
        id_token: Some("header.eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20ifQ.signature".to_string()),
        account_id: Some("account-1".to_string()),
        last_refresh: None,
        home_path: source_home.clone(),
    };

    let live = ensure_live_account_imported_with_workspace(&store, &credentials, None)
        .unwrap()
        .unwrap();
    let loaded = store.load_accounts().unwrap();
    let managed_credentials = load_credentials_from_home(&legacy_home).unwrap();

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, legacy_id);
    assert_eq!(loaded[0].provider_account_id.as_deref(), Some("account-1"));
    assert_eq!(live.record.unwrap().id, legacy_id);
    assert_eq!(managed_credentials.access_token, "new-access");
    assert_eq!(managed_credentials.refresh_token, "new-refresh");

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(source_home);
    let _ = fs::remove_dir_all(shared);
}
#[test]
fn live_account_not_yet_stored_is_auto_imported() {
    let root = temp_root("live-import-root");
    let source_home = temp_root("live-import-source");
    let shared = temp_root("live-import-shared");
    fs::write(
        source_home.join("auth.json"),
        r#"{"tokens":{"access_token":"access","refresh_token":"refresh"}}"#,
    )
    .unwrap();
    let store = ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
    let credentials = CodexOAuthCredentials {
        access_token: "access".to_string(),
        refresh_token: "refresh".to_string(),
        id_token: Some("header.eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20ifQ.signature".to_string()),
        account_id: Some("account-1".to_string()),
        last_refresh: None,
        home_path: source_home.clone(),
    };

    let imported = ensure_live_account_imported_with_workspace(&store, &credentials, None)
        .unwrap()
        .unwrap();
    let loaded = store.load_accounts().unwrap();

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].email.as_deref(), Some("user@example.com"));
    assert_eq!(loaded[0].provider_account_id.as_deref(), Some("account-1"));
    assert!(PathBuf::from(&loaded[0].home_path)
        .join("auth.json")
        .exists());
    assert_eq!(imported.record.unwrap().id, loaded[0].id);
    assert!(source_home.join("auth.json").exists());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(source_home);
    let _ = fs::remove_dir_all(shared);
}
