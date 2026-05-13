mod codex;
mod domain;
mod error;

use codex::account_store::{
    default_wovo_codex_root, ManagedCodexAccountRecord, ManagedCodexAccountStore,
};
use codex::auth_store::{
    detected_ambient_account, load_ambient_credentials, load_credentials_from_home,
    replace_auth_json_from_home, save_credentials, system_codex_home_path, CodexOAuthCredentials,
};
use codex::login_runner::{self, LoginRunnerState};
use codex::token_refresh;
use codex::usage_fetcher;
use domain::account::AccountSummary;
use domain::usage::UsageSnapshot;
use error::AppError;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

#[tauri::command]
fn get_detected_codex_account() -> Result<Option<AccountSummary>, AppError> {
    detected_ambient_account()
}

#[tauri::command]
fn list_codex_accounts(app: AppHandle) -> Result<Vec<AccountSummary>, AppError> {
    let store = managed_account_store(&app)?;
    store.cleanup_legacy_current_state()?;
    let mut ambient_fallback = None;
    let live_identity = match load_ambient_credentials() {
        Ok(credentials) => {
            let live_identity = ensure_live_account_imported(&store, &credentials)?;
            if live_identity
                .as_ref()
                .and_then(|identity| identity.record.as_ref())
                .is_none()
            {
                ambient_fallback = Some(ambient_summary_from_credentials(&credentials));
            }
            live_identity
        }
        Err(AppError::AuthNotFound) => None,
        Err(error) if store.load_accounts()?.is_empty() => return Err(error),
        Err(_) => None,
    };

    let records = store.load_accounts()?;
    Ok(summarize_account_list(
        records,
        live_identity.as_ref(),
        ambient_fallback,
    ))
}

#[tauri::command]
async fn add_codex_account(
    app: AppHandle,
    login_state: State<'_, LoginRunnerState>,
) -> Result<AccountSummary, AppError> {
    authenticate_managed_account(app, &login_state, None).await
}

#[tauri::command]
async fn reauthenticate_codex_account(
    app: AppHandle,
    login_state: State<'_, LoginRunnerState>,
    account_id: String,
) -> Result<AccountSummary, AppError> {
    if account_id == "ambient" {
        let system_home = system_codex_home_path();
        login_runner::run_login(&login_state, Some(&system_home), Duration::from_secs(120)).await?;
        return detected_ambient_account()?.ok_or(AppError::AuthNotFound);
    }

    authenticate_managed_account(app, &login_state, Some(account_id)).await
}

#[tauri::command]
async fn cancel_codex_account_login(
    login_state: State<'_, LoginRunnerState>,
) -> Result<bool, AppError> {
    login_runner::cancel_login(&login_state).await
}

#[tauri::command]
fn remove_codex_account(app: AppHandle, account_id: String) -> Result<(), AppError> {
    if account_id == "ambient" {
        return Err(AppError::UnknownAccount(account_id));
    }
    let store = managed_account_store(&app)?;
    let system_credentials = load_ambient_credentials().ok();
    remove_codex_account_from_store(&store, &account_id, system_credentials.as_ref())
}

fn remove_codex_account_from_store(
    store: &ManagedCodexAccountStore,
    account_id: &str,
    system_credentials: Option<&CodexOAuthCredentials>,
) -> Result<(), AppError> {
    let account = store.find_account(account_id)?;
    if let Some(credentials) = system_credentials {
        let records = store.load_accounts()?;
        if live_system_account_id_for_credentials(&records, credentials) == Some(account.id) {
            return Err(AppError::LiveAccountRemovalBlocked);
        }
    }
    store.remove_account(account_id)
}

#[tauri::command]
fn set_system_codex_account(
    app: AppHandle,
    account_id: String,
) -> Result<AccountSummary, AppError> {
    let store = managed_account_store(&app)?;
    set_system_codex_account_in_store(&store, &account_id, &system_codex_home_path())
}

