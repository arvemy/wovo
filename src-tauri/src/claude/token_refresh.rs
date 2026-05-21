use crate::claude::auth_store::ClaudeOAuthCredentials;
use crate::error::AppError;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::time::Duration;
use time::{Duration as TimeDuration, OffsetDateTime};

const REFRESH_ENDPOINT: &str = "https://platform.claude.com/v1/oauth/token";
const DEFAULT_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const TOKEN_REFRESH_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_SCOPES: &[&str] = &["user:profile", "user:inference"];

#[derive(Debug, Serialize)]
struct RefreshRequest<'a> {
    grant_type: &'a str,
    refresh_token: &'a str,
    client_id: &'a str,
    scope: String,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    scope: Option<String>,
}

pub async fn refresh(
    credentials: ClaudeOAuthCredentials,
) -> Result<ClaudeOAuthCredentials, AppError> {
    let Some(refresh_token) = credentials
        .refresh_token
        .clone()
        .and_then(normalize_optional)
    else {
        return Ok(credentials);
    };

    let client_id = client_id_for_refresh(&credentials);
    let scope = scope_for_refresh(&credentials);
    let client = Client::builder()
        .timeout(TOKEN_REFRESH_TIMEOUT)
        .build()
        .map_err(|error| AppError::ClaudeTokenRefresh(error.to_string()))?;
    let response = client
        .post(REFRESH_ENDPOINT)
        .json(&RefreshRequest {
            grant_type: "refresh_token",
            refresh_token: &refresh_token,
            client_id: &client_id,
            scope,
        })
        .send()
        .await
        .map_err(|error| AppError::ClaudeTokenRefresh(error.to_string()))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| AppError::ClaudeTokenRefresh(error.to_string()))?;

    if !status.is_success() {
        return Err(AppError::ClaudeTokenRefresh(refresh_error_message(
            status.as_u16(),
            &body,
        )));
    }

    let refreshed: RefreshResponse = serde_json::from_str(&body)
        .map_err(|error| AppError::ClaudeTokenRefresh(error.to_string()))?;
    let access_token = refreshed
        .access_token
        .and_then(normalize_optional)
        .ok_or_else(|| AppError::ClaudeTokenRefresh("response missing access token".to_string()))?;
    let expires_in = refreshed
        .expires_in
        .filter(|seconds| *seconds > 0)
        .ok_or_else(|| {
            AppError::ClaudeTokenRefresh("response missing valid expires_in".to_string())
        })?;
    let expires_at = OffsetDateTime::now_utc()
        .checked_add(TimeDuration::seconds(expires_in))
        .ok_or_else(|| {
            AppError::ClaudeTokenRefresh("response expires_in out of range".to_string())
        })?;
    let scopes = refreshed
        .scope
        .as_deref()
        .map(parse_scopes)
        .filter(|scopes| !scopes.is_empty())
        .unwrap_or_else(|| credentials.scopes.clone());

    Ok(ClaudeOAuthCredentials {
        access_token,
        refresh_token: Some(
            refreshed
                .refresh_token
                .and_then(normalize_optional)
                .unwrap_or(refresh_token),
        ),
        expires_at: Some(expires_at),
        scopes,
        rate_limit_tier: credentials.rate_limit_tier,
        subscription_type: credentials.subscription_type,
        client_id: Some(client_id),
        home_path: credentials.home_path,
    })
}

fn client_id_for_refresh(credentials: &ClaudeOAuthCredentials) -> String {
    credentials
        .client_id
        .clone()
        .and_then(normalize_optional)
        .or_else(|| {
            env::var("CLAUDE_CODE_OAUTH_CLIENT_ID")
                .ok()
                .and_then(normalize_optional)
        })
        .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string())
}

fn scope_for_refresh(credentials: &ClaudeOAuthCredentials) -> String {
    if credentials.scopes.is_empty() {
        return DEFAULT_SCOPES.join(" ");
    }

    credentials.scopes.join(" ")
}

fn parse_scopes(raw: &str) -> Vec<String> {
    raw.split_whitespace()
        .filter_map(|scope| normalize_optional(scope.to_string()))
        .collect()
}

fn normalize_optional(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn refresh_error_message(status: u16, body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(code) = value
            .get("error")
            .and_then(|error| error.get("code").or(Some(error)))
            .and_then(Value::as_str)
        {
            return format!("status {status}: {code}");
        }
    }

    format!("status {status}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn credentials(scopes: Vec<String>, client_id: Option<String>) -> ClaudeOAuthCredentials {
        ClaudeOAuthCredentials {
            access_token: "access".to_string(),
            refresh_token: Some("refresh".to_string()),
            expires_at: None,
            scopes,
            rate_limit_tier: None,
            subscription_type: None,
            client_id,
            home_path: PathBuf::from("/tmp/claude"),
        }
    }

    #[test]
    fn refresh_scope_uses_existing_scopes() {
        let credentials = credentials(
            vec![
                "user:profile".to_string(),
                "user:sessions:claude_code".to_string(),
            ],
            None,
        );

        assert_eq!(
            scope_for_refresh(&credentials),
            "user:profile user:sessions:claude_code"
        );
    }

    #[test]
    fn refresh_scope_defaults_to_usage_scopes() {
        let credentials = credentials(Vec::new(), None);

        assert_eq!(
            scope_for_refresh(&credentials),
            "user:profile user:inference"
        );
    }

    #[test]
    fn refresh_client_id_prefers_stored_value() {
        let credentials = credentials(Vec::new(), Some(" stored-client ".to_string()));

        assert_eq!(client_id_for_refresh(&credentials), "stored-client");
    }

    #[test]
    fn refresh_error_message_extracts_error_code() {
        assert_eq!(
            refresh_error_message(400, r#"{"error":{"code":"invalid_grant"}}"#),
            "status 400: invalid_grant"
        );
    }
}
