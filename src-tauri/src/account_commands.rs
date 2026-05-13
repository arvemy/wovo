use crate::codex::account_store::{
    default_wovo_codex_root, ManagedCodexAccountRecord, ManagedCodexAccountStore,
};
use crate::codex::auth_store::{
    detected_ambient_account, load_ambient_credentials, load_credentials_from_home,
    replace_auth_json_from_home, save_credentials, system_codex_home_path, CodexOAuthCredentials,
};
use crate::codex::login_runner::{self, LoginRunnerState};
use crate::codex::token_refresh;
use crate::codex::workspace_resolver::{self, WorkspaceResolution};
use crate::domain::account::AccountSummary;
use crate::error::AppError;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

#[tauri::command]
pub(crate) fn get_detected_codex_account() -> Result<Option<AccountSummary>, AppError> {
    detected_ambient_account()
}

#[tauri::command]
pub(crate) async fn list_codex_accounts(app: AppHandle) -> Result<Vec<AccountSummary>, AppError> {
    let store = managed_account_store(&app)?;
    store.cleanup_legacy_current_state()?;
    let mut ambient_fallback = None;
    let live_identity = match load_ambient_credentials() {
        Ok(credentials) => {
            let live_identity = ensure_live_account_imported(&store, &credentials).await?;
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
pub(crate) async fn add_codex_account(
    app: AppHandle,
    login_state: State<'_, LoginRunnerState>,
) -> Result<AccountSummary, AppError> {
    authenticate_managed_account(app, &login_state, None).await
}

#[tauri::command]
pub(crate) async fn reauthenticate_codex_account(
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
pub(crate) async fn cancel_codex_account_login(
    login_state: State<'_, LoginRunnerState>,
) -> Result<bool, AppError> {
    login_runner::cancel_login(&login_state).await
}

#[tauri::command]
pub(crate) fn remove_codex_account(app: AppHandle, account_id: String) -> Result<(), AppError> {
    if account_id == "ambient" {
        return Err(AppError::UnknownAccount(account_id));
    }
    let store = managed_account_store(&app)?;
    let system_credentials = load_ambient_credentials().ok();
    remove_codex_account_from_store(&store, &account_id, system_credentials.as_ref())
}

pub(crate) fn remove_codex_account_from_store(
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
pub(crate) fn set_system_codex_account(
    app: AppHandle,
    account_id: String,
) -> Result<AccountSummary, AppError> {
    let store = managed_account_store(&app)?;
    set_system_codex_account_in_store(&store, &account_id, &system_codex_home_path())
}

pub(crate) fn set_system_codex_account_in_store(
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
        let workspace = resolve_workspace_without_failing(&credentials).await;

        if existing_account_id.is_none()
            && identity_already_exists(
                &app,
                email.as_deref(),
                provider_account_id.as_deref(),
                workspace
                    .as_ref()
                    .and_then(|workspace| workspace.account_id.as_deref()),
            )?
        {
            return Err(AppError::AccountAlreadyExists);
        }

        let (account, replaced_home_paths) =
            upsert_authenticated_account_and_mirror_system_if_needed(
                &store,
                preferred_id,
                email,
                provider_account_id,
                workspace,
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
    workspace: Option<WorkspaceResolution>,
    home_path: PathBuf,
    system_mirror_home: Option<&Path>,
) -> Result<(ManagedCodexAccountRecord, Vec<PathBuf>), AppError> {
    let workspace_account_id = workspace
        .as_ref()
        .and_then(|workspace| workspace.account_id.clone());
    let workspace_label = workspace.and_then(|workspace| workspace.label);
    if let Some(mirror_home_path) = system_mirror_home {
        store.upsert_authenticated_account_with_workspace_and_then(
            preferred_id,
            email,
            provider_account_id,
            workspace_account_id,
            workspace_label,
            home_path.clone(),
            |_| replace_auth_json_from_home(&home_path, mirror_home_path),
        )
    } else {
        store.upsert_authenticated_account_with_workspace(
            preferred_id,
            email,
            provider_account_id,
            workspace_account_id,
            workspace_label,
            home_path,
        )
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
    workspace_account_id: Option<&str>,
) -> Result<bool, AppError> {
    let mut accounts = managed_account_store(app)?.load_summaries()?;
    if let Ok(Some(ambient)) = detected_ambient_account() {
        accounts.push(ambient);
    }

    Ok(accounts.iter().any(|account| {
        account_matches_identity(account, email, provider_account_id, workspace_account_id)
    }))
}

fn account_matches_identity(
    account: &AccountSummary,
    email: Option<&str>,
    provider_account_id: Option<&str>,
    workspace_account_id: Option<&str>,
) -> bool {
    identities_match(
        account.email.as_deref(),
        account.provider_account_id.as_deref(),
        account.workspace_account_id.as_deref(),
        email,
        provider_account_id,
        workspace_account_id,
    )
}

pub(crate) async fn load_fresh_credentials_for_account(
    app: &AppHandle,
    account_id: &str,
) -> Result<CodexOAuthCredentials, AppError> {
    let (mut credentials, mirror_home_path, managed_record_id) = if account_id == "ambient" {
        (load_ambient_credentials()?, None, None)
    } else {
        let account = managed_account_store(app)?.find_account(account_id)?;
        let mirror_home_path = live_credential_mirror_home_for_account(&account)?;
        (
            load_credentials_from_home(&PathBuf::from(account.home_path))?,
            mirror_home_path,
            Some(account.id),
        )
    };

    let mut refreshed = false;
    if credentials.needs_refresh() {
        credentials = token_refresh::refresh(credentials).await?;
        save_credentials(&credentials)?;
        if let Some(mirror_home_path) = mirror_home_path {
            save_credentials_to_home(&credentials, mirror_home_path)?;
        }
        refreshed = true;
    }

    if refreshed {
        if let Some(record_id) = managed_record_id {
            refresh_account_workspace_if_available(app, record_id, &credentials).await;
        }
    }

    Ok(credentials)
}

async fn refresh_account_workspace_if_available(
    app: &AppHandle,
    account_id: Uuid,
    credentials: &CodexOAuthCredentials,
) {
    let Some(workspace) = resolve_workspace_without_failing(credentials).await else {
        return;
    };
    if workspace.account_id.is_none() && workspace.label.is_none() {
        return;
    }
    if let Ok(store) = managed_account_store(app) {
        let _ = store.update_account_workspace(account_id, workspace.account_id, workspace.label);
    }
}

async fn resolve_workspace_without_failing(
    credentials: &CodexOAuthCredentials,
) -> Option<WorkspaceResolution> {
    workspace_resolver::resolve_workspace(credentials)
        .await
        .ok()
        .flatten()
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
        account.workspace_account_id.as_deref(),
        email.as_deref(),
        provider_account_id.as_deref(),
        None,
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

    let Some(identity) = ensure_live_account_imported_with_workspace(store, &credentials, None)?
    else {
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

pub(crate) fn managed_account_store(app: &AppHandle) -> Result<ManagedCodexAccountStore, AppError> {
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
    workspace_account_id: Option<String>,
    record: Option<ManagedCodexAccountRecord>,
}

fn ambient_summary_from_credentials(credentials: &CodexOAuthCredentials) -> AccountSummary {
    AccountSummary::ambient(
        credentials.home_path.to_string_lossy().to_string(),
        credentials.email(),
        credentials.provider_account_id(),
        None,
        None,
    )
}

async fn ensure_live_account_imported(
    store: &ManagedCodexAccountStore,
    credentials: &CodexOAuthCredentials,
) -> Result<Option<LiveCodexIdentity>, AppError> {
    let workspace = resolve_workspace_without_failing(credentials).await;
    ensure_live_account_imported_with_workspace(store, credentials, workspace)
}

fn ensure_live_account_imported_with_workspace(
    store: &ManagedCodexAccountStore,
    credentials: &CodexOAuthCredentials,
    workspace: Option<WorkspaceResolution>,
) -> Result<Option<LiveCodexIdentity>, AppError> {
    let email = credentials.email();
    let provider_account_id = credentials.provider_account_id();
    let workspace_account_id = workspace
        .as_ref()
        .and_then(|workspace| workspace.account_id.clone());
    let workspace_label = workspace
        .as_ref()
        .and_then(|workspace| workspace.label.clone());
    if email.is_none() && provider_account_id.is_none() && workspace_account_id.is_none() {
        return Ok(None);
    }

    if let Some(existing) = store.find_matching_account(
        email.as_deref(),
        provider_account_id.as_deref(),
        workspace_account_id.as_deref(),
    )? {
        let record = sync_live_account_record(
            store,
            credentials,
            existing.id,
            PathBuf::from(&existing.home_path),
            email.clone(),
            provider_account_id.clone(),
            workspace.clone(),
        )?;
        return Ok(Some(LiveCodexIdentity {
            email,
            provider_account_id,
            workspace_account_id,
            record: Some(record),
        }));
    }

    let preferred_id = Uuid::new_v4();
    let home_path = store.create_home(preferred_id)?;
    let result = (|| {
        store.import_auth_from_home(&credentials.home_path, &home_path)?;
        let (account, replaced_home_paths) = store.upsert_authenticated_account_with_workspace(
            preferred_id,
            email.clone(),
            provider_account_id.clone(),
            workspace_account_id.clone(),
            workspace_label.clone(),
            home_path.clone(),
        )?;
        remove_replaced_homes(store, replaced_home_paths);
        Ok::<ManagedCodexAccountRecord, AppError>(account)
    })();

    match result {
        Ok(record) => Ok(Some(LiveCodexIdentity {
            email,
            provider_account_id,
            workspace_account_id,
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
    workspace: Option<WorkspaceResolution>,
) -> Result<ManagedCodexAccountRecord, AppError> {
    if canonical_or_original(&credentials.home_path)? != canonical_or_original(&home_path)? {
        store.import_auth_from_home(&credentials.home_path, &home_path)?;
    }
    let workspace_account_id = workspace
        .as_ref()
        .and_then(|workspace| workspace.account_id.clone());
    let workspace_label = workspace.and_then(|workspace| workspace.label);
    let (account, replaced_home_paths) = store.upsert_authenticated_account_with_workspace(
        preferred_id,
        email,
        provider_account_id,
        workspace_account_id,
        workspace_label,
        home_path,
    )?;
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
    let duplicate_emails = duplicate_emails(&records);
    records.sort_by(|left, right| {
        let left_system = live_system_account_id == Some(left.id);
        let right_system = live_system_account_id == Some(right.id);
        right_system
            .cmp(&left_system)
            .then_with(|| left.email.cmp(&right.email))
            .then_with(|| left.workspace_account_id.cmp(&right.workspace_account_id))
            .then_with(|| left.provider_account_id.cmp(&right.provider_account_id))
            .then_with(|| left.id.cmp(&right.id))
    });

    records
        .into_iter()
        .map(|record| {
            let is_live_system = live_system_account_id == Some(record.id);
            let duplicate_email = record
                .email
                .as_deref()
                .map(|email| duplicate_emails.contains(email))
                .unwrap_or(false);
            let mut summary = record.summary_with_status(is_live_system);
            summary.label = display_label_for_record(&record, duplicate_email);
            summary
        })
        .collect()
}

fn duplicate_emails(records: &[ManagedCodexAccountRecord]) -> std::collections::HashSet<String> {
    let mut counts = std::collections::HashMap::<String, usize>::new();
    for record in records {
        if let Some(email) = record.email.as_deref() {
            *counts.entry(email.to_string()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter_map(|(email, count)| (count > 1).then_some(email))
        .collect()
}

fn display_label_for_record(record: &ManagedCodexAccountRecord, duplicate_email: bool) -> String {
    let identity_id = record_identity_id(record);
    let workspace_label = record.workspace_label.as_deref();
    let is_personal = workspace_label
        .map(|label| label.eq_ignore_ascii_case("personal"))
        .unwrap_or(true);

    if let Some(email) = record.email.as_deref() {
        if duplicate_email || !is_personal {
            let suffix = workspace_label
                .filter(|label| !label.trim().is_empty())
                .map(str::to_string)
                .or_else(|| identity_id.map(short_account_id))
                .unwrap_or_else(|| "workspace".to_string());
            return format!("{email} - {suffix}");
        }
        return email.to_string();
    }

    workspace_label
        .map(str::to_string)
        .or_else(|| identity_id.map(str::to_string))
        .unwrap_or_else(|| "Managed Codex account".to_string())
}

fn short_account_id(account_id: &str) -> String {
    let trimmed = account_id.trim();
    let compact = trimmed.strip_prefix("account-").unwrap_or(trimmed);
    let short: String = compact.chars().take(8).collect();
    if short.is_empty() {
        trimmed.chars().take(8).collect()
    } else {
        short
    }
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
        live_identity.workspace_account_id.as_deref(),
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
        None,
    )
}

fn live_system_account_id(
    records: &[ManagedCodexAccountRecord],
    email: Option<&str>,
    provider_account_id: Option<&str>,
    workspace_account_id: Option<&str>,
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

    if let Some(workspace_account_id) = workspace_account_id {
        if let Some(record) = records
            .iter()
            .find(|record| record.workspace_account_id.as_deref() == Some(workspace_account_id))
        {
            return Some(record.id);
        }

        return provider_account_id.and_then(|provider_account_id| {
            records
                .iter()
                .find(|record| {
                    record.workspace_account_id.is_none()
                        && record.provider_account_id.as_deref() == Some(provider_account_id)
                })
                .map(|record| record.id)
        });
    }

    if let Some(provider_account_id) = provider_account_id {
        return records
            .iter()
            .find(|record| record.provider_account_id.as_deref() == Some(provider_account_id))
            .map(|record| record.id);
    }

    records
        .iter()
        .find(|record| {
            record.workspace_account_id.is_none()
                && record.provider_account_id.is_none()
                && emails_match(record.email.as_deref(), email)
        })
        .map(|record| record.id)
}

fn identities_match(
    existing_email: Option<&str>,
    existing_provider_account_id: Option<&str>,
    existing_workspace_account_id: Option<&str>,
    candidate_email: Option<&str>,
    candidate_provider_account_id: Option<&str>,
    candidate_workspace_account_id: Option<&str>,
) -> bool {
    if let Some(candidate_workspace_account_id) = candidate_workspace_account_id {
        if existing_workspace_account_id == Some(candidate_workspace_account_id) {
            return true;
        }
        return existing_workspace_account_id.is_none()
            && candidate_provider_account_id.is_some()
            && existing_provider_account_id == candidate_provider_account_id;
    }

    if let Some(candidate_provider_account_id) = candidate_provider_account_id {
        return existing_provider_account_id == Some(candidate_provider_account_id);
    }

    existing_workspace_account_id.is_none()
        && existing_provider_account_id.is_none()
        && emails_match(existing_email, candidate_email)
}

fn record_identity_id(record: &ManagedCodexAccountRecord) -> Option<&str> {
    record
        .workspace_account_id
        .as_deref()
        .or(record.provider_account_id.as_deref())
}

fn emails_match(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
