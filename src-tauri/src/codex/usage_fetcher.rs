use crate::codex::auth_store::CodexOAuthCredentials;
use crate::domain::usage::{CreditsSnapshot, UsageSnapshot, UsageWindow};
use crate::error::AppError;
use reqwest::Client;
use serde::Deserialize;
use time::OffsetDateTime;

const USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";

#[derive(Debug, Deserialize)]
struct CodexUsageResponse {
    plan_type: Option<String>,
    rate_limit: Option<RateLimitResponse>,
    credits: Option<CreditsResponse>,
}

#[derive(Debug, Deserialize)]
struct RateLimitResponse {
    primary_window: Option<WindowResponse>,
    secondary_window: Option<WindowResponse>,
}

#[derive(Debug, Deserialize)]
struct WindowResponse {
    used_percent: f64,
    reset_at: Option<i64>,
    limit_window_seconds: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CreditsResponse {
    has_credits: Option<bool>,
    unlimited: Option<bool>,
    balance: Option<serde_json::Value>,
}

pub async fn fetch_usage(
    account_id: String,
    credentials: &CodexOAuthCredentials,
) -> Result<UsageSnapshot, AppError> {
    let client = Client::new();
    let mut request = client
        .get(USAGE_ENDPOINT)
        .bearer_auth(&credentials.access_token)
        .header("Accept", "application/json")
        .header("User-Agent", "wovo");

    if let Some(account_id) = credentials
        .account_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        request = request.header("ChatGPT-Account-Id", account_id);
    }

    let response = request
        .send()
        .await
        .map_err(|error| AppError::UsageFetch(error.to_string()))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| AppError::UsageFetch(error.to_string()))?;

    if !status.is_success() {
        return Err(AppError::UsageFetch(format!("status {}", status.as_u16())));
    }

    let decoded: CodexUsageResponse =
        serde_json::from_str(&body).map_err(|error| AppError::UsageFetch(error.to_string()))?;
    normalize_usage(account_id, decoded)
}

fn normalize_usage(
    account_id: String,
    response: CodexUsageResponse,
) -> Result<UsageSnapshot, AppError> {
    let primary = response
        .rate_limit
        .as_ref()
        .and_then(|rate_limit| rate_limit.primary_window.as_ref())
        .map(|window| normalize_window("5h limit", window));
    let secondary = response
        .rate_limit
        .as_ref()
        .and_then(|rate_limit| rate_limit.secondary_window.as_ref())
        .map(|window| normalize_window("Weekly limit", window));

    if primary.is_none() && secondary.is_none() && response.credits.is_none() {
        return Err(AppError::InvalidUsageResponse);
    }

    Ok(UsageSnapshot {
        account_id,
        source: "oauth".to_string(),
        plan_type: response.plan_type,
        primary,
        secondary,
        credits: response.credits.map(normalize_credits),
        updated_at: OffsetDateTime::now_utc().unix_timestamp(),
    })
}

fn normalize_window(label: &str, window: &WindowResponse) -> UsageWindow {
    let used_percent = window.used_percent.clamp(0.0, 100.0);

    UsageWindow {
        label: label.to_string(),
        used_percent,
        remaining_percent: 100.0 - used_percent,
        reset_at: window.reset_at,
        window_seconds: window.limit_window_seconds,
    }
}

fn normalize_credits(credits: CreditsResponse) -> CreditsSnapshot {
    CreditsSnapshot {
        balance: credits.balance.and_then(value_to_f64),
        has_credits: credits.has_credits.unwrap_or(false),
        unlimited: credits.unlimited.unwrap_or(false),
    }
}

fn value_to_f64(value: serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_usage_response_with_windows_and_credits() {
        let response: CodexUsageResponse = serde_json::from_str(
            r#"{
                "plan_type": "pro",
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 25,
                        "reset_at": 1770000000,
                        "limit_window_seconds": 18000
                    },
                    "secondary_window": {
                        "used_percent": 55,
                        "reset_at": 1770500000,
                        "limit_window_seconds": 604800
                    }
                },
                "credits": {
                    "has_credits": true,
                    "unlimited": false,
                    "balance": "12.5"
                }
            }"#,
        )
        .unwrap();

        let snapshot = normalize_usage("ambient".to_string(), response).unwrap();
        assert_eq!(snapshot.plan_type.as_deref(), Some("pro"));
        assert_eq!(snapshot.primary.unwrap().used_percent, 25.0);
        assert_eq!(snapshot.secondary.unwrap().remaining_percent, 45.0);
        assert_eq!(snapshot.credits.unwrap().balance, Some(12.5));
    }

    #[test]
    fn allows_missing_optional_fields_when_some_usage_exists() {
        let response: CodexUsageResponse = serde_json::from_str(
            r#"{
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 0
                    }
                }
            }"#,
        )
        .unwrap();

        let snapshot = normalize_usage("ambient".to_string(), response).unwrap();
        assert_eq!(snapshot.primary.unwrap().remaining_percent, 100.0);
    }
}
