use super::*;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("wovo-{name}-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn path_contains_file_with_contents(path: &Path, contents: &str) -> bool {
    if path.is_file() {
        return fs::read_to_string(path)
            .map(|actual| actual == contents)
            .unwrap_or(false);
    }

    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries
        .filter_map(Result::ok)
        .any(|entry| path_contains_file_with_contents(&entry.path(), contents))
}

#[test]
fn stores_and_loads_managed_account() {
    let root = temp_root("store-load");
    let store = ManagedCodexAccountStore::new(root.clone());
    let id = Uuid::new_v4();
    let home = store.create_home(id).unwrap();

    let (record, _) = store
        .upsert_authenticated_account(
            id,
            Some("USER@Example.COM".to_string()),
            Some("account-1".to_string()),
            home,
        )
        .unwrap();

    let loaded = store.load_accounts().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, record.id);
    assert_eq!(loaded[0].email.as_deref(), Some("user@example.com"));
    assert_eq!(loaded[0].provider_account_id.as_deref(), Some("account-1"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn refuses_to_remove_home_outside_managed_root() {
    let root = temp_root("unsafe-remove");
    let store = ManagedCodexAccountStore::new(root.clone());
    let outside = std::env::temp_dir().join(format!("wovo-outside-{}", Uuid::new_v4()));
    fs::create_dir_all(&outside).unwrap();

    let error = store.remove_home_if_safe(&outside).unwrap_err();
    assert!(matches!(error, AppError::UnsafeManagedHome(_)));

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}

#[test]
fn remove_account_keeps_record_when_home_path_is_unsafe() {
    let root = temp_root("unsafe-account-remove");
    let store = ManagedCodexAccountStore::new(root.clone());
    let outside = temp_root("unsafe-account-home");
    let id = Uuid::new_v4();
    let payload = ManagedCodexAccountSet {
        version: STORE_VERSION,
        accounts: vec![ManagedCodexAccountRecord {
            id,
            email: Some("user@example.com".to_string()),
            provider_account_id: Some("account-1".to_string()),
            workspace_account_id: None,
            workspace_label: None,
            home_path: outside.to_string_lossy().to_string(),
            created_at: 1,
            updated_at: 2,
            last_authenticated_at: Some(3),
        }],
    };
    fs::write(
        root.join(STORE_FILE_NAME),
        serde_json::to_string_pretty(&payload).unwrap(),
    )
    .unwrap();

    let error = store.remove_account(&id.to_string()).unwrap_err();

    assert!(matches!(error, AppError::UnsafeManagedHome(_)));
    assert_eq!(store.load_accounts().unwrap().len(), 1);
    assert!(outside.exists());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}

#[test]
fn remove_account_removes_record_and_home() {
    let root = temp_root("remove-account");
    let store = ManagedCodexAccountStore::new(root.clone());
    let id = Uuid::new_v4();
    let home = store.create_home(id).unwrap();
    store
        .upsert_authenticated_account(
            id,
            Some("user@example.com".to_string()),
            Some("account-1".to_string()),
            home.clone(),
        )
        .unwrap();

    store.remove_account(&id.to_string()).unwrap();

    assert!(store.load_accounts().unwrap().is_empty());
    assert!(!home.exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
#[cfg(unix)]
fn remove_account_restores_record_when_home_cleanup_fails() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_root("remove-account-cleanup-fails");
    let store = ManagedCodexAccountStore::new(root.clone());
    let id = Uuid::new_v4();
    let home = store.create_home(id).unwrap();
    let locked_dir = home.join("locked");
    fs::create_dir_all(&locked_dir).unwrap();
    fs::write(locked_dir.join("auth-fragment.json"), "{}").unwrap();
    fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o500)).unwrap();
    store
        .upsert_authenticated_account(
            id,
            Some("user@example.com".to_string()),
            Some("account-1".to_string()),
            home.clone(),
        )
        .unwrap();

    let error = store.remove_account(&id.to_string()).unwrap_err();

    assert!(matches!(error, AppError::AccountStore(_)));
    let accounts = store.load_accounts().unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, id);
    assert_eq!(accounts[0].home_path, home.to_string_lossy().to_string());
    assert!(home.join("locked").join("auth-fragment.json").exists());
    assert!(!fs::read_dir(store.managed_homes_dir())
        .unwrap()
        .any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".removing.")
        }));

    fs::set_permissions(home.join("locked"), fs::Permissions::from_mode(0o700)).unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_reauth_id_replaces_existing_home_after_success() {
    let root = temp_root("reauth-replace");
    let store = ManagedCodexAccountStore::new(root.clone());
    let id = Uuid::new_v4();
    let first_home = store.create_home(id).unwrap();
    store
        .upsert_authenticated_account(
            id,
            Some("first@example.com".to_string()),
            Some("account-1".to_string()),
            first_home.clone(),
        )
        .unwrap();

    let second_home = store.create_home(Uuid::new_v4()).unwrap();
    let (record, replaced) = store
        .upsert_authenticated_account(
            id,
            Some("second@example.com".to_string()),
            Some("account-1".to_string()),
            second_home.clone(),
        )
        .unwrap();

    assert_eq!(record.id, id);
    assert_eq!(record.email.as_deref(), Some("second@example.com"));
    assert_eq!(replaced, vec![first_home]);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_reauth_id_rejects_mismatched_identity() {
    let root = temp_root("reauth-mismatch");
    let store = ManagedCodexAccountStore::new(root.clone());
    let id = Uuid::new_v4();
    let first_home = store.create_home(id).unwrap();
    store
        .upsert_authenticated_account(
            id,
            Some("first@example.com".to_string()),
            Some("account-1".to_string()),
            first_home,
        )
        .unwrap();

    let second_home = store.create_home(Uuid::new_v4()).unwrap();
    let error = store
        .upsert_authenticated_account(
            id,
            Some("second@example.com".to_string()),
            Some("account-2".to_string()),
            second_home,
        )
        .unwrap_err();

    assert!(matches!(error, AppError::AccountIdentityMismatch));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn same_email_with_different_provider_accounts_can_coexist() {
    let root = temp_root("same-email-providers");
    let store = ManagedCodexAccountStore::new(root.clone());
    let first_id = Uuid::new_v4();
    let second_id = Uuid::new_v4();
    let first_home = store.create_home(first_id).unwrap();
    let second_home = store.create_home(second_id).unwrap();

    store
        .upsert_authenticated_account(
            first_id,
            Some("user@example.com".to_string()),
            Some("account-1".to_string()),
            first_home,
        )
        .unwrap();
    store
        .upsert_authenticated_account(
            second_id,
            Some("user@example.com".to_string()),
            Some("account-2".to_string()),
            second_home,
        )
        .unwrap();

    let loaded = store.load_accounts().unwrap();
    assert_eq!(loaded.len(), 2);
    assert!(loaded.iter().any(|account| {
        account.email.as_deref() == Some("user@example.com")
            && account.provider_account_id.as_deref() == Some("account-1")
    }));
    assert!(loaded.iter().any(|account| {
        account.email.as_deref() == Some("user@example.com")
            && account.provider_account_id.as_deref() == Some("account-2")
    }));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn same_email_with_different_workspace_accounts_can_coexist() {
    let root = temp_root("same-email-workspaces");
    let store = ManagedCodexAccountStore::new(root.clone());
    let first_id = Uuid::new_v4();
    let second_id = Uuid::new_v4();
    let first_home = store.create_home(first_id).unwrap();
    let second_home = store.create_home(second_id).unwrap();

    store
        .upsert_authenticated_account_with_workspace(
            first_id,
            Some("user@example.com".to_string()),
            Some("provider-user".to_string()),
            Some("workspace-1".to_string()),
            Some("Personal".to_string()),
            first_home,
        )
        .unwrap();
    store
        .upsert_authenticated_account_with_workspace(
            second_id,
            Some("user@example.com".to_string()),
            Some("provider-user".to_string()),
            Some("workspace-2".to_string()),
            Some("Team".to_string()),
            second_home,
        )
        .unwrap();

    let loaded = store.load_accounts().unwrap();
    assert_eq!(loaded.len(), 2);
    assert!(loaded
        .iter()
        .any(|account| account.workspace_account_id.as_deref() == Some("workspace-1")));
    assert!(loaded
        .iter()
        .any(|account| account.workspace_account_id.as_deref() == Some("workspace-2")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn provider_identity_does_not_merge_legacy_email_account() {
    let root = temp_root("legacy-upgrade");
    let store = ManagedCodexAccountStore::new(root.clone());
    let legacy_id = Uuid::new_v4();
    let new_id = Uuid::new_v4();
    let legacy_home = store.create_home(legacy_id).unwrap();
    let new_home = store.create_home(new_id).unwrap();

    store
        .upsert_authenticated_account(
            legacy_id,
            Some("user@example.com".to_string()),
            None,
            legacy_home.clone(),
        )
        .unwrap();
    let (record, replaced) = store
        .upsert_authenticated_account(
            new_id,
            Some("user@example.com".to_string()),
            Some("account-1".to_string()),
            new_home,
        )
        .unwrap();

    let loaded = store.load_accounts().unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(record.id, new_id);
    assert_eq!(record.provider_account_id.as_deref(), Some("account-1"));
    assert!(replaced.is_empty());
    assert!(loaded
        .iter()
        .any(|account| account.id == legacy_id && account.provider_account_id.is_none()));
    assert!(legacy_home.exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn provider_only_lookup_matches_workspace_record_when_workspace_is_unavailable() {
    let workspace_id = Uuid::new_v4();
    let other_id = Uuid::new_v4();
    let accounts = vec![
        ManagedCodexAccountRecord {
            id: workspace_id,
            email: Some("user@example.com".to_string()),
            provider_account_id: Some("provider-user".to_string()),
            workspace_account_id: Some("workspace-1".to_string()),
            workspace_label: Some("Team".to_string()),
            home_path: "/tmp/workspace".to_string(),
            created_at: 1,
            updated_at: 2,
            last_authenticated_at: Some(3),
        },
        ManagedCodexAccountRecord {
            id: other_id,
            email: Some("other@example.com".to_string()),
            provider_account_id: Some("provider-other".to_string()),
            workspace_account_id: Some("workspace-2".to_string()),
            workspace_label: Some("Other".to_string()),
            home_path: "/tmp/other".to_string(),
            created_at: 1,
            updated_at: 2,
            last_authenticated_at: Some(3),
        },
    ];

    let matched = find_matching_account_index(
        &accounts,
        Some("user@example.com"),
        Some("provider-user"),
        None,
    )
    .unwrap();

    assert_eq!(accounts[matched].id, workspace_id);
    assert!(authenticated_identity_matches(
        &accounts[matched],
        Some("user@example.com"),
        Some("provider-user"),
        None
    ));
}

#[test]
fn provider_only_upsert_preserves_existing_workspace_metadata() {
    let root = temp_root("workspace-preserve");
    let store = ManagedCodexAccountStore::new(root.clone());
    let id = Uuid::new_v4();
    let original_home = store.create_home(id).unwrap();
    let refreshed_home = store.create_home(Uuid::new_v4()).unwrap();

    store
        .upsert_authenticated_account_with_workspace(
            id,
            Some("user@example.com".to_string()),
            Some("provider-user".to_string()),
            Some("workspace-1".to_string()),
            Some("Team".to_string()),
            original_home,
        )
        .unwrap();

    let (record, _replaced) = store
        .upsert_authenticated_account(
            Uuid::new_v4(),
            Some("user@example.com".to_string()),
            Some("provider-user".to_string()),
            refreshed_home,
        )
        .unwrap();

    let loaded = store.load_accounts().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(record.id, id);
    assert_eq!(record.workspace_account_id.as_deref(), Some("workspace-1"));
    assert_eq!(record.workspace_label.as_deref(), Some("Team"));
    assert_eq!(
        loaded[0].workspace_account_id.as_deref(),
        Some("workspace-1")
    );
    assert_eq!(loaded[0].workspace_label.as_deref(), Some("Team"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolved_workspace_lookup_does_not_match_different_workspace_with_same_provider() {
    let accounts = vec![ManagedCodexAccountRecord {
        id: Uuid::new_v4(),
        email: Some("user@example.com".to_string()),
        provider_account_id: Some("provider-user".to_string()),
        workspace_account_id: Some("workspace-1".to_string()),
        workspace_label: Some("Team".to_string()),
        home_path: "/tmp/workspace".to_string(),
        created_at: 1,
        updated_at: 2,
        last_authenticated_at: Some(3),
    }];

    assert!(find_matching_account_index(
        &accounts,
        Some("user@example.com"),
        Some("provider-user"),
        Some("workspace-2"),
    )
    .is_none());
    assert!(!authenticated_identity_matches(
        &accounts[0],
        Some("user@example.com"),
        Some("provider-user"),
        Some("workspace-2")
    ));
}

#[test]
fn email_only_lookup_does_not_match_workspace_record() {
    let account = ManagedCodexAccountRecord {
        id: Uuid::new_v4(),
        email: Some("user@example.com".to_string()),
        provider_account_id: Some("provider-user".to_string()),
        workspace_account_id: Some("workspace-1".to_string()),
        workspace_label: Some("Team".to_string()),
        home_path: "/tmp/workspace".to_string(),
        created_at: 1,
        updated_at: 2,
        last_authenticated_at: Some(3),
    };

    assert!(find_matching_account_index(
        std::slice::from_ref(&account),
        Some("user@example.com"),
        None,
        None
    )
    .is_none());
    assert!(!authenticated_identity_matches(
        &account,
        Some("user@example.com"),
        None,
        None
    ));
}

#[test]
#[cfg(unix)]
fn managed_home_is_auth_only_and_does_not_link_shared_state() {
    let root = temp_root("auth-overlay");
    let shared = temp_root("auth-overlay-shared");
    fs::write(shared.join("history.jsonl"), "shared-history").unwrap();
    fs::write(shared.join(".codex-global-state.json"), "{}").unwrap();
    fs::write(shared.join("session_index.jsonl"), "{}").unwrap();
    fs::create_dir_all(shared.join("sessions")).unwrap();
    fs::create_dir_all(shared.join("archived_sessions")).unwrap();
    let store = ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());

    let home = store.create_home(Uuid::new_v4()).unwrap();
    fs::write(home.join("auth.json"), "{}").unwrap();

    assert!(!fs::symlink_metadata(home.join("auth.json"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!home.join("history.jsonl").exists());
    assert!(!home.join(".codex-global-state.json").exists());
    assert!(!home.join("session_index.jsonl").exists());
    assert!(!home.join("sessions").exists());
    assert!(!home.join("archived_sessions").exists());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(shared);
}

#[test]
#[cfg(unix)]
fn prepare_home_backs_up_materialized_state_without_touching_auth() {
    let root = temp_root("promote-state");
    let shared = temp_root("promote-state-shared");
    let store = ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
    let home = store.make_home_path(Uuid::new_v4());
    fs::create_dir_all(home.join("sessions")).unwrap();
    fs::write(home.join("history.jsonl"), "managed-history").unwrap();
    fs::write(home.join("sessions").join("session.jsonl"), "{}").unwrap();
    fs::write(home.join("auth.json"), "{}").unwrap();

    store.prepare_home(&home).unwrap();

    assert!(!shared.join("history.jsonl").exists());
    assert!(!shared.join("sessions").exists());
    assert!(!home.join("history.jsonl").exists());
    assert!(!home.join("sessions").exists());
    assert!(path_contains_file_with_contents(
        &root.join(BACKUPS_DIR_NAME),
        "managed-history"
    ));
    assert!(path_contains_file_with_contents(
        &root.join(BACKUPS_DIR_NAME),
        "{}"
    ));
    assert!(!fs::symlink_metadata(home.join("auth.json"))
        .unwrap()
        .file_type()
        .is_symlink());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(shared);
}

#[test]
#[cfg(unix)]
fn prepare_home_backs_up_non_auth_files_without_touching_shared_home() {
    let root = temp_root("backup-conflict");
    let shared = temp_root("backup-conflict-shared");
    fs::write(shared.join("history.jsonl"), "shared-history").unwrap();
    let store = ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
    let home = store.make_home_path(Uuid::new_v4());
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("history.jsonl"), "managed-history").unwrap();

    store.prepare_home(&home).unwrap();

    assert_eq!(
        fs::read_to_string(shared.join("history.jsonl")).unwrap(),
        "shared-history"
    );
    assert!(!home.join("history.jsonl").exists());
    assert!(path_contains_file_with_contents(
        &root.join(BACKUPS_DIR_NAME),
        "managed-history"
    ));

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(shared);
}

#[test]
#[cfg(unix)]
fn legacy_current_symlink_and_account_id_are_cleaned_up() {
    let root = temp_root("cleanup-current-symlink");
    let store = ManagedCodexAccountStore::new(root.clone());
    let target = temp_root("cleanup-current-target");
    std::os::unix::fs::symlink(&target, store.current_link_path()).unwrap();
    fs::write(store.current_account_id_path(), Uuid::new_v4().to_string()).unwrap();

    store.cleanup_legacy_current_state().unwrap();

    assert!(!store.current_link_path().exists());
    assert!(!store.current_account_id_path().exists());
    assert!(target.exists());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(target);
}

#[test]
fn non_empty_materialized_current_directory_is_moved_to_backups() {
    let root = temp_root("cleanup-current-backup");
    let store = ManagedCodexAccountStore::new(root.clone());
    let current = store.current_link_path();
    fs::create_dir_all(current.join("sessions")).unwrap();
    fs::write(current.join("auth.json"), "legacy-auth").unwrap();
    fs::write(
        current.join("sessions").join("session.jsonl"),
        "legacy-session",
    )
    .unwrap();

    store.cleanup_legacy_current_state().unwrap();

    assert!(!current.exists());
    assert!(path_contains_file_with_contents(
        &root.join(BACKUPS_DIR_NAME),
        "legacy-auth"
    ));
    assert!(path_contains_file_with_contents(
        &root.join(BACKUPS_DIR_NAME),
        "legacy-session"
    ));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn migrates_legacy_app_data_accounts_to_wovo_codex_root() {
    let root = temp_root("migration-new");
    let legacy_root = temp_root("migration-legacy");
    let id = Uuid::new_v4();
    let legacy_home = legacy_root.join("managed-codex-homes").join(id.to_string());
    fs::create_dir_all(&legacy_home).unwrap();
    fs::write(legacy_home.join("auth.json"), "{}").unwrap();
    fs::create_dir_all(legacy_home.join("sessions")).unwrap();
    fs::write(legacy_home.join("sessions").join("session.jsonl"), "{}").unwrap();
    fs::create_dir_all(legacy_home.join("rules")).unwrap();
    fs::write(legacy_home.join("rules").join("legacy.rules"), "legacy").unwrap();

    let payload = ManagedCodexAccountSet {
        version: STORE_VERSION,
        accounts: vec![ManagedCodexAccountRecord {
            id,
            email: Some("user@example.com".to_string()),
            provider_account_id: Some("account-1".to_string()),
            workspace_account_id: None,
            workspace_label: None,
            home_path: legacy_home.to_string_lossy().to_string(),
            created_at: 1,
            updated_at: 2,
            last_authenticated_at: Some(3),
        }],
    };
    fs::write(
        legacy_root.join(STORE_FILE_NAME),
        serde_json::to_string_pretty(&payload).unwrap(),
    )
    .unwrap();

    let store = ManagedCodexAccountStore::with_legacy_root(root.clone(), legacy_root.clone());
    let loaded = store.load_accounts().unwrap();

    assert_eq!(loaded.len(), 1);
    let migrated_home = root.join("accounts").join(id.to_string());
    assert_eq!(
        loaded[0].home_path,
        migrated_home.to_string_lossy().to_string()
    );
    assert!(migrated_home.join("auth.json").exists());
    assert!(!migrated_home.join("sessions").exists());
    assert!(!migrated_home.join("rules").exists());
    assert!(legacy_home.join("auth.json").exists());
    assert!(legacy_home.join("sessions").join("session.jsonl").exists());
    assert!(legacy_home.join("rules").join("legacy.rules").exists());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(legacy_root);
}
