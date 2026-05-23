use crate::claude::account_commands::{
    claude_home_for_usage_account, list_claude_accounts_inner, load_fresh_credentials_for_account,
};
use crate::claude::settings::{self, ClaudeUsageSourceMode};
use crate::claude::usage_fetcher;
use crate::domain::usage::UsageSnapshot;
use crate::error::AppError;
use crate::provider::{
    now_utc_timestamp, AccountRefreshDiagnostics, ProviderFetchAttempt, ProviderId,
    ProviderSourceMode,
};
use tauri::AppHandle;

pub(crate) struct ClaudeUsageRefreshResult {
    pub(crate) snapshot: UsageSnapshot,
    pub(crate) diagnostics: AccountRefreshDiagnostics,
}

#[tauri::command]
pub(crate) async fn refresh_claude_usage(
    _app: AppHandle,
    account_id: String,
) -> Result<UsageSnapshot, AppError> {
    let mode = settings::load_settings()?.usage_source_mode;
    refresh_claude_usage_with_mode(account_id, mode)
        .await
        .map(|result| result.snapshot)
}

#[tauri::command]
pub(crate) async fn refresh_all_claude_usage(
    _app: AppHandle,
) -> Result<Vec<UsageSnapshot>, AppError> {
    let accounts = list_claude_accounts_inner()?;
    let mut snapshots = Vec::new();
    for account in accounts {
        snapshots.push(
            refresh_claude_usage_with_mode(
                account.id,
                settings::load_settings()?.usage_source_mode,
            )
            .await?
            .snapshot,
        );
    }
    Ok(snapshots)
}

pub(crate) async fn refresh_claude_usage_with_mode(
    account_id: String,
    mode: ClaudeUsageSourceMode,
) -> Result<ClaudeUsageRefreshResult, AppError> {
    match refresh_claude_usage_with_diagnostics(account_id, mode).await {
        Ok(result) => Ok(result),
        Err((error, _diagnostics)) => Err(error),
    }
}

pub(crate) async fn refresh_claude_usage_with_diagnostics(
    account_id: String,
    mode: ClaudeUsageSourceMode,
) -> Result<ClaudeUsageRefreshResult, (AppError, AccountRefreshDiagnostics)> {
    let mut attempts = Vec::new();
    let result = match mode {
        ClaudeUsageSourceMode::Oauth => {
            refresh_claude_usage_via_oauth_with_attempt(account_id, &mut attempts).await
        }
        ClaudeUsageSourceMode::Cli => {
            refresh_claude_usage_via_cli_with_attempt(account_id, &mut attempts).await
        }
        ClaudeUsageSourceMode::Auto => {
            let oauth_result =
                refresh_claude_usage_via_oauth_with_attempt(account_id.clone(), &mut attempts)
                    .await;
            match oauth_result {
                Ok(snapshot) => Ok(snapshot),
                Err(error) if oauth_error_allows_cli_fallback(&error) => {
                    if let Some(last) = attempts.last_mut() {
                        last.status = crate::provider::ProviderFetchAttemptStatus::Fallback;
                    }
                    refresh_claude_usage_via_cli_with_attempt(account_id, &mut attempts).await
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
            Ok(ClaudeUsageRefreshResult {
                snapshot,
                diagnostics,
            })
        }
        Err(error) => Err((error, diagnostics)),
    }
}

async fn refresh_claude_usage_via_oauth_with_attempt(
    account_id: String,
    attempts: &mut Vec<ProviderFetchAttempt>,
) -> Result<UsageSnapshot, AppError> {
    let started_at = now_utc_timestamp();
    let result = refresh_claude_usage_via_oauth(account_id).await;
    record_attempt(
        attempts,
        ProviderSourceMode::Oauth,
        started_at,
        result.as_ref().map(|_| ()),
    );
    result
}

async fn refresh_claude_usage_via_cli_with_attempt(
    account_id: String,
    attempts: &mut Vec<ProviderFetchAttempt>,
) -> Result<UsageSnapshot, AppError> {
    let started_at = now_utc_timestamp();
    let result = refresh_claude_usage_via_cli(account_id).await;
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
            ProviderId::Claude,
            source_mode,
            started_at,
        )),
        Err(error) => attempts.push(ProviderFetchAttempt::failed(
            ProviderId::Claude,
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

async fn refresh_claude_usage_via_oauth(account_id: String) -> Result<UsageSnapshot, AppError> {
    let credentials = load_fresh_credentials_for_account(&account_id).await?;
    usage_fetcher::fetch_oauth_usage(account_id, &credentials).await
}

async fn refresh_claude_usage_via_cli(account_id: String) -> Result<UsageSnapshot, AppError> {
    let home_path = claude_home_for_usage_account(&account_id)?;
    usage_fetcher::fetch_cli_usage(account_id, &home_path).await
}

pub(crate) fn oauth_error_allows_cli_fallback(error: &AppError) -> bool {
    match error {
        AppError::ClaudeAuthNotFound | AppError::ClaudeMissingTokens => true,
        AppError::ClaudeTokenRefresh(message) => {
            let message = message.to_ascii_lowercase();
            message.contains("invalid_grant")
                || message.contains("unauthorized")
                || message.contains("revoked")
                || message.contains("expired")
                || message.contains("status 401")
                || message.contains("status 403")
        }
        AppError::ClaudeUsageFetch(message) => {
            let message = message.to_ascii_lowercase();
            message.contains("status 401")
                || message.contains("status 403")
                || message.contains("unauthorized")
                || message.contains("expired")
                || message.contains("missing user:profile")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_auto_fallback_is_limited_to_auth_class_errors() {
        assert!(oauth_error_allows_cli_fallback(
            &AppError::ClaudeAuthNotFound
        ));
        assert!(oauth_error_allows_cli_fallback(
            &AppError::ClaudeMissingTokens
        ));
        assert!(oauth_error_allows_cli_fallback(
            &AppError::ClaudeTokenRefresh("status 400: invalid_grant".to_string())
        ));
        assert!(oauth_error_allows_cli_fallback(
            &AppError::ClaudeTokenRefresh("status 401".to_string())
        ));
        assert!(oauth_error_allows_cli_fallback(
            &AppError::ClaudeUsageFetch("status 403".to_string())
        ));
        assert!(!oauth_error_allows_cli_fallback(
            &AppError::ClaudeTokenRefresh("connection reset".to_string())
        ));
        assert!(!oauth_error_allows_cli_fallback(
            &AppError::ClaudeUsageFetch("status 429".to_string())
        ));
    }
}
