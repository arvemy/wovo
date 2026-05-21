use crate::claude::account_store::{
    managed_account_store, ManagedClaudeAccountRecord, ManagedClaudeAccountStore,
};
use crate::claude::auth_store::{
    credentials_file_lacks_claude_oauth_payload, detected_ambient_account,
    load_ambient_credentials, load_credentials_from_home, replace_credentials_from_home,
    save_credentials, system_claude_home_path, ClaudeOAuthCredentials,
};
use crate::claude::login_runner::{self, ClaudeLoginRunnerState};
use crate::claude::token_refresh;
use crate::claude::usage_fetcher;
use crate::domain::account::AccountSummary;
use crate::error::AppError;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, State};
use uuid::Uuid;

#[tauri::command]
pub(crate) fn get_detected_claude_account() -> Result<Option<AccountSummary>, AppError> {
    detected_ambient_account()
}

#[tauri::command]
pub(crate) async fn list_claude_accounts(_app: AppHandle) -> Result<Vec<AccountSummary>, AppError> {
    list_claude_accounts_inner()
}

pub(crate) fn list_claude_accounts_inner() -> Result<Vec<AccountSummary>, AppError> {
    let store = managed_account_store();
    let ambient_result = load_ambient_credentials();
    if let Ok(credentials) = ambient_result.as_ref() {
        ensure_live_claude_account_imported(&store, credentials)?;
    }
    let records = store.load_accounts()?;
    summaries_for_claude_accounts(records, ambient_result)
}

fn summaries_for_claude_accounts(
    records: Vec<ManagedClaudeAccountRecord>,
    ambient_result: Result<ClaudeOAuthCredentials, AppError>,
) -> Result<Vec<AccountSummary>, AppError> {
    summaries_for_claude_accounts_with_system_home(
        records,
        ambient_result,
        &system_claude_home_path(),
    )
}

fn summaries_for_claude_accounts_with_system_home(
    records: Vec<ManagedClaudeAccountRecord>,
    ambient_result: Result<ClaudeOAuthCredentials, AppError>,
    system_home: &Path,
) -> Result<Vec<AccountSummary>, AppError> {
    let cli_ambient = cli_ambient_summary_for_missing_oauth(&ambient_result, system_home)?;
    let ambient = match optional_ambient_credentials(ambient_result, !records.is_empty()) {
        Ok(ambient) => ambient,
        Err(AppError::ClaudeMissingTokens) if cli_ambient.is_some() => None,
        Err(error) => return Err(error),
    };
    let live_id = ambient
        .as_ref()
        .and_then(|credentials| live_system_account_id_for_credentials(&records, credentials));

    let mut summaries: Vec<AccountSummary> = records
        .iter()
        .map(|record| record.summary_with_status(Some(record.id) == live_id))
        .collect();

    if let Some(credentials) = ambient {
        if live_id.is_none() {
            summaries.push(AccountSummary::ambient(
                credentials.home_path.to_string_lossy().to_string(),
                None,
                credentials.provider_account_id(),
                None,
                credentials.plan_type(),
            ));
        }
    }
    if let Some(summary) = cli_ambient {
        summaries.push(summary);
    }

    Ok(summaries)
}

fn cli_ambient_summary_for_missing_oauth(
    ambient_result: &Result<ClaudeOAuthCredentials, AppError>,
    system_home: &Path,
) -> Result<Option<AccountSummary>, AppError> {
    if !matches!(ambient_result, Err(AppError::ClaudeMissingTokens)) {
        return Ok(None);
    }
    if !credentials_file_lacks_claude_oauth_payload(system_home)? {
        return Ok(None);
    }
    Ok(Some(AccountSummary::ambient(
        system_home.to_string_lossy().to_string(),
        None,
        None,
        None,
        Some("Claude CLI".to_string()),
    )))
}

