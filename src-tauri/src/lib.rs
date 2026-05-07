mod codex;
mod domain;
mod error;

use codex::account_store::{ManagedCodexAccountRecord, ManagedCodexAccountStore};
use codex::auth_store::{
    detected_ambient_account, load_ambient_credentials, load_credentials_from_home,
    save_credentials, CodexOAuthCredentials,
};
use codex::login_runner::{self, LoginRunnerState};
use codex::token_refresh;
use codex::usage_fetcher;
use domain::account::AccountSummary;
use domain::usage::UsageSnapshot;
use error::AppError;
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

#[tauri::command]
fn get_detected_codex_account() -> Result<Option<AccountSummary>, AppError> {
    detected_ambient_account()
}

#[tauri::command]
fn list_codex_accounts(app: AppHandle) -> Result<Vec<AccountSummary>, AppError> {
    let mut accounts = managed_account_store(&app)?.load_summaries()?;
    match detected_ambient_account() {
        Ok(Some(ambient)) => accounts.insert(0, ambient),
        Ok(None) => {}
        Err(error) if accounts.is_empty() => return Err(error),
        Err(_) => {}
    }
    Ok(accounts)
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
        login_runner::run_login(&login_state, None, Duration::from_secs(120)).await?;
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
    managed_account_store(&app)?.remove_account(&account_id)
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
    if let Some(existing_account_id) = existing_account_id.as_deref() {
        store.find_account(existing_account_id)?;
    }
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

        let (account, replaced_home_paths) = store.upsert_authenticated_account(
            preferred_id,
            email,
            provider_account_id,
            home_path.clone(),
        )?;
        remove_replaced_homes(&store, replaced_home_paths);
        Ok::<ManagedCodexAccountRecord, AppError>(account)
    }
    .await;

    match result {
        Ok(account) => Ok(account.summary()),
        Err(error) => {
            let _ = store.remove_home_if_safe(&home_path);
            Err(error)
        }
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
    if let (Some(existing), Some(candidate)) =
        (account.provider_account_id.as_deref(), provider_account_id)
    {
        return existing == candidate;
    }

    if let (Some(existing), Some(candidate)) = (account.email.as_deref(), email) {
        return existing.eq_ignore_ascii_case(candidate);
    }

    false
}

async fn load_fresh_credentials_for_account(
    app: &AppHandle,
    account_id: &str,
) -> Result<CodexOAuthCredentials, AppError> {
    let mut credentials = if account_id == "ambient" {
        load_ambient_credentials()?
    } else {
        let account = managed_account_store(app)?.find_account(account_id)?;
        load_credentials_from_home(&PathBuf::from(account.home_path))?
    };

    if credentials.needs_refresh() {
        credentials = token_refresh::refresh(credentials).await?;
        save_credentials(&credentials)?;
    }

    Ok(credentials)
}

fn managed_account_store(app: &AppHandle) -> Result<ManagedCodexAccountStore, AppError> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::AccountStore(error.to_string()))?;
    Ok(ManagedCodexAccountStore::new(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::AccountSourceKind;

    fn summary(email: Option<&str>, provider_account_id: Option<&str>) -> AccountSummary {
        AccountSummary {
            id: "test".to_string(),
            label: email.or(provider_account_id).unwrap_or("test").to_string(),
            email: email.map(str::to_string),
            provider_account_id: provider_account_id.map(str::to_string),
            home_path: "/tmp/codex".to_string(),
            source: AccountSourceKind::Managed,
            authenticated: true,
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
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(LoginRunnerState::default())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            add_codex_account,
            cancel_codex_account_login,
            get_detected_codex_account,
            list_codex_accounts,
            reauthenticate_codex_account,
            remove_codex_account,
            refresh_codex_usage,
            refresh_all_usage
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