fn set_system_codex_account_in_store(
    store: &ManagedCodexAccountStore,
    account_id: &str,
    system_home: &Path,
) -> Result<AccountSummary, AppError> {
    let account = store.find_account(account_id)?;
    let target_home = PathBuf::from(&account.home_path);
    let target_credentials = load_credentials_from_home(&target_home)?;
    if !managed_record_matches_credentials(&account, &target_credentials) {
        return Err(AppError::AccountIdentityMismatch);
    }

    preserve_system_account_before_overwrite(store, system_home)?;

    let account = store.find_account(account_id)?;
    let account_home = PathBuf::from(&account.home_path);
    replace_auth_json_from_home(&account_home, system_home)?;
    Ok(account.summary_with_status(true))
}

#[tauri::command]
async fn refresh_codex_usage(
    app: AppHandle,
    account_id: String,
) -> Result<UsageSnapshot, AppError> {
    let credentials = load_fresh_credentials_for_account(&app, &account_id).await?;
    usage_fetcher::fetch_usage(account_id, &credentials).await
}

#[tauri::command]
async fn refresh_all_usage(app: AppHandle) -> Result<Vec<UsageSnapshot>, AppError> {
    let accounts = list_codex_accounts(app.clone())?;
    let mut snapshots = Vec::new();
    for account in accounts {
        snapshots.push(refresh_codex_usage(app.clone(), account.id).await?);
    }
    Ok(snapshots)
}

