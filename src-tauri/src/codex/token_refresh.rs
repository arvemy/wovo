use crate::codex::auth_store::CodexOAuthCredentials;
use crate::error::AppError;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

const REFRESH_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

#[derive(Debug, Serialize)]
struct RefreshRequest<'a> {
    client_id: &'a str,
    grant_type: &'a str,
    refresh_token: &'a str,
    scope: &'a str,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
}

pub async fn refresh(
    credentials: CodexOAuthCredentials,
) -> Result<CodexOAuthCredentials, AppError> {
    if credentials.refresh_token.trim().is_empty() {
        return Ok(credentials);
    }

    let client = Client::new();
    let response = client
        .post(REFRESH_ENDPOINT)
        .json(&RefreshRequest {
            client_id: CLIENT_ID,
            grant_type: "refresh_token",
            refresh_token: &credentials.refresh_token,
            scope: "openid profile email",
        })
        .send()
        .await
        .map_err(|error| AppError::TokenRefresh(error.to_string()))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| AppError::TokenRefresh(error.to_string()))?;

    if !status.is_success() {
        return Err(AppError::TokenRefresh(refresh_error_message(
            status.as_u16(),
            &body,
        )));
    }

    let refreshed: RefreshResponse =
        serde_json::from_str(&body).map_err(|error| AppError::TokenRefresh(error.to_string()))?;

    Ok(CodexOAuthCredentials {
        access_token: refreshed.access_token.unwrap_or(credentials.access_token),
        refresh_token: refreshed.refresh_token.unwrap_or(credentials.refresh_token),
        id_token: refreshed.id_token.or(credentials.id_token),
        account_id: credentials.account_id,
        last_refresh: Some(OffsetDateTime::now_utc()),
        home_path: credentials.home_path,
    })
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