fn optional_ambient_credentials(
    result: Result<ClaudeOAuthCredentials, AppError>,
    has_managed_accounts: bool,
) -> Result<Option<ClaudeOAuthCredentials>, AppError> {
    match result {
        Ok(credentials) => Ok(Some(credentials)),
        Err(AppError::ClaudeAuthNotFound) => Ok(None),
        Err(error) if has_managed_accounts && is_unusable_ambient_credentials_error(&error) => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn is_unusable_ambient_credentials_error(error: &AppError) -> bool {
    matches!(
        error,
        AppError::ClaudeAuthRead(_) | AppError::ClaudeAuthDecode(_) | AppError::ClaudeMissingTokens
    )
}

#[tauri::command]
pub(crate) async fn add_claude_account(
    _app: AppHandle,
    login_state: State<'_, ClaudeLoginRunnerState>,
) -> Result<AccountSummary, AppError> {
    authenticate_managed_account(&login_state, None).await
}

#[tauri::command]
pub(crate) async fn reauthenticate_claude_account(
    _app: AppHandle,
    login_state: State<'_, ClaudeLoginRunnerState>,
    account_id: String,
) -> Result<AccountSummary, AppError> {
    if account_id == "ambient" {
        let system_home = system_claude_home_path();
        login_runner::run_login(&login_state, Some(&system_home), Duration::from_secs(180)).await?;
        return detected_ambient_account()?.ok_or(AppError::ClaudeAuthNotFound);
    }

    authenticate_managed_account(&login_state, Some(account_id)).await
}

#[tauri::command]
pub(crate) async fn cancel_claude_account_login(
    login_state: State<'_, ClaudeLoginRunnerState>,
) -> Result<bool, AppError> {
    login_runner::cancel_login(&login_state).await
}

#[tauri::command]
pub(crate) fn remove_claude_account(_app: AppHandle, account_id: String) -> Result<(), AppError> {
    if account_id == "ambient" {
        return Err(AppError::ClaudeUnknownAccount(account_id));
    }
    let store = managed_account_store();
    let system_credentials = load_ambient_credentials().ok();
    let account = store.find_account(&account_id)?;
    if let Some(credentials) = system_credentials.as_ref() {
        let records = store.load_accounts()?;
        if live_system_account_id_for_credentials(&records, credentials) == Some(account.id) {
            return Err(AppError::ClaudeLiveAccountRemovalBlocked);
        }
    }
    store.remove_account(&account_id)
}

#[tauri::command]
pub(crate) fn set_system_claude_account(
    _app: AppHandle,
    account_id: String,
) -> Result<AccountSummary, AppError> {
    let store = managed_account_store();
    set_system_claude_account_in_store(&store, &account_id, &system_claude_home_path())
}

pub(crate) fn set_system_claude_account_in_store(
    store: &ManagedClaudeAccountStore,
    account_id: &str,
    system_home: &Path,
) -> Result<AccountSummary, AppError> {
    let account = store.find_account(account_id)?;
    let target_home = PathBuf::from(&account.home_path);
    let target_credentials = load_credentials_from_home(&target_home)?;
    if account.provider_account_id != target_credentials.provider_account_id() {
        return Err(AppError::ClaudeAccountIdentityMismatch);
    }

    preserve_system_account_before_overwrite(store, system_home)?;
    replace_credentials_from_home(&target_home, system_home)?;
    Ok(account.summary_with_status(true))
}

async fn authenticate_managed_account(
    login_state: &ClaudeLoginRunnerState,
    existing_account_id: Option<String>,
) -> Result<AccountSummary, AppError> {
    let store = managed_account_store();
    let preferred_id = existing_account_id
        .as_deref()
        .map(|value| {
            Uuid::parse_str(value).map_err(|_| AppError::ClaudeUnknownAccount(value.to_string()))
        })
        .transpose()?
        .unwrap_or_else(Uuid::new_v4);
    let system_mirror_home = if let Some(existing_account_id) = existing_account_id.as_deref() {
        let existing = store.find_account(existing_account_id)?;
        live_credential_mirror_home_for_account(&existing)?
    } else {
        None
    };
    let home_id = if existing_account_id.is_some() {
        Uuid::new_v4()
    } else {
        preferred_id
    };
    let home_path = store.create_home(home_id)?;

    let result = async {
        login_runner::run_login(login_state, Some(&home_path), Duration::from_secs(180)).await?;
        let credentials = load_credentials_from_home(&home_path)?;
        let (email, organization, plan) = usage_fetcher::fetch_cli_identity(&home_path)
            .await
            .unwrap_or((None, None, None));
        let account_label = organization.or(plan);
        let provider_account_id = credentials.provider_account_id();
        let live_import = if existing_account_id.is_none() {
            import_ambient_claude_account_if_available(&store)?
        } else {
            None
        };

        if existing_account_id.is_none() {
            match duplicate_identity_after_live_import(
                &store,
                email.as_deref(),
                provider_account_id.as_deref(),
                live_import.as_ref(),
            )? {
                Some(ClaudeDuplicateIdentity::NewlyImportedLive(account)) => {
                    let _ = store.remove_home_if_safe(&home_path);
                    return Ok((account, true));
                }
                Some(ClaudeDuplicateIdentity::Existing) => {
                    return Err(AppError::ClaudeAccountAlreadyExists);
                }
                None => {}
            }
        }

        let (account, replaced_home_paths) =
            upsert_authenticated_account_and_mirror_system_if_needed(
                &store,
                preferred_id,
                email,
                provider_account_id,
                account_label,
                home_path.clone(),
                system_mirror_home.as_deref(),
            )?;
        remove_replaced_homes(&store, replaced_home_paths);
        Ok::<(ManagedClaudeAccountRecord, bool), AppError>((account, system_mirror_home.is_some()))
    }
    .await;

    match result {
        Ok((account, is_live_system)) => Ok(account.summary_with_status(is_live_system)),
        Err(error) => {
            let _ = store.remove_home_if_safe(&home_path);
            Err(error)
        }
    }
}

fn upsert_authenticated_account_and_mirror_system_if_needed(
    store: &ManagedClaudeAccountStore,
    preferred_id: Uuid,
    email: Option<String>,
    provider_account_id: Option<String>,
    organization: Option<String>,
    home_path: PathBuf,
    system_mirror_home: Option<&Path>,
) -> Result<(ManagedClaudeAccountRecord, Vec<PathBuf>), AppError> {
    if let Some(mirror_home_path) = system_mirror_home {
        store.upsert_authenticated_account_and_then(
            preferred_id,
            email,
            provider_account_id,
            None,
            organization,
            home_path.clone(),
            |_| replace_credentials_from_home(&home_path, mirror_home_path),
        )
    } else {
        store.upsert_authenticated_account(
            preferred_id,
            email,
            provider_account_id,
            None,
            organization,
            home_path,
        )
    }
}

pub(crate) async fn load_fresh_credentials_for_account(
    account_id: &str,
) -> Result<ClaudeOAuthCredentials, AppError> {
    let (mut credentials, mirror_home_path, managed_record_id) = if account_id == "ambient" {
        (load_ambient_credentials()?, None, None)
    } else {
        let account = managed_account_store().find_account(account_id)?;
        let mirror_home_path = live_credential_mirror_home_for_account(&account)?;
        (
            load_credentials_from_home(&PathBuf::from(account.home_path))?,
            mirror_home_path,
            Some(account.id),
        )
    };

    if credentials.is_expired() {
        credentials = token_refresh::refresh(credentials).await?;
        save_credentials(&credentials)?;
        if let Some(mirror_home_path) = mirror_home_path {
            save_credentials_to_home(&credentials, mirror_home_path)?;
        }
        if let Some(record_id) = managed_record_id {
            managed_account_store()
                .update_account_provider_id(record_id, credentials.provider_account_id())?;
        }
    }

    Ok(credentials)
}

pub(crate) fn claude_home_for_usage_account(account_id: &str) -> Result<PathBuf, AppError> {
    if account_id == "ambient" {
        return Ok(system_claude_home_path());
    }

    let account = managed_account_store().find_account(account_id)?;
    Ok(PathBuf::from(account.home_path))
}

fn find_duplicate_managed_account(
    store: &ManagedClaudeAccountStore,
    email: Option<&str>,
    provider_account_id: Option<&str>,
) -> Result<Option<ManagedClaudeAccountRecord>, AppError> {
    Ok(store.load_accounts()?.into_iter().find(|account| {
        account_summary_identity_matches(&account.summary(), email, provider_account_id)
    }))
}

struct LiveClaudeImport {
    record: ManagedClaudeAccountRecord,
    created: bool,
}

enum ClaudeDuplicateIdentity {
    Existing,
    NewlyImportedLive(ManagedClaudeAccountRecord),
}

fn duplicate_identity_after_live_import(
    store: &ManagedClaudeAccountStore,
    email: Option<&str>,
    provider_account_id: Option<&str>,
    live_import: Option<&LiveClaudeImport>,
) -> Result<Option<ClaudeDuplicateIdentity>, AppError> {
    let Some(duplicate) = find_duplicate_managed_account(store, email, provider_account_id)? else {
        return Ok(None);
    };

    if live_import
        .filter(|import| import.created && import.record.id == duplicate.id)
        .is_some()
    {
        return Ok(Some(ClaudeDuplicateIdentity::NewlyImportedLive(duplicate)));
    }

    Ok(Some(ClaudeDuplicateIdentity::Existing))
}

fn import_ambient_claude_account_if_available(
    store: &ManagedClaudeAccountStore,
) -> Result<Option<LiveClaudeImport>, AppError> {
    match load_ambient_credentials() {
        Ok(credentials) => ensure_live_claude_account_imported(store, &credentials),
        Err(_) => Ok(None),
    }
}

fn ensure_live_claude_account_imported(
    store: &ManagedClaudeAccountStore,
    credentials: &ClaudeOAuthCredentials,
) -> Result<Option<LiveClaudeImport>, AppError> {
    let Some(provider_account_id) = credentials.provider_account_id() else {
        return Ok(None);
    };

    if let Some(existing) =
        store.find_matching_account(None, Some(provider_account_id.as_str()), None)?
    {
        let home_path = PathBuf::from(&existing.home_path);
        if canonical_or_original(&credentials.home_path)? != canonical_or_original(&home_path)? {
            replace_credentials_from_home(&credentials.home_path, &home_path)?;
        }
        let (record, replaced_home_paths) = store.upsert_authenticated_account(
            existing.id,
            None,
            Some(provider_account_id),
            None,
            credentials.plan_type(),
            home_path,
        )?;
        remove_replaced_homes(store, replaced_home_paths);
        return Ok(Some(LiveClaudeImport {
            record,
            created: false,
        }));
    }

    let preferred_id = Uuid::new_v4();
    let home_path = store.create_home(preferred_id)?;
    let result = (|| {
        replace_credentials_from_home(&credentials.home_path, &home_path)?;
        let (record, replaced_home_paths) = store.upsert_authenticated_account(
            preferred_id,
            None,
            Some(provider_account_id),
            None,
            credentials.plan_type(),
            home_path.clone(),
        )?;
        remove_replaced_homes(store, replaced_home_paths);
        Ok::<ManagedClaudeAccountRecord, AppError>(record)
    })();

    match result {
        Ok(record) => Ok(Some(LiveClaudeImport {
            record,
            created: true,
        })),
        Err(error) => {
            let _ = store.remove_home_if_safe(&home_path);
            Err(error)
        }
    }
}

fn canonical_or_original(path: &Path) -> Result<PathBuf, AppError> {
    path.canonicalize().or_else(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Ok(path.to_path_buf())
        } else {
            Err(AppError::ClaudeAccountStore(error.to_string()))
        }
    })
}

