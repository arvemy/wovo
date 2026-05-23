use std::collections::HashMap;

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/tauri_bridge.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = invokeWithPolicy)]
    async fn invoke_with_policy(
        cmd: &str,
        args: JsValue,
        timeout_ms: u32,
        retries: u32,
        retry_delay_ms: u32,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch)]
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
    #[serde(default)]
    pub(crate) source_mode: Option<CodexUsageSourceMode>,
    #[serde(default)]
    pub(crate) fetch_attempts: Vec<ProviderFetchAttempt>,
    pub(crate) plan_type: Option<String>,
    pub(crate) primary: Option<UsageWindow>,
    pub(crate) secondary: Option<UsageWindow>,
    #[serde(default)]
    pub(crate) tertiary: Option<UsageWindow>,
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
    #[serde(default)]
    pub(crate) reset_at: Option<i64>,
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
    Cached,
}

impl CodexUsageSourceMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Oauth => "OAuth",
            Self::Cli => "CLI",
            Self::Cached => "Cached",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexSettings {
    #[serde(default)]
    pub(crate) schema_version: u16,
    pub(crate) usage_source_mode: CodexUsageSourceMode,
    pub(crate) cost_usage_enabled: bool,
    pub(crate) notifications_enabled: bool,
    pub(crate) auto_account_switching_enabled: bool,
    #[serde(default = "default_auto_switch_threshold_percent")]
    pub(crate) auto_switch_threshold_percent: f64,
    #[serde(default = "default_cost_usage_range_days")]
    pub(crate) cost_usage_range_days: u16,
    pub(crate) hide_account_credentials: bool,
    #[serde(default)]
    pub(crate) launch_on_login: bool,
    #[serde(default)]
    pub(crate) config_warnings: Vec<String>,
}

fn default_auto_switch_threshold_percent() -> f64 {
    90.0
}

