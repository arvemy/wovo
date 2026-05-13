use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::domain::account::AccountSummary;

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
    pub plan_type: Option<String>,
    pub primary: Option<UsageWindow>,
    pub secondary: Option<UsageWindow>,
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
    pub title: String,
    pub body: String,
    pub generated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostUsageDailyPoint {
    pub day_key: String,
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
    pub daily: Vec<CostUsageDailyPoint>,
    pub updated_at: i64,
    pub source_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOverviewSnapshot {
    pub accounts: Vec<AccountSummary>,
    pub usage_by_account_id: HashMap<String, UsageSnapshot>,
    pub errors_by_account_id: HashMap<String, String>,
    #[serde(default)]
    pub quota_events: Vec<QuotaEvent>,
    pub cost_usage: Option<CostUsageSnapshot>,
    pub cost_error: Option<String>,
    pub generated_at: i64,
    pub stale: bool,
}
