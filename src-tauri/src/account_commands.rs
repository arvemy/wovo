use crate::codex::account_store::{
    default_wovo_codex_root, ManagedCodexAccountRecord, ManagedCodexAccountStore,
};
use crate::codex::auth_store::{
    detected_ambient_account, load_ambient_credentials, load_credentials_from_home,
    replace_auth_json_from_home, save_credentials, system_codex_home_path, CodexOAuthCredentials,
};
use crate::codex::login_runner::{self, LoginRunnerState};
use crate::codex::token_refresh;
use crate::codex::workspace_resolver::WorkspaceResolution;
use crate::domain::account::AccountSummary;
use crate::error::AppError;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

mod account_summaries;
mod identity_resolver;
mod live_account_importer;

use account_summaries::summarize_account_list;
#[cfg(test)]
use account_summaries::summarize_accounts;
use identity_resolver::{
    account_matches_identity, live_credential_mirror_home_for_account_with_ambient,
    live_system_account_id_for_credentials, managed_record_matches_credentials,
};
#[cfg(test)]
use live_account_importer::LiveCodexIdentity;
use live_account_importer::{
    ambient_summary_from_credentials, canonical_or_original, ensure_live_account_imported,
    ensure_live_account_imported_with_workspace, remove_replaced_homes,
    resolve_workspace_without_failing,
};

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

#[cfg(test)]
mod tests;