fn save_credentials_to_home(
    credentials: &ClaudeOAuthCredentials,
    home_path: PathBuf,
) -> Result<(), AppError> {
    if canonical_or_original(&credentials.home_path)? == canonical_or_original(&home_path)? {
        return Ok(());
    }

    let mut mirrored = credentials.clone();
    mirrored.home_path = home_path;
    save_credentials(&mirrored)
}

fn account_summary_identity_matches(
    account: &AccountSummary,
    email: Option<&str>,
    provider_account_id: Option<&str>,
) -> bool {
    if let Some(provider_account_id) = provider_account_id {
        return account.provider_account_id.as_deref() == Some(provider_account_id);
    }

    if account.provider_account_id.is_some() || account.workspace_account_id.is_some() {
        return false;
    }

    email
        .zip(account.email.as_deref())
        .map(|(left, right)| left.eq_ignore_ascii_case(right))
        .unwrap_or(false)
}

fn live_credential_mirror_home_for_account(
    account: &ManagedClaudeAccountRecord,
) -> Result<Option<PathBuf>, AppError> {
    let ambient = match load_ambient_credentials() {
        Ok(credentials) => credentials,
        Err(AppError::ClaudeAuthNotFound) => return Ok(None),
        Err(_) => return Ok(None),
    };
    if account.provider_account_id == ambient.provider_account_id() {
        Ok(Some(ambient.home_path))
    } else {
        Ok(None)
    }
}

