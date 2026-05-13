use crate::account_commands::{
    list_codex_accounts, load_fresh_credentials_for_account, managed_account_store,
};
use crate::codex::auth_store::system_codex_home_path;
use crate::codex::settings::{self, CodexUsageSourceMode};
use crate::codex::usage_fetcher;
use crate::domain::usage::UsageSnapshot;
use crate::error::AppError;
use std::path::PathBuf;
use tauri::AppHandle;

#[tauri::command]
pub(crate) async fn refresh_codex_usage(
    app: AppHandle,
    account_id: String,
) -> Result<UsageSnapshot, AppError> {
    let mode = settings::load_settings()?.usage_source_mode;
    refresh_codex_usage_with_mode(&app, account_id, mode).await
}

#[tauri::command]
pub(crate) async fn refresh_all_usage(app: AppHandle) -> Result<Vec<UsageSnapshot>, AppError> {
    let accounts = list_codex_accounts(app.clone()).await?;
    let mut snapshots = Vec::new();
    for account in accounts {
        snapshots.push(refresh_codex_usage(app.clone(), account.id).await?);
    }
    Ok(snapshots)
}

pub(crate) async fn refresh_codex_usage_with_mode(
    app: &AppHandle,
    account_id: String,
    mode: CodexUsageSourceMode,
) -> Result<UsageSnapshot, AppError> {
    match mode {
        CodexUsageSourceMode::Oauth => refresh_codex_usage_via_oauth(app, account_id).await,
        CodexUsageSourceMode::Cli => refresh_codex_usage_via_cli(app, account_id).await,
        CodexUsageSourceMode::Auto => {
            match refresh_codex_usage_via_oauth(app, account_id.clone()).await {
                Ok(snapshot) => Ok(snapshot),
                Err(error) if oauth_error_allows_cli_fallback(&error) => {
                    refresh_codex_usage_via_cli(app, account_id).await
                }
                Err(error) => Err(error),
            }
        }
    }
}

pub(crate) async fn refresh_codex_usage_via_oauth(
    app: &AppHandle,
    account_id: String,
) -> Result<UsageSnapshot, AppError> {
    let credentials = load_fresh_credentials_for_account(app, &account_id).await?;
    usage_fetcher::fetch_oauth_usage(account_id, &credentials).await
}

pub(crate) async fn refresh_codex_usage_via_cli(
    app: &AppHandle,
    account_id: String,
) -> Result<UsageSnapshot, AppError> {
    let home_path = codex_home_for_usage_account(app, &account_id)?;
    usage_fetcher::fetch_cli_usage(account_id, &home_path).await
}

fn codex_home_for_usage_account(app: &AppHandle, account_id: &str) -> Result<PathBuf, AppError> {
    if account_id == "ambient" {
        return Ok(system_codex_home_path());
    }

    let account = managed_account_store(app)?.find_account(account_id)?;
    Ok(PathBuf::from(account.home_path))
}

pub(crate) fn oauth_error_allows_cli_fallback(error: &AppError) -> bool {
    match error {
        AppError::AuthNotFound | AppError::MissingTokens => true,
        AppError::TokenRefresh(message) => {
            let message = message.to_ascii_lowercase();
            message.contains("invalid_grant")
                || message.contains("unauthorized")
                || message.contains("revoked")
                || message.contains("expired")
                || message.contains("status 401")
                || message.contains("status 403")
        }
        AppError::UsageFetch(message) => {
            let message = message.to_ascii_lowercase();
            message.contains("status 401") || message.contains("status 403")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
