use crate::claude::auth_store::ClaudeOAuthCredentials;
use crate::claude::login_runner;
use crate::domain::usage::{CreditsSnapshot, UsageSnapshot, UsageWindow};
use crate::error::AppError;
use reqwest::Client;
use serde::Deserialize;
use std::cmp::Ordering;
use std::path::Path;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::time::Duration;

const USAGE_ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_USAGE_TIMEOUT: Duration = Duration::from_secs(15);
const CLI_USAGE_TIMEOUT: Duration = Duration::from_secs(25);
const CLI_STATUS_TIMEOUT: Duration = Duration::from_secs(12);
const FALLBACK_CLAUDE_CODE_VERSION: &str = "2.1.0";

#[derive(Debug, Deserialize)]
struct ClaudeOAuthUsageResponse {
    five_hour: Option<OAuthUsageWindow>,
    seven_day: Option<OAuthUsageWindow>,
    seven_day_sonnet: Option<OAuthUsageWindow>,
    seven_day_opus: Option<OAuthUsageWindow>,
    extra_usage: Option<OAuthExtraUsage>,
}

#[derive(Debug, Clone, Deserialize)]
struct OAuthUsageWindow {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthExtraUsage {
    is_enabled: Option<bool>,
    monthly_limit: Option<f64>,
    used_credits: Option<f64>,
    utilization: Option<f64>,
}

pub async fn fetch_oauth_usage(
    account_id: String,
    credentials: &ClaudeOAuthCredentials,
) -> Result<UsageSnapshot, AppError> {
    if !credentials.has_profile_scope() {
        return Err(AppError::ClaudeUsageFetch(
            "status 403: missing user:profile scope".to_string(),
        ));
    }
    if credentials.is_expired() {
        return Err(AppError::ClaudeUsageFetch(
            "status 401: OAuth token expired".to_string(),
        ));
    }

    let client = Client::builder()
        .timeout(OAUTH_USAGE_TIMEOUT)
        .build()
        .map_err(|error| AppError::ClaudeUsageFetch(error.to_string()))?;
    let response = client
        .get(USAGE_ENDPOINT)
        .bearer_auth(&credentials.access_token)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", claude_code_user_agent())
        .send()
        .await
        .map_err(|error| AppError::ClaudeUsageFetch(error.to_string()))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| AppError::ClaudeUsageFetch(error.to_string()))?;

    if !status.is_success() {
        return Err(AppError::ClaudeUsageFetch(format!(
            "status {}",
            status.as_u16()
        )));
    }

    let decoded: ClaudeOAuthUsageResponse = serde_json::from_str(&body)
        .map_err(|error| AppError::ClaudeUsageFetch(error.to_string()))?;
    normalize_oauth_usage(account_id, credentials.plan_type(), decoded)
}

pub async fn fetch_cli_usage(
    account_id: String,
    claude_home: &Path,
) -> Result<UsageSnapshot, AppError> {
    let usage = login_runner::run_slash_command(claude_home, "/usage", CLI_USAGE_TIMEOUT).await?;
    let status = login_runner::run_slash_command(claude_home, "/status", CLI_STATUS_TIMEOUT)
        .await
        .ok();
    normalize_cli_usage(account_id, &usage, status.as_deref())
}

pub async fn fetch_cli_identity(
    claude_home: &Path,
) -> Result<(Option<String>, Option<String>, Option<String>), AppError> {
    let status =
        login_runner::run_slash_command(claude_home, "/status", CLI_STATUS_TIMEOUT).await?;
    let clean = strip_ansi_codes(&status);
    Ok(parse_identity("", Some(&clean)))
}