fn live_system_account_id_for_credentials(
    records: &[ManagedClaudeAccountRecord],
    credentials: &ClaudeOAuthCredentials,
) -> Option<Uuid> {
    let provider_account_id = credentials.provider_account_id()?;
    records
        .iter()
        .find(|record| record.provider_account_id.as_deref() == Some(provider_account_id.as_str()))
        .map(|record| record.id)
}

fn preserve_system_account_before_overwrite(
    store: &ManagedClaudeAccountStore,
    system_home: &Path,
) -> Result<(), AppError> {
    let credentials = match load_credentials_from_home(system_home) {
        Ok(credentials) => credentials,
        Err(AppError::ClaudeAuthNotFound) => return Ok(()),
        Err(error) => return Err(error),
    };
    if ensure_live_claude_account_imported(store, &credentials)?.is_none() {
        return Err(AppError::ClaudeAccountStore(
            "current system Claude account has no stable OAuth identity; refusing to overwrite"
                .to_string(),
        ));
    }
    Ok(())
}

fn remove_replaced_homes(store: &ManagedClaudeAccountStore, home_paths: Vec<PathBuf>) {
    for path in home_paths {
        let _ = store.remove_home_if_safe(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn account_summary(
        email: Option<&str>,
        provider_account_id: Option<&str>,
        workspace_account_id: Option<&str>,
    ) -> AccountSummary {
        AccountSummary::managed(
            Uuid::new_v4().to_string(),
            email.map(str::to_string),
            provider_account_id.map(str::to_string),
            workspace_account_id.map(str::to_string),
            None,
            "/tmp/claude".to_string(),
            1,
            2,
            Some(3),
            false,
        )
    }

    fn account_record(
        email: Option<&str>,
        provider_account_id: Option<&str>,
    ) -> ManagedClaudeAccountRecord {
        ManagedClaudeAccountRecord {
            id: Uuid::new_v4(),
            email: email.map(str::to_string),
            provider_account_id: provider_account_id.map(str::to_string),
            workspace_account_id: None,
            workspace_label: None,
            home_path: "/tmp/claude".to_string(),
            created_at: 1,
            updated_at: 2,
            last_authenticated_at: Some(3),
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("wovo-claude-{name}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn temp_store(name: &str) -> (ManagedClaudeAccountStore, PathBuf) {
        let root = temp_root(name);
        (ManagedClaudeAccountStore::new(root.clone()), root)
    }

    fn write_claude_credentials(home: &Path, access_token: &str, refresh_token: &str) {
        fs::create_dir_all(home).unwrap();
        let contents = serde_json::json!({
            "claudeAiOauth": {
                "accessToken": access_token,
                "refreshToken": refresh_token,
                "subscriptionType": "max"
            }
        });
        fs::write(
            home.join(".credentials.json"),
            serde_json::to_vec_pretty(&contents).unwrap(),
        )
        .unwrap();
    }

    fn claude_credentials(
        home_path: PathBuf,
        access_token: &str,
        refresh_token: &str,
    ) -> ClaudeOAuthCredentials {
        ClaudeOAuthCredentials {
            access_token: access_token.to_string(),
            refresh_token: Some(refresh_token.to_string()),
            expires_at: None,
            scopes: Vec::new(),
            rate_limit_tier: None,
            subscription_type: Some("max".to_string()),
            client_id: None,
            home_path,
        }
    }

    #[test]
    fn add_identity_match_uses_provider_id_before_email() {
        let account = account_summary(Some("user@example.com"), Some("provider-1"), None);

        assert!(!account_summary_identity_matches(
            &account,
            Some("USER@example.com"),
            Some("provider-2"),
        ));
        assert!(account_summary_identity_matches(
            &account,
            Some("other@example.com"),
            Some("provider-1"),
        ));
    }

    #[test]
    fn add_identity_match_falls_back_to_email_only_without_stable_ids() {
        let legacy = account_summary(Some("user@example.com"), None, None);
        let workspace = account_summary(Some("user@example.com"), None, Some("workspace-1"));

        assert!(account_summary_identity_matches(
            &legacy,
            Some("USER@example.com"),
            None,
        ));
        assert!(!account_summary_identity_matches(
            &workspace,
            Some("USER@example.com"),
            None,
        ));
    }

    #[test]
    fn ambient_credentials_absent_only_when_not_found() {
        let ambient =
            optional_ambient_credentials(Err(AppError::ClaudeAuthNotFound), false).unwrap();

        assert!(ambient.is_none());
    }

    #[test]
    fn ambient_credentials_error_propagates_malformed_state_without_managed_accounts() {
        let error = optional_ambient_credentials(
            Err(AppError::ClaudeAuthDecode("bad json".to_string())),
            false,
        )
        .unwrap_err();

        assert!(matches!(error, AppError::ClaudeAuthDecode(_)));
    }

    #[test]
    fn bad_ambient_credentials_do_not_hide_managed_accounts() {
        let record = account_record(Some("user@example.com"), Some("provider-1"));
        let account_id = record.id.to_string();

        let summaries = summaries_for_claude_accounts(
            vec![record],
            Err(AppError::ClaudeAuthDecode("bad json".to_string())),
        )
        .unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, account_id);
        assert!(!summaries[0].is_live_system);
    }

    #[test]
    fn cli_only_ambient_credentials_are_listed_without_oauth_payload() {
        let system_home = temp_root("cli-only-system-home");
        fs::write(
            system_home.join(".credentials.json"),
            r#"{"apiKeyHelper": true}"#,
        )
        .unwrap();

        let summaries = summaries_for_claude_accounts_with_system_home(
            Vec::new(),
            Err(AppError::ClaudeMissingTokens),
            &system_home,
        )
        .unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "ambient");
        assert_eq!(summaries[0].label, "Claude CLI");
        assert_eq!(
            summaries[0].home_path,
            system_home.to_string_lossy().to_string()
        );
        assert!(summaries[0].is_live_system);

        let _ = fs::remove_dir_all(system_home);
    }

    #[test]
    fn malformed_oauth_payload_still_errors_without_managed_accounts() {
        let system_home = temp_root("malformed-oauth-system-home");
        fs::write(
            system_home.join(".credentials.json"),
            r#"{"claudeAiOauth": {"refreshToken": "refresh"}}"#,
        )
        .unwrap();

        let error = summaries_for_claude_accounts_with_system_home(
            Vec::new(),
            Err(AppError::ClaudeMissingTokens),
            &system_home,
        )
        .unwrap_err();

        assert!(matches!(error, AppError::ClaudeMissingTokens));

        let _ = fs::remove_dir_all(system_home);
    }

    #[test]
    fn live_claude_account_not_yet_stored_is_imported() {
        let (store, store_root) = temp_store("live-import-store");
        let source_home = temp_root("live-import-source");
        write_claude_credentials(&source_home, "access", "refresh");
        let credentials = claude_credentials(source_home.clone(), "access", "refresh");

        let imported = ensure_live_claude_account_imported(&store, &credentials)
            .unwrap()
            .unwrap();
        let loaded = store.load_accounts().unwrap();
        let managed_home = PathBuf::from(&loaded[0].home_path);
        let managed_credentials = load_credentials_from_home(&managed_home).unwrap();

        assert!(imported.created);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, imported.record.id);
        assert_eq!(
            loaded[0].provider_account_id,
            credentials.provider_account_id()
        );
        assert_eq!(loaded[0].workspace_label.as_deref(), Some("Claude Max"));
        assert_eq!(
            managed_credentials.refresh_token.as_deref(),
            Some("refresh")
        );
        assert!(source_home.join(".credentials.json").exists());

        let _ = fs::remove_dir_all(store_root);
        let _ = fs::remove_dir_all(source_home);
    }

    #[test]
    fn newly_imported_live_account_is_returned_instead_of_duplicate() {
        let (store, store_root) = temp_store("live-duplicate-store");
        let source_home = temp_root("live-duplicate-source");
        write_claude_credentials(&source_home, "access", "refresh");
        let credentials = claude_credentials(source_home.clone(), "access", "refresh");
        let provider_account_id = credentials.provider_account_id();
        let live_import = ensure_live_claude_account_imported(&store, &credentials)
            .unwrap()
            .unwrap();

        let duplicate = duplicate_identity_after_live_import(
            &store,
            None,
            provider_account_id.as_deref(),
            Some(&live_import),
        )
        .unwrap();

        match duplicate {
            Some(ClaudeDuplicateIdentity::NewlyImportedLive(account)) => {
                assert_eq!(account.id, live_import.record.id);
            }
            _ => panic!("freshly imported live account should be returned"),
        }

        let _ = fs::remove_dir_all(store_root);
        let _ = fs::remove_dir_all(source_home);
    }
}