fn default_cost_usage_range_days() -> u16 {
    30
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotificationDiagnostics {
    pub(crate) last_attempt_at: Option<i64>,
    pub(crate) last_status: Option<String>,
    pub(crate) last_error: Option<String>,
    pub(crate) last_title: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotificationStatus {
    pub(crate) diagnostics: NotificationDiagnostics,
    pub(crate) test_available: bool,
    pub(crate) permission_state: NotificationPermissionState,
    pub(crate) rationale_required: bool,
    pub(crate) settings_action_available: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum NotificationPermissionState {
    Unknown,
    Granted,
    Prompt,
    Denied,
    Unsupported,
}

impl NotificationPermissionState {
    pub(crate) fn is_denied(self) -> bool {
        matches!(self, Self::Denied)
    }

    pub(crate) fn needs_rationale(self) -> bool {
        matches!(self, Self::Prompt)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotificationSettingsOpenResult {
    pub(crate) opened: bool,
    pub(crate) user_message: String,
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
    pub(crate) threshold: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetCostUsageRangeDaysArgs {
    pub(crate) range_days: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetLaunchOnLoginArgs {
    pub(crate) enabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppUpdateInfo {
    pub(crate) version: String,
    pub(crate) current_version: String,
    pub(crate) date: Option<String>,
    pub(crate) body: Option<String>,
    pub(crate) can_install: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppUpdateProgress {
    pub(crate) phase: String,
    pub(crate) downloaded: u64,
    pub(crate) chunk_length: Option<usize>,
    pub(crate) content_length: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CostUsageDailyPoint {
    pub(crate) day_key: String,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) project: Option<String>,
    pub(crate) input_tokens: i64,
    pub(crate) cached_input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) total_tokens: i64,
    #[serde(default)]
    pub(crate) cost_usd: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CostUsageSnapshot {
    pub(crate) today_tokens: i64,
    pub(crate) today_cost_usd: Option<f64>,
    pub(crate) last_30_days_tokens: i64,
    pub(crate) last_30_days_cost_usd: Option<f64>,
    #[serde(default = "default_cost_usage_range_days")]
    pub(crate) range_days: u16,
    #[serde(default)]
    pub(crate) timezone: Option<String>,
    #[serde(default)]
    pub(crate) today_key: Option<String>,
    #[serde(default)]
    pub(crate) scan_stats: Option<CostUsageScanStats>,
    pub(crate) daily: Vec<CostUsageDailyPoint>,
    pub(crate) updated_at: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexOverviewSnapshot {
    pub(crate) accounts: Vec<AccountSummary>,
    pub(crate) usage_by_account_id: HashMap<String, UsageSnapshot>,
    pub(crate) errors_by_account_id: HashMap<String, AccountIssue>,
    #[serde(default)]
    pub(crate) diagnostics_by_account_id: HashMap<String, AccountRefreshDiagnostics>,
    #[serde(default)]
    pub(crate) stale_reason: Option<String>,
    #[serde(default)]
    pub(crate) last_successful_at: Option<i64>,
    #[serde(default)]
    pub(crate) last_attempt_at: Option<i64>,
    #[serde(default)]
    pub(crate) quota_events: Vec<QuotaEvent>,
    pub(crate) cost_usage: Option<CostUsageSnapshot>,
    pub(crate) cost_error: Option<String>,
    pub(crate) generated_at: i64,
    pub(crate) stale: bool,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CostUsageScanStats {
    pub(crate) files_scanned: usize,
    pub(crate) files_reused: usize,
    pub(crate) files_removed: usize,
    pub(crate) events_retained: usize,
    pub(crate) retention_days: u16,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountRefreshDiagnostics {
    #[serde(default)]
    pub(crate) attempts: Vec<ProviderFetchAttempt>,
    pub(crate) last_successful_at: Option<i64>,
    pub(crate) last_attempt_at: Option<i64>,
    pub(crate) stale_reason: Option<String>,
    pub(crate) cache_status: Option<String>,
    pub(crate) scan_stats: Option<String>,
    pub(crate) auto_switch_preview: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderFetchAttempt {
    pub(crate) provider_id: String,
    pub(crate) source_mode: CodexUsageSourceMode,
    pub(crate) status: String,
    pub(crate) started_at: i64,
    pub(crate) finished_at: Option<i64>,
    pub(crate) error_class: Option<String>,
    pub(crate) error_code: Option<String>,
    pub(crate) message: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountIssue {
    #[expect(
        dead_code,
        reason = "error codes are retained for non-string UI decisions as surfaces grow"
    )]
    pub(crate) code: String,
    pub(crate) user_message: String,
    pub(crate) auth_related: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageWindow {
    pub(crate) label: String,
    pub(crate) used_percent: f64,
    pub(crate) remaining_percent: f64,
    pub(crate) reset_at: Option<i64>,
    pub(crate) window_seconds: Option<i64>,
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
    #[expect(
        dead_code,
        reason = "callers currently show only user_message but code stays in the IPC contract"
    )]
    pub(crate) code: String,
    pub(crate) user_message: String,
}

impl CommandError {
    pub(crate) fn from_message(message: String) -> Self {
        Self {
            code: "client_error".to_string(),
            user_message: message,
        }
    }
}

pub(crate) async fn refresh_snapshot(
    cmd: &str,
    force: bool,
) -> Result<CodexOverviewSnapshot, CommandError> {
    let args = serde_wasm_bindgen::to_value(&RefreshSnapshotArgs { force })
        .map_err(|error| CommandError::from_message(error.to_string()))?;
    invoke_tauri(cmd, args).await
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
    let policy = policy_for_command(cmd);
    let value = invoke_with_policy(
        cmd,
        args,
        policy.timeout_ms,
        policy.retries,
        policy.retry_delay_ms,
    )
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
    let code = js_sys::Reflect::get(value, &JsValue::from_str("code"))
        .ok()
        .and_then(|value| value.as_string())
        .unwrap_or_else(|| "unknown_error".to_string());
    let user_message = js_sys::Reflect::get(value, &JsValue::from_str("userMessage"))
        .ok()
        .and_then(|value| value.as_string())
        .or_else(|| {
            js_sys::Reflect::get(value, &JsValue::from_str("message"))
                .ok()
                .and_then(|value| value.as_string())
        })
        .or_else(|| value.as_string())
        .unwrap_or_else(|| "Wovo could not complete the request.".to_string());

    CommandError { code, user_message }
}

#[derive(Clone, Copy)]
struct InvokePolicy {
    timeout_ms: u32,
    retries: u32,
    retry_delay_ms: u32,
}

fn policy_for_command(cmd: &str) -> InvokePolicy {
    match cmd {
        "get_cached_claude_snapshot"
        | "get_cached_codex_snapshot"
        | "get_claude_settings"
        | "get_codex_settings"
        | "get_codex_notification_status"
        | "validate_wovo_config"
        | "check_app_update" => InvokePolicy {
            timeout_ms: 10_000,
            retries: 2,
            retry_delay_ms: 1_000,
        },
        "refresh_claude_snapshot" | "refresh_claude_usage" | "refresh_all_claude_usage" => {
            InvokePolicy {
                timeout_ms: 0,
                retries: 0,
                retry_delay_ms: 0,
            }
        }
        "refresh_codex_snapshot" | "refresh_codex_usage" | "refresh_all_usage" => InvokePolicy {
            timeout_ms: 60_000,
            retries: 0,
            retry_delay_ms: 0,
        },
        "set_claude_usage_source_mode"
        | "set_claude_cost_usage_enabled"
        | "set_claude_notifications_enabled"
        | "set_claude_auto_account_switching_enabled"
        | "set_claude_auto_switch_threshold_percent"
        | "set_claude_cost_usage_range_days"
        | "set_claude_hide_account_credentials"
        | "set_codex_usage_source_mode"
        | "set_codex_cost_usage_enabled"
        | "set_codex_notifications_enabled"
        | "set_codex_auto_account_switching_enabled"
        | "set_codex_auto_switch_threshold_percent"
        | "set_codex_cost_usage_range_days"
        | "set_codex_hide_account_credentials"
        | "set_codex_launch_on_login"
        | "open_notification_settings" => InvokePolicy {
            timeout_ms: 15_000,
            retries: 0,
            retry_delay_ms: 0,
        },
        "add_claude_account"
        | "reauthenticate_claude_account"
        | "cancel_claude_account_login"
        | "remove_claude_account"
        | "set_system_claude_account"
        | "add_codex_account"
        | "reauthenticate_codex_account"
        | "cancel_codex_account_login"
        | "remove_codex_account"
        | "set_system_codex_account"
        | "send_codex_test_notification"
        | "install_app_update" => InvokePolicy {
            timeout_ms: 0,
            retries: 0,
            retry_delay_ms: 0,
        },
        _ => InvokePolicy {
            timeout_ms: 30_000,
            retries: 0,
            retry_delay_ms: 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_commands_use_bounded_retry_policy() {
        let policy = policy_for_command("get_codex_settings");
        assert_eq!(policy.timeout_ms, 10_000);
        assert_eq!(policy.retries, 2);
    }

    #[test]
    fn mutating_commands_do_not_retry() {
        let policy = policy_for_command("set_codex_notifications_enabled");
        assert_eq!(policy.timeout_ms, 15_000);
        assert_eq!(policy.retries, 0);
    }

    #[test]
    fn login_commands_are_not_timed_out_by_frontend() {
        let policy = policy_for_command("add_codex_account");
        assert_eq!(policy.timeout_ms, 0);
        assert_eq!(policy.retries, 0);
    }

    #[test]
    fn claude_refresh_commands_are_not_timed_out_by_frontend() {
        let policy = policy_for_command("refresh_claude_snapshot");
        assert_eq!(policy.timeout_ms, 0);
        assert_eq!(policy.retries, 0);

        let policy = policy_for_command("refresh_all_claude_usage");
        assert_eq!(policy.timeout_ms, 0);
        assert_eq!(policy.retries, 0);
    }

    #[test]
    fn codex_refresh_commands_keep_bounded_frontend_timeout() {
        let policy = policy_for_command("refresh_codex_snapshot");
        assert_eq!(policy.timeout_ms, 60_000);
        assert_eq!(policy.retries, 0);
    }
}
