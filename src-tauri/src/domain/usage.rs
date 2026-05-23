use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

use crate::domain::account::AccountSummary;
use crate::provider::{AccountRefreshDiagnostics, ProviderFetchAttempt, ProviderSourceMode};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub label: String,
    pub used_percent: f64,
    pub remaining_percent: f64,
    pub reset_at: Option<i64>,
    pub window_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditsSnapshot {
    pub balance: Option<f64>,
    pub has_credits: bool,
    pub unlimited: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub account_id: String,
    pub source: String,
    #[serde(default)]
    pub source_mode: Option<ProviderSourceMode>,
    #[serde(default)]
    pub fetch_attempts: Vec<ProviderFetchAttempt>,
    pub plan_type: Option<String>,
    pub primary: Option<UsageWindow>,
    pub secondary: Option<UsageWindow>,
    #[serde(default)]
    pub tertiary: Option<UsageWindow>,
    pub credits: Option<CreditsSnapshot>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuotaEventKind {
    Warning,
    Reset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuotaEventSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaEvent {
    pub id: String,
    pub kind: QuotaEventKind,
    pub severity: QuotaEventSeverity,
    pub account_id: String,
    pub account_label: String,
    pub window_key: String,
    pub window_label: String,
    pub used_percent: f64,
    pub threshold_percent: Option<f64>,
    #[serde(default)]
    pub reset_at: Option<i64>,
    pub title: String,
    pub body: String,
    pub generated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostUsageDailyPoint {
    pub day_key: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostUsageSnapshot {
    pub today_tokens: i64,
    pub today_cost_usd: Option<f64>,
    pub last_30_days_tokens: i64,
    pub last_30_days_cost_usd: Option<f64>,
    #[serde(default = "default_cost_usage_range_days")]
    pub range_days: u16,
    #[serde(default)]
    pub timezone: Option<String>,
    /// The day_key for "today" in the snapshot's bucketing timezone. The
    /// frontend uses this to look up today_tokens in `daily`; deriving it
    /// from a UTC offset can land on the wrong bucket near local midnight.
    #[serde(default)]
    pub today_key: Option<String>,
    #[serde(default)]
    pub scan_stats: Option<CostUsageScanStats>,
    pub daily: Vec<CostUsageDailyPoint>,
    pub updated_at: i64,
    pub source_root: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostUsageScanStats {
    pub files_scanned: usize,
    pub files_reused: usize,
    pub files_removed: usize,
    pub events_retained: usize,
    pub retention_days: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountIssue {
    pub code: String,
    pub user_message: String,
    pub auth_related: bool,
}

impl AccountIssue {
    pub fn new(
        code: impl Into<String>,
        user_message: impl Into<String>,
        auth_related: bool,
    ) -> Self {
        Self {
            code: code.into(),
            user_message: user_message.into(),
            auth_related,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOverviewSnapshot {
    pub accounts: Vec<AccountSummary>,
    pub usage_by_account_id: HashMap<String, UsageSnapshot>,
    #[serde(default, deserialize_with = "deserialize_account_issues")]
    pub errors_by_account_id: HashMap<String, AccountIssue>,
    #[serde(default)]
    pub diagnostics_by_account_id: HashMap<String, AccountRefreshDiagnostics>,
    #[serde(default)]
    pub stale_reason: Option<String>,
    #[serde(default)]
    pub last_successful_at: Option<i64>,
    #[serde(default)]
    pub last_attempt_at: Option<i64>,
    #[serde(default)]
    pub quota_events: Vec<QuotaEvent>,
    pub cost_usage: Option<CostUsageSnapshot>,
    pub cost_error: Option<String>,
    pub generated_at: i64,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeOverviewSnapshot {
    pub accounts: Vec<AccountSummary>,
    pub usage_by_account_id: HashMap<String, UsageSnapshot>,
    #[serde(default, deserialize_with = "deserialize_account_issues")]
    pub errors_by_account_id: HashMap<String, AccountIssue>,
    #[serde(default)]
    pub diagnostics_by_account_id: HashMap<String, AccountRefreshDiagnostics>,
    #[serde(default)]
    pub stale_reason: Option<String>,
    #[serde(default)]
    pub last_successful_at: Option<i64>,
    #[serde(default)]
    pub last_attempt_at: Option<i64>,
    #[serde(default)]
    pub quota_events: Vec<QuotaEvent>,
    pub cost_usage: Option<CostUsageSnapshot>,
    pub cost_error: Option<String>,
    pub generated_at: i64,
    pub stale: bool,
}

fn default_cost_usage_range_days() -> u16 {
    30
}

fn deserialize_account_issues<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, AccountIssue>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StoredIssue {
        Structured(AccountIssue),
        Legacy(String),
    }

    let issues = HashMap::<String, StoredIssue>::deserialize(deserializer)?;
    Ok(issues
        .into_iter()
        .map(|(account_id, issue)| {
            let issue = match issue {
                StoredIssue::Structured(issue) => issue,
                StoredIssue::Legacy(message) => {
                    let auth_related = legacy_message_is_auth_related(&message);
                    AccountIssue::new("legacy_error", message, auth_related)
                }
            };
            (account_id, issue)
        })
        .collect())
}

fn legacy_message_is_auth_related(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("401")
        || message.contains("403")
        || message.contains("unauthorized")
        || message.contains("invalid_grant")
        || message.contains("auth.json was not found")
        || message.contains("does not contain oauth tokens")
}
