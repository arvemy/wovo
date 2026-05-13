use std::collections::HashMap;

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = ["window", "__TAURI__", "core"])]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_namespace = ["window", "__TAURI__", "event"])]
    async fn listen(event: &str, handler: &js_sys::Function) -> Result<JsValue, JsValue>;
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountSummary {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) source: AccountSourceKind,
    pub(crate) is_live_system: bool,
    pub(crate) can_set_system: bool,
    pub(crate) can_remove: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AccountSourceKind {
    Ambient,
    Managed,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageSnapshot {
    pub(crate) source: String,
    pub(crate) plan_type: Option<String>,
    pub(crate) primary: Option<UsageWindow>,
    pub(crate) secondary: Option<UsageWindow>,
    pub(crate) credits: Option<CreditsSnapshot>,
    pub(crate) updated_at: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum QuotaEventKind {
    Warning,
    Reset,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum QuotaEventSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuotaEvent {
    pub(crate) id: String,
    pub(crate) kind: QuotaEventKind,
    pub(crate) severity: QuotaEventSeverity,
    pub(crate) account_id: String,
    pub(crate) account_label: String,
    pub(crate) window_key: String,
    pub(crate) window_label: String,
    pub(crate) used_percent: f64,
    pub(crate) threshold_percent: Option<f64>,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) generated_at: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CodexUsageSourceMode {
    Auto,
    Oauth,
    Cli,
}

impl CodexUsageSourceMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Oauth => "OAuth",
            Self::Cli => "CLI",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexSettings {
    pub(crate) usage_source_mode: CodexUsageSourceMode,
    pub(crate) cost_usage_enabled: bool,
    pub(crate) notifications_enabled: bool,
    pub(crate) auto_account_switching_enabled: bool,
    pub(crate) hide_account_credentials: bool,
    #[serde(default = "default_auto_switch_threshold")]
    pub(crate) auto_switch_threshold_percent: f64,
    #[serde(default = "default_weekly_penalty_threshold")]
    pub(crate) weekly_penalty_threshold: f64,
}

fn default_auto_switch_threshold() -> f64 {
    90.0
}

fn default_weekly_penalty_threshold() -> f64 {
    20.0
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetUsageSourceModeArgs {
    pub(crate) usage_source_mode: CodexUsageSourceMode,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetCostUsageEnabledArgs {
    pub(crate) enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetNotificationsEnabledArgs {
    pub(crate) enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetAutoAccountSwitchingEnabledArgs {
    pub(crate) enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetHideAccountCredentialsArgs {
    pub(crate) enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetAutoSwitchThresholdArgs {
    pub(crate) value: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetWeeklyPenaltyThresholdArgs {
    pub(crate) value: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CostUsageDailyPoint {
    pub(crate) day_key: String,
    pub(crate) input_tokens: i64,
    pub(crate) cached_input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) total_tokens: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CostUsageSnapshot {
    pub(crate) today_tokens: i64,
    pub(crate) today_cost_usd: Option<f64>,
    pub(crate) last_30_days_tokens: i64,
    pub(crate) last_30_days_cost_usd: Option<f64>,
    pub(crate) daily: Vec<CostUsageDailyPoint>,
    pub(crate) updated_at: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexOverviewSnapshot {
    pub(crate) accounts: Vec<AccountSummary>,
    pub(crate) usage_by_account_id: HashMap<String, UsageSnapshot>,
    pub(crate) errors_by_account_id: HashMap<String, String>,
    #[serde(default)]
    pub(crate) quota_events: Vec<QuotaEvent>,
    pub(crate) cost_usage: Option<CostUsageSnapshot>,
    pub(crate) cost_error: Option<String>,
    pub(crate) generated_at: i64,
    pub(crate) stale: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageWindow {
    pub(crate) label: String,
    pub(crate) used_percent: f64,
    pub(crate) remaining_percent: f64,
    pub(crate) reset_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreditsSnapshot {
    pub(crate) balance: Option<f64>,
    pub(crate) has_credits: bool,
    pub(crate) unlimited: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RefreshSnapshotArgs {
    force: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountActionArgs<'a> {
    account_id: &'a str,
}

#[derive(Clone, Debug)]
pub(crate) struct CommandError {
    pub(crate) message: String,
}

impl CommandError {
    pub(crate) fn from_message(message: String) -> Self {
        Self { message }
    }
}

pub(crate) async fn refresh_snapshot(force: bool) -> Result<CodexOverviewSnapshot, CommandError> {
    let args = serde_wasm_bindgen::to_value(&RefreshSnapshotArgs { force })
        .map_err(|error| CommandError::from_message(error.to_string()))?;
    invoke_tauri("refresh_codex_snapshot", args).await
}

pub(crate) async fn account_action<T>(cmd: &str, account_id: &str) -> Result<T, CommandError>
where
    T: DeserializeOwned,
{
    let args = serde_wasm_bindgen::to_value(&AccountActionArgs { account_id })
        .map_err(|error| CommandError::from_message(error.to_string()))?;
    invoke_tauri(cmd, args).await
}

pub(crate) async fn invoke_tauri<T>(cmd: &str, args: JsValue) -> Result<T, CommandError>
where
    T: DeserializeOwned,
{
    let value = invoke(cmd, args)
        .await
        .map_err(|error| js_command_error(&error))?;
    serde_wasm_bindgen::from_value(value)
        .map_err(|error| CommandError::from_message(error.to_string()))
}

pub(crate) async fn listen_tauri(
    event: &str,
    handler: &js_sys::Function,
) -> Result<JsValue, JsValue> {
    listen(event, handler).await
}

pub(crate) fn js_command_error(value: &JsValue) -> CommandError {
    let message = js_sys::Reflect::get(value, &JsValue::from_str("message"))
        .ok()
        .and_then(|value| value.as_string())
        .or_else(|| value.as_string())
        .unwrap_or_else(|| "Wovo could not complete the request.".to_string());

    CommandError { message }
}