async fn authenticate_managed_account(
    app: AppHandle,
    login_state: &LoginRunnerState,
    existing_account_id: Option<String>,
) -> Result<AccountSummary, AppError> {
    let store = managed_account_store(&app)?;
    let preferred_id = existing_account_id
        .as_deref()
        .map(|value| {
            Uuid::parse_str(value).map_err(|_| AppError::UnknownAccount(value.to_string()))
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
        login_runner::run_login(login_state, Some(&home_path), Duration::from_secs(120)).await?;
        let credentials = load_credentials_from_home(&home_path)?;
        let email = credentials.email();
        let provider_account_id = credentials.provider_account_id();

        if existing_account_id.is_none()
            && identity_already_exists(&app, email.as_deref(), provider_account_id.as_deref())?
        {
            return Err(AppError::AccountAlreadyExists);
        }

        let (account, replaced_home_paths) =
            upsert_authenticated_account_and_mirror_system_if_needed(
                &store,
                preferred_id,
                email,
                provider_account_id,
                home_path.clone(),
                system_mirror_home.as_deref(),
            )?;
        remove_replaced_homes(&store, replaced_home_paths);
        Ok::<ManagedCodexAccountRecord, AppError>(account)
    }
    .await;

    match result {
        Ok(account) => Ok(account.summary_with_status(system_mirror_home.is_some())),
        Err(error) => {
            let _ = store.remove_home_if_safe(&home_path);
            Err(error)
        }
    }
}

fn upsert_authenticated_account_and_mirror_system_if_needed(
    store: &ManagedCodexAccountStore,
    preferred_id: Uuid,
    email: Option<String>,
    provider_account_id: Option<String>,
    home_path: PathBuf,
    system_mirror_home: Option<&Path>,
) -> Result<(ManagedCodexAccountRecord, Vec<PathBuf>), AppError> {
    if let Some(mirror_home_path) = system_mirror_home {
        store.upsert_authenticated_account_and_then(
            preferred_id,
            email,
            provider_account_id,
            home_path.clone(),
            |_| replace_auth_json_from_home(&home_path, mirror_home_path),
        )
    } else {
        store.upsert_authenticated_account(preferred_id, email, provider_account_id, home_path)
    }
}

fn remove_replaced_homes(store: &ManagedCodexAccountStore, home_paths: Vec<PathBuf>) {
    for home_path in home_paths {
        let _ = store.remove_home_if_safe(&home_path);
    }
}

fn identity_already_exists(
    app: &AppHandle,
    email: Option<&str>,
    provider_account_id: Option<&str>,
) -> Result<bool, AppError> {
    let mut accounts = managed_account_store(app)?.load_summaries()?;
    if let Ok(Some(ambient)) = detected_ambient_account() {
        accounts.push(ambient);
    }

    Ok(accounts
        .iter()
        .any(|account| account_matches_identity(account, email, provider_account_id)))
}

fn account_matches_identity(
    account: &AccountSummary,
    email: Option<&str>,
    provider_account_id: Option<&str>,
) -> bool {
    identities_match(
        account.email.as_deref(),
        account.provider_account_id.as_deref(),
        email,
        provider_account_id,
    )
}

async fn load_fresh_credentials_for_account(
    app: &AppHandle,
    account_id: &str,
) -> Result<CodexOAuthCredentials, AppError> {
    let (mut credentials, mirror_home_path) = if account_id == "ambient" {
        (load_ambient_credentials()?, None)
    } else {
        let account = managed_account_store(app)?.find_account(account_id)?;
        let mirror_home_path = live_credential_mirror_home_for_account(&account)?;
        (
            load_credentials_from_home(&PathBuf::from(account.home_path))?,
            mirror_home_path,
        )
    };

    if credentials.needs_refresh() {
        credentials = token_refresh::refresh(credentials).await?;
        save_credentials(&credentials)?;
        if let Some(mirror_home_path) = mirror_home_path {
            save_credentials_to_home(&credentials, mirror_home_path)?;
        }
    }

    Ok(credentials)
}

fn save_credentials_to_home(
    credentials: &CodexOAuthCredentials,
    home_path: PathBuf,
) -> Result<(), AppError> {
    if canonical_or_original(&credentials.home_path)? == canonical_or_original(&home_path)? {
        return Ok(());
    }

    let mut mirrored = credentials.clone();
    mirrored.home_path = home_path;
    save_credentials(&mirrored)
}

fn live_credential_mirror_home_for_account(
    account: &ManagedCodexAccountRecord,
) -> Result<Option<PathBuf>, AppError> {
    let ambient = match load_ambient_credentials() {
        Ok(credentials) => credentials,
        Err(AppError::AuthNotFound) => return Ok(None),
        Err(_) => return Ok(None),
    };
    if live_credential_mirror_home_for_account_with_ambient(account, &ambient) {
        Ok(Some(ambient.home_path))
    } else {
        Ok(None)
    }
}

fn managed_record_matches_credentials(
    account: &ManagedCodexAccountRecord,
    credentials: &CodexOAuthCredentials,
) -> bool {
    let email = credentials.email();
    let provider_account_id = credentials.provider_account_id();
    identities_match(
        account.email.as_deref(),
        account.provider_account_id.as_deref(),
        email.as_deref(),
        provider_account_id.as_deref(),
    )
}

fn preserve_system_account_before_overwrite(
    store: &ManagedCodexAccountStore,
    system_home: &Path,
) -> Result<(), AppError> {
    let credentials = match load_credentials_from_home(system_home) {
        Ok(credentials) => credentials,
        Err(AppError::AuthNotFound) => return Ok(()),
        Err(error) => return Err(error),
    };

    let Some(identity) = ensure_live_account_imported(store, &credentials)? else {
        return Err(AppError::AccountStore(
            "current system Codex account has no stable OAuth identity; refusing to overwrite"
                .to_string(),
        ));
    };

    if identity.record.is_none() {
        return Err(AppError::AccountStore(
            "current system Codex account could not be preserved; refusing to overwrite"
                .to_string(),
        ));
    }

    Ok(())
}

fn live_credential_mirror_home_for_account_with_ambient(
    account: &ManagedCodexAccountRecord,
    ambient: &CodexOAuthCredentials,
) -> bool {
    live_system_account_id_for_credentials(std::slice::from_ref(account), ambient)
        == Some(account.id)
}

fn managed_account_store(app: &AppHandle) -> Result<ManagedCodexAccountStore, AppError> {
    let legacy_root = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::AccountStore(error.to_string()))?;
    Ok(ManagedCodexAccountStore::with_legacy_root(
        default_wovo_codex_root(),
        legacy_root,
    ))
}

#[derive(Debug, Clone)]
struct LiveCodexIdentity {
    email: Option<String>,
    provider_account_id: Option<String>,
    record: Option<ManagedCodexAccountRecord>,
}

fn ambient_summary_from_credentials(credentials: &CodexOAuthCredentials) -> AccountSummary {
    AccountSummary::ambient(
        credentials.home_path.to_string_lossy().to_string(),
        credentials.email(),
        credentials.provider_account_id(),
    )
}

