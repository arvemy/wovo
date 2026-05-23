use crate::account_commands::{
    list_codex_accounts, load_fresh_credentials_for_account, managed_account_store,
};
use crate::codex::auth_store::system_codex_home_path;
use crate::codex::runtime::fetch_cli_usage_with_runtime_auth;
use crate::codex::settings::{self, CodexUsageSourceMode};
use crate::codex::usage_fetcher;
use crate::domain::usage::UsageSnapshot;
use crate::error::AppError;
use crate::provider::{
    now_utc_timestamp, AccountRefreshDiagnostics, ProviderFetchAttempt, ProviderId,
    ProviderSourceMode,
};
use std::path::PathBuf;
use tauri::AppHandle;

pub(crate) struct UsageRefreshResult {
    pub(crate) snapshot: UsageSnapshot,
    pub(crate) diagnostics: AccountRefreshDiagnostics,
}

#[tauri::command]
pub(crate) async fn refresh_codex_usage(
    app: AppHandle,
    account_id: String,
) -> Result<UsageSnapshot, AppError> {
    let mode = settings::load_settings()?.usage_source_mode;
    refresh_codex_usage_with_mode(&app, account_id, mode)
        .await
        .map(|result| result.snapshot)
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
) -> Result<UsageRefreshResult, AppError> {
    match refresh_codex_usage_with_diagnostics(app, account_id, mode).await {
        Ok(result) => Ok(result),
        Err((error, _diagnostics)) => Err(error),
    }
}

pub(crate) async fn refresh_codex_usage_with_diagnostics(
    app: &AppHandle,
    account_id: String,
    mode: CodexUsageSourceMode,
) -> Result<UsageRefreshResult, (AppError, AccountRefreshDiagnostics)> {
    let mut attempts = Vec::new();
    let result = match mode {
        CodexUsageSourceMode::Oauth => {
            refresh_codex_usage_via_oauth_with_attempt(app, account_id, &mut attempts).await
        }
        CodexUsageSourceMode::Cli => {
            refresh_codex_usage_via_cli_with_attempt(app, account_id, &mut attempts).await
        }
        CodexUsageSourceMode::Auto => {
            let oauth_result =
                refresh_codex_usage_via_oauth_with_attempt(app, account_id.clone(), &mut attempts)
                    .await;
            match oauth_result {
                Ok(snapshot) => Ok(snapshot),
                Err(error) if oauth_error_allows_cli_fallback(&error) => {
                    if let Some(last) = attempts.last_mut() {
                        last.status = crate::provider::ProviderFetchAttemptStatus::Fallback;
                    }
                    refresh_codex_usage_via_cli_with_attempt(app, account_id, &mut attempts).await
                }
                Err(error) => Err(error),
            }
        }
    };

    let diagnostics = AccountRefreshDiagnostics::from_attempts(attempts);
    match result {
        Ok(mut snapshot) => {
            snapshot.fetch_attempts = diagnostics.attempts.clone();
            snapshot.source_mode = snapshot
                .source_mode
                .or_else(|| source_mode_from_source(&snapshot.source));
            Ok(UsageRefreshResult {
                snapshot,
                diagnostics,
            })
        }
        Err(error) => Err((error, diagnostics)),
    }
}

async fn refresh_codex_usage_via_oauth_with_attempt(
    app: &AppHandle,
    account_id: String,
    attempts: &mut Vec<ProviderFetchAttempt>,
) -> Result<UsageSnapshot, AppError> {
    let started_at = now_utc_timestamp();
    let result = refresh_codex_usage_via_oauth(app, account_id).await;
    record_attempt(
        attempts,
        ProviderSourceMode::Oauth,
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

async fn refresh_codex_usage_via_cli_with_attempt(
    app: &AppHandle,
    account_id: String,
    attempts: &mut Vec<ProviderFetchAttempt>,
) -> Result<UsageSnapshot, AppError> {
    let started_at = now_utc_timestamp();
    let result = refresh_codex_usage_via_cli(app, account_id).await;
    record_attempt(
        attempts,
        ProviderSourceMode::Cli,
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

fn record_attempt(
    attempts: &mut Vec<ProviderFetchAttempt>,
    source_mode: ProviderSourceMode,
    started_at: i64,
    result: Result<(), &AppError>,
) {
    match result {
        Ok(()) => attempts.push(ProviderFetchAttempt::success(
            ProviderId::Codex,
            source_mode,
            started_at,
        )),
        Err(error) => attempts.push(ProviderFetchAttempt::failed(
            ProviderId::Codex,
            source_mode,
            started_at,
            error,
        )),
    }
}

fn source_mode_from_source(source: &str) -> Option<ProviderSourceMode> {
    match source {
        "oauth" => Some(ProviderSourceMode::Oauth),
        "cli" => Some(ProviderSourceMode::Cli),
        "cached" => Some(ProviderSourceMode::Cached),
        _ => None,
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
    if account_id == "ambient" {
        let home_path = system_codex_home_path();
        return usage_fetcher::fetch_cli_usage(account_id, &home_path).await;
    }

    let account = managed_account_store(app)?.find_account(&account_id)?;
    let home_path = PathBuf::from(account.home_path);
    fetch_cli_usage_with_runtime_auth(
        account_id,
        home_path,
        |account_id, runtime_home| async move {
            usage_fetcher::fetch_cli_usage(account_id, &runtime_home).await
        },
    )
    .await
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