fn normalize_oauth_usage(
    account_id: String,
    plan_type: Option<String>,
    response: ClaudeOAuthUsageResponse,
) -> Result<UsageSnapshot, AppError> {
    let weekly = response.seven_day.as_ref();
    let primary = response
        .five_hour
        .as_ref()
        .map(|window| normalize_oauth_window("Current session", window, Some(5 * 60 * 60)));
    let secondary = weekly
        .as_ref()
        .map(|window| normalize_oauth_window("Weekly limit", window, Some(7 * 24 * 60 * 60)));
    let tertiary = most_constrained_model_window(&response);
    let credits = response.extra_usage.and_then(normalize_extra_usage);

    if primary.is_none() && secondary.is_none() && tertiary.is_none() && credits.is_none() {
        return Err(AppError::ClaudeInvalidUsageResponse);
    }

    Ok(UsageSnapshot {
        account_id,
        source: "oauth".to_string(),
        plan_type,
        primary,
        secondary,
        tertiary,
        credits,
        updated_at: OffsetDateTime::now_utc().unix_timestamp(),
    })
}

fn normalize_oauth_window(
    label: &str,
    window: &OAuthUsageWindow,
    fallback_window_seconds: Option<i64>,
) -> UsageWindow {
    let used_percent = normalize_utilization(window.utilization.unwrap_or(0.0));
    UsageWindow {
        label: label.to_string(),
        used_percent,
        remaining_percent: 100.0 - used_percent,
        reset_at: window.resets_at.as_deref().and_then(parse_timestamp),
        window_seconds: fallback_window_seconds,
    }
}

fn most_constrained_model_window(response: &ClaudeOAuthUsageResponse) -> Option<UsageWindow> {
    let opus = response
        .seven_day_opus
        .as_ref()
        .map(|window| normalize_oauth_window("Weekly Opus", window, Some(7 * 24 * 60 * 60)));
    let sonnet = response
        .seven_day_sonnet
        .as_ref()
        .map(|window| normalize_oauth_window("Weekly Sonnet", window, Some(7 * 24 * 60 * 60)));

    match (opus, sonnet) {
        (Some(opus), Some(sonnet)) if sonnet.used_percent > opus.used_percent => Some(sonnet),
        (Some(opus), Some(_)) => Some(opus),
        (Some(opus), None) => Some(opus),
        (None, Some(sonnet)) => Some(sonnet),
        (None, None) => None,
    }
}

fn normalize_extra_usage(extra: OAuthExtraUsage) -> Option<CreditsSnapshot> {
    if extra.is_enabled == Some(false) {
        return None;
    }
    let balance = match (extra.monthly_limit, extra.used_credits) {
        (Some(limit), Some(used)) => Some((limit - used).max(0.0)),
        _ => None,
    };
    Some(CreditsSnapshot {
        balance,
        has_credits: extra.is_enabled.unwrap_or(false) || extra.utilization.is_some(),
        unlimited: false,
    })
}

fn normalize_utilization(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    if value <= 1.0 {
        (value * 100.0).clamp(0.0, 100.0)
    } else {
        value.clamp(0.0, 100.0)
    }
}

fn normalize_cli_usage(
    account_id: String,
    usage_text: &str,
    status_text: Option<&str>,
) -> Result<UsageSnapshot, AppError> {
    let clean = strip_ansi_codes(usage_text);
    if let Some(error) = extract_usage_error(&clean) {
        return Err(AppError::ClaudeUsageFetch(error));
    }
    let primary = extract_window(
        &clean,
        &["Current session"],
        &[],
        "Current session",
        Some(5 * 60 * 60),
    );
    let secondary = extract_window(
        &clean,
        &["Current week (all models)", "Current week"],
        &[
            "Current week (Opus)",
            "Current week (Sonnet only)",
            "Current week (Sonnet)",
        ],
        "Weekly limit",
        Some(7 * 24 * 60 * 60),
    );
    let tertiary = extract_most_constrained_window(
        &clean,
        &[
            "Current week (Opus)",
            "Current week (Sonnet only)",
            "Current week (Sonnet)",
        ],
        &[],
        "Weekly model limit",
        Some(7 * 24 * 60 * 60),
    );
    let (_, _, plan_type) = parse_identity(&clean, status_text.map(strip_ansi_codes).as_deref());

    if primary.is_none() && secondary.is_none() && tertiary.is_none() {
        return Err(AppError::ClaudeInvalidUsageResponse);
    }

    Ok(UsageSnapshot {
        account_id,
        source: "cli".to_string(),
        plan_type,
        primary,
        secondary,
        tertiary,
        credits: None,
        updated_at: OffsetDateTime::now_utc().unix_timestamp(),
    })
}