fn ensure_live_account_imported(
    store: &ManagedCodexAccountStore,
    credentials: &CodexOAuthCredentials,
) -> Result<Option<LiveCodexIdentity>, AppError> {
    let email = credentials.email();
    let provider_account_id = credentials.provider_account_id();
    if email.is_none() && provider_account_id.is_none() {
        return Ok(None);
    }

    if let Some(existing) =
        store.find_matching_account(email.as_deref(), provider_account_id.as_deref())?
    {
        let record = sync_live_account_record(
            store,
            credentials,
            existing.id,
            PathBuf::from(&existing.home_path),
            email.clone(),
            provider_account_id.clone(),
        )?;
        return Ok(Some(LiveCodexIdentity {
            email,
            provider_account_id,
            record: Some(record),
        }));
    }

    let preferred_id = Uuid::new_v4();
    let home_path = store.create_home(preferred_id)?;
    let result = (|| {
        store.import_auth_from_home(&credentials.home_path, &home_path)?;
        let (account, replaced_home_paths) = store.upsert_authenticated_account(
            preferred_id,
            email.clone(),
            provider_account_id.clone(),
            home_path.clone(),
        )?;
        remove_replaced_homes(store, replaced_home_paths);
        Ok::<ManagedCodexAccountRecord, AppError>(account)
    })();

    match result {
        Ok(record) => Ok(Some(LiveCodexIdentity {
            email,
            provider_account_id,
            record: Some(record),
        })),
        Err(error) => {
            let _ = store.remove_home_if_safe(&home_path);
            Err(error)
        }
    }
}

fn sync_live_account_record(
    store: &ManagedCodexAccountStore,
    credentials: &CodexOAuthCredentials,
    preferred_id: Uuid,
    home_path: PathBuf,
    email: Option<String>,
    provider_account_id: Option<String>,
) -> Result<ManagedCodexAccountRecord, AppError> {
    if canonical_or_original(&credentials.home_path)? != canonical_or_original(&home_path)? {
        store.import_auth_from_home(&credentials.home_path, &home_path)?;
    }
    let (account, replaced_home_paths) =
        store.upsert_authenticated_account(preferred_id, email, provider_account_id, home_path)?;
    remove_replaced_homes(store, replaced_home_paths);
    Ok(account)
}

fn canonical_or_original(path: &std::path::Path) -> Result<PathBuf, AppError> {
    path.canonicalize().or_else(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Ok(path.to_path_buf())
        } else {
            Err(AppError::AccountStore(error.to_string()))
        }
    })
}

fn summarize_account_list(
    records: Vec<ManagedCodexAccountRecord>,
    live_identity: Option<&LiveCodexIdentity>,
    ambient_fallback: Option<AccountSummary>,
) -> Vec<AccountSummary> {
    let mut summaries = summarize_accounts(records, live_identity);
    if let Some(ambient) = ambient_fallback {
        summaries.push(ambient);
    }
    summaries
}

fn summarize_accounts(
    mut records: Vec<ManagedCodexAccountRecord>,
    live_identity: Option<&LiveCodexIdentity>,
) -> Vec<AccountSummary> {
    let live_system_account_id = live_system_account_id_for_identity(&records, live_identity);
    records.sort_by(|left, right| {
        let left_system = live_system_account_id == Some(left.id);
        let right_system = live_system_account_id == Some(right.id);
        right_system
            .cmp(&left_system)
            .then_with(|| left.email.cmp(&right.email))
            .then_with(|| left.provider_account_id.cmp(&right.provider_account_id))
            .then_with(|| left.id.cmp(&right.id))
    });

    records
        .into_iter()
        .map(|record| {
            let is_live_system = live_system_account_id == Some(record.id);
            record.summary_with_status(is_live_system)
        })
        .collect()
}

fn live_system_account_id_for_identity(
    records: &[ManagedCodexAccountRecord],
    live_identity: Option<&LiveCodexIdentity>,
) -> Option<Uuid> {
    let live_identity = live_identity?;
    let preferred_record_id = live_identity.record.as_ref().map(|record| record.id);
    live_system_account_id(
        records,
        live_identity.email.as_deref(),
        live_identity.provider_account_id.as_deref(),
        preferred_record_id,
    )
}

fn live_system_account_id_for_credentials(
    records: &[ManagedCodexAccountRecord],
    credentials: &CodexOAuthCredentials,
) -> Option<Uuid> {
    let email = credentials.email();
    let provider_account_id = credentials.provider_account_id();
    live_system_account_id(
        records,
        email.as_deref(),
        provider_account_id.as_deref(),
        None,
    )
}

