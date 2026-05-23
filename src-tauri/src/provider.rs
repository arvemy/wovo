use crate::error::AppError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Codex,
    Claude,
}

impl ProviderId {
    pub fn root_dir_name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderSourceMode {
    Auto,
    Oauth,
    Cli,
    Cached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderFetchAttemptStatus {
    Success,
    Skipped,
    Fallback,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderFetchErrorClass {
    Auth,
    Credentials,
    Network,
    RateLimit,
    Transient,
    Decode,
    Command,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderFetchAttempt {
    pub provider_id: ProviderId,
    pub source_mode: ProviderSourceMode,
    pub status: ProviderFetchAttemptStatus,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub error_class: Option<ProviderFetchErrorClass>,
    pub error_code: Option<String>,
    pub message: Option<String>,
}

impl ProviderFetchAttempt {
    pub fn success(
        provider_id: ProviderId,
        source_mode: ProviderSourceMode,
        started_at: i64,
    ) -> Self {
        Self {
            provider_id,
            source_mode,
            status: ProviderFetchAttemptStatus::Success,
            started_at,
            finished_at: Some(now_utc_timestamp()),
            error_class: None,
            error_code: None,
            message: None,
        }
    }

    pub fn failed(
        provider_id: ProviderId,
        source_mode: ProviderSourceMode,
        started_at: i64,
        error: &AppError,
    ) -> Self {
        Self {
            provider_id,
            source_mode,
            status: ProviderFetchAttemptStatus::Failed,
            started_at,
            finished_at: Some(now_utc_timestamp()),
            error_class: Some(error_class(error)),
            error_code: Some(error.code().to_string()),
            message: Some(error.user_message().into_owned()),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRefreshDiagnostics {
    #[serde(default)]
    pub attempts: Vec<ProviderFetchAttempt>,
    #[serde(default)]
    pub last_successful_at: Option<i64>,
    #[serde(default)]
    pub last_attempt_at: Option<i64>,
    #[serde(default)]
    pub stale_reason: Option<String>,
    #[serde(default)]
    pub cache_status: Option<String>,
    #[serde(default)]
    pub scan_stats: Option<String>,
    #[serde(default)]
    pub auto_switch_preview: Option<String>,
}

impl AccountRefreshDiagnostics {
    pub fn from_attempts(attempts: Vec<ProviderFetchAttempt>) -> Self {
        let last_attempt_at = attempts
            .iter()
            .filter_map(|attempt| attempt.finished_at.or(Some(attempt.started_at)))
            .max();
        let last_successful_at = attempts
            .iter()
            .filter(|attempt| attempt.status == ProviderFetchAttemptStatus::Success)
            .filter_map(|attempt| attempt.finished_at.or(Some(attempt.started_at)))
            .max();

        Self {
            attempts,
            last_successful_at,
            last_attempt_at,
            stale_reason: None,
            cache_status: None,
            scan_stats: None,
            auto_switch_preview: None,
        }
    }

    pub fn mark_cached(mut self, reason: impl Into<String>) -> Self {
        self.stale_reason = Some(reason.into());
        self.cache_status = Some("cached".to_string());
        self
    }
}

// TODO(post-launch): extract a shared snapshot coordinator covering account
// listing, fetch dispatch, and diagnostics aggregation. Today, snapshot.rs and
// claude/snapshot.rs duplicate ~95% of refresh_locked; the trait-based seam
// was deferred until both providers exercise the same set of call sites.

pub fn now_utc_timestamp() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

pub fn error_class(error: &AppError) -> ProviderFetchErrorClass {
    if error.auth_related() {
        return ProviderFetchErrorClass::Auth;
    }

    let message = error.to_string().to_ascii_lowercase();
    if message.contains("credential") || message.contains("token") || message.contains("oauth") {
        ProviderFetchErrorClass::Credentials
    } else if message.contains("429") || message.contains("rate limit") {
        ProviderFetchErrorClass::RateLimit
    } else if message.contains("timeout")
        || message.contains("timed out")
        || message.contains("temporar")
        || message.contains("connection reset")
    {
        ProviderFetchErrorClass::Transient
    } else if message.contains("network")
        || message.contains("dns")
        || message.contains("connect")
        || message.contains("request")
    {
        ProviderFetchErrorClass::Network
    } else if message.contains("decode") || message.contains("json") || message.contains("parse") {
        ProviderFetchErrorClass::Decode
    } else if message.contains("cli") || message.contains("app-server") {
        ProviderFetchErrorClass::Command
    } else {
        ProviderFetchErrorClass::Unknown
    }
}