fn extract_window(
    text: &str,
    labels: &[&str],
    excluded_labels: &[&str],
    output_label: &str,
    window_seconds: Option<i64>,
) -> Option<UsageWindow> {
    extract_window_candidates(text, labels, excluded_labels, output_label, window_seconds)
        .into_iter()
        .next()
}

fn extract_most_constrained_window(
    text: &str,
    labels: &[&str],
    excluded_labels: &[&str],
    output_label: &str,
    window_seconds: Option<i64>,
) -> Option<UsageWindow> {
    extract_window_candidates(text, labels, excluded_labels, output_label, window_seconds)
        .into_iter()
        .max_by(|left, right| {
            left.used_percent
                .partial_cmp(&right.used_percent)
                .unwrap_or(Ordering::Equal)
        })
}

fn extract_window_candidates(
    text: &str,
    labels: &[&str],
    excluded_labels: &[&str],
    output_label: &str,
    window_seconds: Option<i64>,
) -> Vec<UsageWindow> {
    let lines: Vec<&str> = text.lines().collect();
    let normalized_labels: Vec<String> =
        labels.iter().map(|label| normalize_label(label)).collect();
    let normalized_excluded_labels: Vec<String> = excluded_labels
        .iter()
        .map(|label| normalize_label(label))
        .collect();
    let mut windows = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let normalized = normalize_label(line);
        if normalized_excluded_labels
            .iter()
            .any(|label| normalized.contains(label))
        {
            continue;
        }
        if !normalized_labels
            .iter()
            .any(|label| normalized.contains(label))
        {
            continue;
        }
        for candidate in lines.iter().skip(index).take(14) {
            if let Some(used_percent) = percent_used_from_line(candidate) {
                windows.push(UsageWindow {
                    label: output_label.to_string(),
                    used_percent,
                    remaining_percent: 100.0 - used_percent,
                    reset_at: None,
                    window_seconds,
                });
                break;
            }
        }
    }
    windows
}

fn percent_used_from_line(line: &str) -> Option<f64> {
    let percent_index = line.find('%')?;
    let prefix = &line[..percent_index];
    let number_start = prefix
        .char_indices()
        .rev()
        .take_while(|(_, ch)| ch.is_ascii_digit() || *ch == '.')
        .last()
        .map(|(index, _)| index)?;
    let value = prefix[number_start..]
        .parse::<f64>()
        .ok()?
        .clamp(0.0, 100.0);
    let lower = line.to_ascii_lowercase();
    if lower.contains("left") || lower.contains("remaining") || lower.contains("available") {
        Some(100.0 - value)
    } else {
        Some(value)
    }
}

fn parse_identity(
    usage_text: &str,
    status_text: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>) {
    let combined = [usage_text, status_text.unwrap_or("")].join("\n");
    let email = extract_prefixed_value(&combined, &["Account:", "Email:"])
        .filter(|value| value.contains('@'))
        .or_else(|| first_email(&combined));
    let organization =
        extract_prefixed_value(&combined, &["Org:", "Organization:"]).filter(|org| {
            email
                .as_deref()
                .map(|email| {
                    !org.to_ascii_lowercase()
                        .starts_with(&email.to_ascii_lowercase())
                })
                .unwrap_or(true)
        });
    let plan = extract_prefixed_value(&combined, &["Login method:"])
        .and_then(normalize_plan_label)
        .or_else(|| extract_claude_plan(&combined));
    (email, organization, plan)
}

fn extract_prefixed_value(text: &str, prefixes: &[&str]) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        for prefix in prefixes {
            if trimmed
                .to_ascii_lowercase()
                .starts_with(&prefix.to_ascii_lowercase())
            {
                let value = trimmed[prefix.len()..].trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

fn first_email(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|token| {
            token.trim_matches(|ch: char| {
                !ch.is_ascii_alphanumeric()
                    && ch != '@'
                    && ch != '.'
                    && ch != '_'
                    && ch != '-'
                    && ch != '+'
            })
        })
        .find(|token| token.contains('@') && token.contains('.'))
        .map(str::to_string)
}

fn extract_claude_plan(text: &str) -> Option<String> {
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("subscription") || lower.contains("select") {
            continue;
        }
        if lower.contains("claude pro") {
            return Some("Claude Pro".to_string());
        }
        if lower.contains("claude max") {
            return Some("Claude Max".to_string());
        }
        if lower.contains("claude team") {
            return Some("Claude Team".to_string());
        }
        if lower.contains("claude enterprise") {
            return Some("Claude Enterprise".to_string());
        }
    }
    None
}