fn live_system_account_id(
    records: &[ManagedCodexAccountRecord],
    email: Option<&str>,
    provider_account_id: Option<&str>,
    preferred_record_id: Option<Uuid>,
) -> Option<Uuid> {
    if let Some(preferred_record_id) = preferred_record_id {
        if records
            .iter()
            .any(|record| record.id == preferred_record_id)
        {
            return Some(preferred_record_id);
        }
    }

    if let Some(provider_account_id) = provider_account_id {
        if let Some(record) = records
            .iter()
            .find(|record| record.provider_account_id.as_deref() == Some(provider_account_id))
        {
            return Some(record.id);
        }
    }

    records
        .iter()
        .find(|record| {
            record.provider_account_id.is_none() && emails_match(record.email.as_deref(), email)
        })
        .map(|record| record.id)
}

fn identities_match(
    existing_email: Option<&str>,
    existing_provider_account_id: Option<&str>,
    candidate_email: Option<&str>,
    candidate_provider_account_id: Option<&str>,
) -> bool {
    if let Some(candidate_provider_account_id) = candidate_provider_account_id {
        return existing_provider_account_id == Some(candidate_provider_account_id);
    }

    existing_provider_account_id.is_none() && emails_match(existing_email, candidate_email)
}

fn emails_match(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
        _ => false,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(LoginRunnerState::default())
        .invoke_handler(tauri::generate_handler![
            add_codex_account,
            cancel_codex_account_login,
            get_detected_codex_account,
            list_codex_accounts,
            reauthenticate_codex_account,
            remove_codex_account,
            set_system_codex_account,
            refresh_codex_usage,
            refresh_all_usage
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::AccountSourceKind;
    use std::fs;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("wovo-{name}-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_auth(home: &Path, access_token: &str, account_id: &str) {
        fs::create_dir_all(home).unwrap();
        fs::write(
            home.join("auth.json"),
            format!(
                r#"{{"tokens":{{"access_token":"{access_token}","refresh_token":"refresh-{access_token}","account_id":"{account_id}"}}}}"#
            ),
        )
        .unwrap();
    }

    fn auth_credentials(home: &Path, account_id: &str) -> CodexOAuthCredentials {
        CodexOAuthCredentials {
            access_token: format!("access-{account_id}"),
            refresh_token: format!("refresh-{account_id}"),
            id_token: None,
            account_id: Some(account_id.to_string()),
            last_refresh: None,
            home_path: home.to_path_buf(),
        }
    }

    fn summary(email: Option<&str>, provider_account_id: Option<&str>) -> AccountSummary {
        AccountSummary {
            id: "test".to_string(),
            label: email.or(provider_account_id).unwrap_or("test").to_string(),
            email: email.map(str::to_string),
            provider_account_id: provider_account_id.map(str::to_string),
            home_path: "/tmp/codex".to_string(),
            source: AccountSourceKind::Managed,
            authenticated: true,
            is_live_system: false,
            can_set_system: true,
            can_remove: true,
            created_at: None,
            updated_at: None,
            last_authenticated_at: None,
        }
    }

    #[test]
    fn account_identity_match_uses_provider_account_id_first() {
        let account = summary(Some("same@example.com"), Some("account-1"));
        assert!(account_matches_identity(
            &account,
            Some("different@example.com"),
            Some("account-1")
        ));
        assert!(!account_matches_identity(
            &account,
            Some("same@example.com"),
            Some("account-2")
        ));
    }

    #[test]
    fn account_identity_match_falls_back_to_email() {
        let account = summary(Some("USER@example.com"), None);
        assert!(account_matches_identity(
            &account,
            Some("user@example.com"),
            None
        ));
    }

    #[test]
    fn account_identity_match_does_not_merge_same_email_different_provider_accounts() {
        let account = summary(Some("same@example.com"), Some("account-1"));
        assert!(!account_matches_identity(
            &account,
            Some("same@example.com"),
            Some("account-2")
        ));
    }

    #[test]
    fn account_identity_match_does_not_pairwise_match_provider_to_email_only_account() {
        let account = summary(Some("same@example.com"), None);
        assert!(!account_matches_identity(
            &account,
            Some("same@example.com"),
            Some("account-1")
        ));
    }

    #[test]
    fn live_account_matching_existing_record_is_summarized_once() {
        let id = Uuid::new_v4();
        let record = ManagedCodexAccountRecord {
            id,
            email: Some("user@example.com".to_string()),
            provider_account_id: Some("account-1".to_string()),
            home_path: "/tmp/home".to_string(),
            created_at: 1,
            updated_at: 2,
            last_authenticated_at: Some(3),
        };
        let live = LiveCodexIdentity {
            email: Some("USER@example.com".to_string()),
            provider_account_id: Some("account-1".to_string()),
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
            home_path: "/tmp/home".to_string(),
            created_at: 1,
            updated_at: 2,
            last_authenticated_at: Some(3),
        };
        let live = LiveCodexIdentity {
            email: Some("user@example.com".to_string()),
            provider_account_id: Some("account-1".to_string()),
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
            home_path: "/tmp/legacy".to_string(),
            created_at: 1,
            updated_at: 2,
            last_authenticated_at: Some(3),
        };
        let provider_record = ManagedCodexAccountRecord {
            id: provider_id,
            email: Some("user@example.com".to_string()),
            provider_account_id: Some("account-1".to_string()),
            home_path: "/tmp/provider".to_string(),
            created_at: 1,
            updated_at: 2,
            last_authenticated_at: Some(3),
        };
        let live = LiveCodexIdentity {
            email: Some("user@example.com".to_string()),
            provider_account_id: Some("account-1".to_string()),
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
    fn token_only_ambient_account_remains_listed() {
        let ambient = AccountSummary::ambient("/tmp/codex".to_string(), None, None);

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
        let store =
            ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
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
            record: Some(second.0),
        };

        let summaries = summarize_accounts(records, Some(&live));

        assert_eq!(summaries[0].id, second_id.to_string());
        assert!(summaries[0].is_live_system);

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(shared);
    }

    #[test]
    fn setting_managed_account_as_system_writes_auth_json() {
        let root = temp_root("set-system-root");
        let shared = temp_root("set-system-shared");
        let system_home = temp_root("set-system-home");
        let store =
            ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
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

        let summary =
            set_system_codex_account_in_store(&store, &id.to_string(), &system_home).unwrap();
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
        let store =
            ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
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
        let store =
            ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
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
        let store =
            ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
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
        let store =
            ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
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

        let error = remove_codex_account_from_store(&store, &id.to_string(), Some(&credentials))
            .unwrap_err();

        assert!(matches!(error, AppError::LiveAccountRemovalBlocked));
        assert_eq!(store.load_accounts().unwrap().len(), 1);

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(shared);
    }

    #[test]
    fn removing_non_system_managed_account_works() {
        let root = temp_root("remove-non-system-root");
        let shared = temp_root("remove-non-system-shared");
        let store =
            ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
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
    fn managed_record_matches_live_credentials_by_provider_id() {
        let id = Uuid::new_v4();
        let record = ManagedCodexAccountRecord {
            id,
            email: Some("different@example.com".to_string()),
            provider_account_id: Some("account-1".to_string()),
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
    fn live_credentials_fall_back_to_email_only_when_no_provider_record_matches() {
        let id = Uuid::new_v4();
        let record = ManagedCodexAccountRecord {
            id,
            email: Some("user@example.com".to_string()),
            provider_account_id: None,
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
            Some(id)
        );
    }

    #[test]
    fn live_matching_managed_account_uses_ambient_home_as_refresh_mirror() {
        let id = Uuid::new_v4();
        let account = ManagedCodexAccountRecord {
            id,
            email: Some("user@example.com".to_string()),
            provider_account_id: Some("account-1".to_string()),
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
        let store =
            ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
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
        let store =
            ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
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
                None,
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

        let live = ensure_live_account_imported(&store, &credentials)
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
        let store =
            ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
        let credentials = CodexOAuthCredentials {
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            id_token: Some("header.eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20ifQ.signature".to_string()),
            account_id: Some("account-1".to_string()),
            last_refresh: None,
            home_path: source_home.clone(),
        };

        let imported = ensure_live_account_imported(&store, &credentials)
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
}
