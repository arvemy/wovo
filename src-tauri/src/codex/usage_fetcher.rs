use crate::codex::auth_store::CodexOAuthCredentials;
use crate::domain::usage::{CreditsSnapshot, UsageSnapshot, UsageWindow};
use crate::error::AppError;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use time::OffsetDateTime;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::{timeout, Duration};

const USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
const APP_SERVER_TIMEOUT: Duration = Duration::from_secs(30);
const APP_SERVER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_ERROR_OUTPUT_BYTES: usize = 4_000;

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

pub async fn fetch_oauth_usage(
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
    normalize_oauth_usage(account_id, decoded)
}

pub async fn fetch_cli_usage(
    account_id: String,
    codex_home: &Path,
) -> Result<UsageSnapshot, AppError> {
    let responses = run_app_server(codex_home).await?;
    let rate_limits = response_result(&responses, 2)?;
    let account = response_result(&responses, 3)?;

    let rate_limits: CliRateLimitsResponse =
        serde_json::from_value(rate_limits.clone()).map_err(|error| {
            AppError::UsageFetch(format!("failed to decode Codex CLI rate limits: {error}"))
        })?;
    let account: CliAccountResponse = serde_json::from_value(account.clone()).map_err(|error| {
        AppError::UsageFetch(format!("failed to decode Codex CLI account: {error}"))
    })?;

    if cli_account_requires_login(&account) {
        return Err(AppError::AuthNotFound);
    }

    normalize_cli_usage(account_id, rate_limits, account)
}

fn cli_account_requires_login(account: &CliAccountResponse) -> bool {
    account.account.is_none()
}

async fn run_app_server(codex_home: &Path) -> Result<HashMap<i64, Value>, AppError> {
    let mut command = Command::new("codex");
    command
        .args(["-s", "read-only", "-a", "untrusted", "app-server"])
        .env("CODEX_HOME", codex_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::CodexBinaryNotFound)
        }
        Err(error) => return Err(AppError::UsageFetch(error.to_string())),
    };

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| AppError::UsageFetch("failed to open Codex CLI stdin".to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::UsageFetch("failed to open Codex CLI stdout".to_string()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::UsageFetch("failed to open Codex CLI stderr".to_string()))?;
    let stderr_task = tokio::spawn(async move {
        let mut buffer = Vec::new();
        let _ = stderr.read_to_end(&mut buffer).await;
        buffer
    });

    let mut lines = BufReader::new(stdout).lines();
    let mut stdout_text = String::new();
    let mut responses = HashMap::new();

    if let Err(error) = write_json_rpc_request(&mut stdin, initialize_request()).await {
        return finish_app_server_error(child, stderr_task, stdout_text, error).await;
    }
    if let Err(error) =
        read_until_responses(&mut lines, &mut responses, &mut stdout_text, &[1]).await
    {
        return finish_app_server_error(child, stderr_task, stdout_text, error).await;
    }
    if let Err(error) = response_result(&responses, 1).map(|_| ()) {
        return finish_app_server_error(child, stderr_task, stdout_text, error).await;
    }

    for request in [
        serde_json::json!({ "method": "initialized" }),
        rate_limits_request(),
        account_request(),
    ] {
        if let Err(error) = write_json_rpc_request(&mut stdin, request).await {
            return finish_app_server_error(child, stderr_task, stdout_text, error).await;
        }
    }
    if let Err(error) = stdin
        .flush()
        .await
        .map_err(|error| AppError::UsageFetch(error.to_string()))
    {
        return finish_app_server_error(child, stderr_task, stdout_text, error).await;
    }

    if let Err(error) =
        read_until_responses(&mut lines, &mut responses, &mut stdout_text, &[2, 3]).await
    {
        return finish_app_server_error(child, stderr_task, stdout_text, error).await;
    }

    drop(stdin);
    stop_app_server(child).await;
    let _ = stderr_task.await;

    Ok(responses)
}

fn initialize_request() -> Value {
    serde_json::json!({
        "id": 1,
        "method": "initialize",
        "params": {
            "clientInfo": {
                "name": "wovo",
                "title": null,
                "version": env!("CARGO_PKG_VERSION")
            },
            "capabilities": {
                "experimentalApi": true
            }
        }
    })
}

fn rate_limits_request() -> Value {
    serde_json::json!({
        "id": 2,
        "method": "account/rateLimits/read"
    })
}

fn account_request() -> Value {
    serde_json::json!({
        "id": 3,
        "method": "account/read",
        "params": {
            "refreshToken": false
        }
    })
}

async fn write_json_rpc_request(
    stdin: &mut tokio::process::ChildStdin,
    request: Value,
) -> Result<(), AppError> {
    stdin
        .write_all(format!("{request}\n").as_bytes())
        .await
        .map_err(|error| AppError::UsageFetch(error.to_string()))
}