fn normalize_plan_label(value: String) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("claude pro") || lower == "pro" {
        Some("Claude Pro".to_string())
    } else if lower.contains("claude max") || lower == "max" {
        Some("Claude Max".to_string())
    } else if lower.contains("claude team") || lower == "team" {
        Some("Claude Team".to_string())
    } else if lower.contains("claude enterprise") || lower == "enterprise" {
        Some("Claude Enterprise".to_string())
    } else if lower.contains("api key") {
        Some("API key".to_string())
    } else {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }
}

fn extract_usage_error(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("token_expired") || lower.contains("token has expired") {
        return Some("Claude CLI token expired. Run `claude /login`.".to_string());
    }
    if lower.contains("authentication_error") {
        return Some("Claude CLI authentication error. Run `claude /login`.".to_string());
    }
    if lower.contains("rate_limit_error") || lower.contains("rate limited") {
        return Some("Claude CLI usage endpoint is rate limited right now.".to_string());
    }
    if lower.contains("failed to load usage data") {
        return Some("Claude CLI could not load usage data.".to_string());
    }
    None
}

fn normalize_label(text: &str) -> String {
    text.to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

fn parse_timestamp(value: &str) -> Option<i64> {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(|value| value.unix_timestamp())
}

fn claude_code_user_agent() -> String {
    format!("claude-code/{FALLBACK_CLAUDE_CODE_VERSION}")
}

fn strip_ansi_codes(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek().copied() {
                Some('[') => {
                    let _ = chars.next();
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    let _ = chars.next();
                    let mut saw_escape = false;
                    for next in chars.by_ref() {
                        if next == '\u{7}' || (saw_escape && next == '\\') {
                            break;
                        }
                        saw_escape = next == '\u{1b}';
                    }
                }
                Some(_) => {
                    let _ = chars.next();
                }
                None => {}
            };
        } else if ch == '\r' {
            output.push('\n');
        } else if ch.is_control() && ch != '\n' && ch != '\t' {
            continue;
        } else {
            output.push(ch);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_oauth_windows_and_model_specific_window() {
        let response: ClaudeOAuthUsageResponse = serde_json::from_str(
            r#"{
                "five_hour": {"utilization": 0.25, "resets_at": "2026-05-19T10:00:00Z"},
                "seven_day": {"utilization": 50, "resets_at": "2026-05-25T10:00:00Z"},
                "seven_day_sonnet": {"utilization": 0.75},
                "extra_usage": {"is_enabled": true, "monthly_limit": 100, "used_credits": 12}
            }"#,
        )
        .unwrap();

        let snapshot = normalize_oauth_usage(
            "ambient".to_string(),
            Some("Claude Max".to_string()),
            response,
        )
        .unwrap();

        assert_eq!(snapshot.source, "oauth");
        assert_eq!(snapshot.plan_type.as_deref(), Some("Claude Max"));
        assert_eq!(snapshot.primary.unwrap().used_percent, 25.0);
        assert_eq!(snapshot.secondary.unwrap().used_percent, 50.0);
        assert_eq!(snapshot.tertiary.unwrap().used_percent, 75.0);
        assert_eq!(snapshot.credits.unwrap().balance, Some(88.0));
    }

    #[test]
    fn oauth_model_window_uses_most_constrained_limit() {
        let response: ClaudeOAuthUsageResponse = serde_json::from_str(
            r#"{
                "seven_day_opus": {"utilization": 0.40},
                "seven_day_sonnet": {"utilization": 0.95}
            }"#,
        )
        .unwrap();

        let snapshot = normalize_oauth_usage("ambient".to_string(), None, response).unwrap();
        let tertiary = snapshot.tertiary.unwrap();

        assert_eq!(tertiary.label, "Weekly Sonnet");
        assert_eq!(tertiary.used_percent, 95.0);
    }

    #[test]
    fn oauth_weekly_only_usage_uses_weekly_window_duration() {
        let response: ClaudeOAuthUsageResponse = serde_json::from_str(
            r#"{
                "seven_day": {"utilization": 0.40, "resets_at": "2026-05-25T10:00:00Z"}
            }"#,
        )
        .unwrap();

        let snapshot = normalize_oauth_usage("ambient".to_string(), None, response).unwrap();
        let secondary = snapshot.secondary.unwrap();

        assert!(snapshot.primary.is_none());
        assert_eq!(secondary.label, "Weekly limit");
        assert_eq!(secondary.window_seconds, Some(7 * 24 * 60 * 60));
    }

    #[test]
    fn parses_cli_usage_output() {
        let snapshot = normalize_cli_usage(
            "managed".to_string(),
            r#"
            Current session
            22% used
            Current week (all models)
            60% left
            Current week (Sonnet only)
            10% used
            "#,
            Some("Account: user@example.com\nLogin method: Claude Max"),
        )
        .unwrap();

        assert_eq!(snapshot.primary.unwrap().used_percent, 22.0);
        assert_eq!(snapshot.secondary.unwrap().used_percent, 40.0);
        assert_eq!(snapshot.tertiary.unwrap().used_percent, 10.0);
        assert_eq!(snapshot.plan_type.as_deref(), Some("Claude Max"));
    }

    #[test]
    fn cli_weekly_fallback_does_not_parse_model_specific_limit() {
        let snapshot = normalize_cli_usage(
            "managed".to_string(),
            r#"
            Current week (Opus)
            100% used
            Current week (Sonnet only)
            15% used
            "#,
            None,
        )
        .unwrap();

        assert!(snapshot.secondary.is_none());
        assert_eq!(snapshot.tertiary.unwrap().used_percent, 100.0);
    }

    #[test]
    fn cli_model_window_uses_most_constrained_limit() {
        let snapshot = normalize_cli_usage(
            "managed".to_string(),
            r#"
            Current week (Opus)
            15% used
            Current week (Sonnet only)
            94% used
            "#,
            None,
        )
        .unwrap();

        assert_eq!(snapshot.tertiary.unwrap().used_percent, 94.0);
    }

    #[test]
    fn parses_cli_usage_from_pty_screen_output() {
        let snapshot = normalize_cli_usage(
            "ambient".to_string(),
            r#"
            Settings  Status  Config  Usage  Stats

              Current session                                      12% used
              Resets 4:50am (Europe/Istanbul)

              Current week (all models)
                                                              34% used
              Resets May 26, 5pm (Europe/Istanbul)

              Extra usage
              Extra usage not enabled
            "#,
            Some(
                r#"
                Version:          2.1.143
                Login method:     Claude Pro account
                Organization:     Example Organization
                Email:            user@example.com
                "#,
            ),
        )
        .unwrap();

        assert_eq!(snapshot.source, "cli");
        assert_eq!(snapshot.primary.unwrap().used_percent, 12.0);
        assert_eq!(snapshot.secondary.unwrap().used_percent, 34.0);
        assert_eq!(snapshot.plan_type.as_deref(), Some("Claude Pro"));
    }

    #[test]
    fn does_not_extract_usage_source_menu_as_plan() {
        let (_, _, plan) = parse_identity(
            r#"
            › 1. Claude account with subscription - Pro, Max, Team, or Enterprise
              2. Anthropic Console account - API usage billing
              3. Third-party platform - Amazon Bedrock, Microsoft Foundry, or Vertex AI
            "#,
            None,
        );

        assert_eq!(plan, None);
    }

    #[test]
    fn strips_terminal_control_sequences() {
        let stripped = strip_ansi_codes(
            "\u{1b}]0;Claude Code\u{7}\u{1b}[31mCurrent session\u{1b}[0m\r12% used\u{1b}7",
        );

        assert_eq!(stripped, "Current session\n12% used");
    }
}