async fn read_until_responses(
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    responses: &mut HashMap<i64, Value>,
    stdout_text: &mut String,
    expected_ids: &[i64],
) -> Result<(), AppError> {
    timeout(APP_SERVER_TIMEOUT, async {
        while !expected_ids.iter().all(|id| responses.contains_key(id)) {
            let Some(line) = lines
                .next_line()
                .await
                .map_err(|error| AppError::UsageFetch(error.to_string()))?
            else {
                return Err(AppError::UsageFetch(
                    "Codex CLI app-server stdout closed before expected responses".to_string(),
                ));
            };
            stdout_text.push_str(&line);
            stdout_text.push('\n');
            if let Some((id, value)) = parse_json_rpc_line(&line)? {
                responses.insert(id, value);
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| AppError::UsageFetch("Codex CLI app-server timed out".to_string()))?
}

async fn finish_app_server_error(
    child: Child,
    stderr_task: tokio::task::JoinHandle<Vec<u8>>,
    stdout_text: String,
    error: AppError,
) -> Result<HashMap<i64, Value>, AppError> {
    stop_app_server(child).await;
    let stderr = stderr_task.await.unwrap_or_default();
    Err(AppError::UsageFetch(format!(
        "{error}: {}",
        trimmed_output(stdout_text.as_bytes(), &stderr)
    )))
}

async fn stop_app_server(mut child: Child) {
    match timeout(APP_SERVER_SHUTDOWN_TIMEOUT, child.wait()).await {
        Ok(_) => {}
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}

#[cfg(test)]
fn parse_json_rpc_output(output: &str) -> Result<HashMap<i64, Value>, AppError> {
    let mut responses = HashMap::new();
    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some((id, value)) = parse_json_rpc_line(line)? {
            responses.insert(id, value);
        }
    }
    Ok(responses)
}

fn parse_json_rpc_line(line: &str) -> Result<Option<(i64, Value)>, AppError> {
    let value: Value =
        serde_json::from_str(line).map_err(|error| AppError::UsageFetch(error.to_string()))?;
    let Some(id) = value.get("id").and_then(Value::as_i64) else {
        return Ok(None);
    };
    Ok(Some((id, value)))
}

fn response_result(responses: &HashMap<i64, Value>, id: i64) -> Result<&Value, AppError> {
    let response = responses
        .get(&id)
        .ok_or_else(|| AppError::UsageFetch(format!("Codex CLI did not return response {id}")))?;
    if let Some(error) = response.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Codex CLI app-server request failed");
        if message
            .to_ascii_lowercase()
            .contains("authentication required")
        {
            return Err(AppError::AuthNotFound);
        }
        return Err(AppError::UsageFetch(message.to_string()));
    }
    response
        .get("result")
        .ok_or_else(|| AppError::UsageFetch(format!("Codex CLI response {id} had no result")))
}

fn trimmed_output(stdout: &[u8], stderr: &[u8]) -> String {
    let mut output = String::new();
    output.push_str(&String::from_utf8_lossy(stdout));
    if !stdout.is_empty() && !stderr.is_empty() {
        output.push('\n');
    }
    output.push_str(&String::from_utf8_lossy(stderr));
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return "Codex CLI app-server exited with a non-zero status".to_string();
    }
    trimmed.chars().take(MAX_ERROR_OUTPUT_BYTES).collect()
}

fn normalize_oauth_usage(
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliRateLimitsResponse {
    rate_limits: CliRateLimitSnapshot,
    rate_limits_by_limit_id: Option<HashMap<String, CliRateLimitSnapshot>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliRateLimitSnapshot {
    #[serde(rename = "limitId")]
    _limit_id: Option<String>,
    #[serde(rename = "limitName")]
    _limit_name: Option<String>,
    primary: Option<CliRateLimitWindow>,
    secondary: Option<CliRateLimitWindow>,
    credits: Option<CliCreditsSnapshot>,
    plan_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliRateLimitWindow {
    used_percent: f64,
    window_duration_mins: Option<i64>,
    resets_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliCreditsSnapshot {
    has_credits: bool,
    unlimited: bool,
    balance: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliAccountResponse {
    account: Option<CliAccount>,
    #[serde(rename = "requiresOpenaiAuth")]
    _requires_openai_auth: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum CliAccount {
    Chatgpt {
        #[serde(rename = "planType")]
        plan_type: Option<String>,
    },
    ApiKey,
    AmazonBedrock,
}

fn normalize_cli_usage(
    account_id: String,
    response: CliRateLimitsResponse,
    account: CliAccountResponse,
) -> Result<UsageSnapshot, AppError> {
    let snapshot = response
        .rate_limits_by_limit_id
        .as_ref()
        .and_then(|by_id| by_id.get("codex"))
        .cloned()
        .or_else(|| {
            response
                .rate_limits_by_limit_id
                .as_ref()
                .and_then(|by_id| by_id.values().next().cloned())
        })
        .unwrap_or(response.rate_limits);

    let primary = snapshot
        .primary
        .as_ref()
        .map(|window| normalize_cli_window("5h limit", window));
    let secondary = snapshot
        .secondary
        .as_ref()
        .map(|window| normalize_cli_window("Weekly limit", window));
    let credits = snapshot.credits.map(normalize_cli_credits);

    if primary.is_none() && secondary.is_none() && credits.is_none() {
        return Err(AppError::InvalidUsageResponse);
    }

    let account_plan_type = match account.account {
        Some(CliAccount::Chatgpt { plan_type }) => plan_type,
        _ => None,
    };

    Ok(UsageSnapshot {
        account_id,
        source: "cli".to_string(),
        plan_type: snapshot.plan_type.or(account_plan_type),
        primary,
        secondary,
        credits,
        updated_at: OffsetDateTime::now_utc().unix_timestamp(),
    })
}

fn normalize_cli_window(label: &str, window: &CliRateLimitWindow) -> UsageWindow {
    let used_percent = window.used_percent.clamp(0.0, 100.0);

    UsageWindow {
        label: label.to_string(),
        used_percent,
        remaining_percent: 100.0 - used_percent,
        reset_at: window.resets_at.map(normalize_timestamp),
        window_seconds: window.window_duration_mins.map(|minutes| minutes * 60),
    }
}

fn normalize_cli_credits(credits: CliCreditsSnapshot) -> CreditsSnapshot {
    CreditsSnapshot {
        balance: credits.balance.and_then(|value| value.parse::<f64>().ok()),
        has_credits: credits.has_credits,
        unlimited: credits.unlimited,
    }
}

fn normalize_timestamp(value: i64) -> i64 {
    if value > 10_000_000_000 {
        value / 1_000
    } else {
        value
    }
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

        let snapshot = normalize_oauth_usage("ambient".to_string(), response).unwrap();
        assert_eq!(snapshot.plan_type.as_deref(), Some("pro"));
        assert_eq!(snapshot.source, "oauth");
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

        let snapshot = normalize_oauth_usage("ambient".to_string(), response).unwrap();
        assert_eq!(snapshot.primary.unwrap().remaining_percent, 100.0);
    }

    #[test]
    fn normalizes_cli_rate_limits_response() {
        let rate_limits: CliRateLimitsResponse = serde_json::from_str(
            r#"{
                "rateLimits": {
                    "limitId": "legacy",
                    "limitName": "Legacy",
                    "primary": null,
                    "secondary": null,
                    "credits": null,
                    "planType": "unknown",
                    "rateLimitReachedType": null
                },
                "rateLimitsByLimitId": {
                    "codex": {
                        "limitId": "codex",
                        "limitName": "Codex",
                        "primary": {
                            "usedPercent": 12.5,
                            "windowDurationMins": 300,
                            "resetsAt": 1770000000000
                        },
                        "secondary": {
                            "usedPercent": 80,
                            "windowDurationMins": 10080,
                            "resetsAt": 1770500000
                        },
                        "credits": {
                            "hasCredits": true,
                            "unlimited": false,
                            "balance": "19.75"
                        },
                        "planType": "pro",
                        "rateLimitReachedType": null
                    }
                }
            }"#,
        )
        .unwrap();
        let account: CliAccountResponse = serde_json::from_str(
            r#"{
                "account": {"type": "chatgpt", "email": "user@example.com", "planType": "plus"},
                "requiresOpenaiAuth": false
            }"#,
        )
        .unwrap();

        let snapshot = normalize_cli_usage("managed".to_string(), rate_limits, account).unwrap();

        assert_eq!(snapshot.source, "cli");
        assert_eq!(snapshot.plan_type.as_deref(), Some("pro"));
        assert_eq!(
            snapshot.primary.as_ref().unwrap().window_seconds,
            Some(18_000)
        );
        assert_eq!(snapshot.primary.unwrap().reset_at, Some(1_770_000_000));
        assert_eq!(snapshot.secondary.unwrap().remaining_percent, 20.0);
        assert_eq!(snapshot.credits.unwrap().balance, Some(19.75));
    }

    #[test]
    fn parses_json_rpc_response_errors_as_auth_failures() {
        let responses = parse_json_rpc_output(
            r#"{"error":{"code":-32600,"message":"codex account authentication required to read rate limits"},"id":2}"#,
        )
        .unwrap();

        let error = response_result(&responses, 2).unwrap_err();

        assert!(matches!(error, AppError::AuthNotFound));
    }

    #[test]
    fn accepts_cli_account_when_account_is_present_even_if_openai_auth_is_required() {
        let account: CliAccountResponse = serde_json::from_str(
            r#"{
                "account": {"type": "chatgpt", "email": "user@example.com", "planType": "plus"},
                "requiresOpenaiAuth": true
            }"#,
        )
        .unwrap();

        assert!(!cli_account_requires_login(&account));
    }

    #[test]
    fn requires_login_when_cli_account_is_missing() {
        let account: CliAccountResponse = serde_json::from_str(
            r#"{
                "account": null,
                "requiresOpenaiAuth": true
            }"#,
        )
        .unwrap();

        assert!(cli_account_requires_login(&account));
    }
}
