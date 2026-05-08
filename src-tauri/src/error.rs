use serde::ser::{Serialize, SerializeStruct, Serializer};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Codex auth.json was not found. Run `codex login` first.")]
    AuthNotFound,
    #[error("Codex auth.json could not be read: {0}")]
    AuthRead(String),
    #[error("Codex auth.json could not be decoded: {0}")]
    AuthDecode(String),
    #[error("Codex auth.json does not contain OAuth tokens.")]
    MissingTokens,
    #[error("Unknown Codex account: {0}")]
    UnknownAccount(String),
    #[error("The Codex CLI was not found on PATH.")]
    CodexBinaryNotFound,
    #[error("Codex login timed out.")]
    CodexLoginTimedOut,
    #[error("Codex login was cancelled.")]
    CodexLoginCancelled,
    #[error("Codex login is already running.")]
    CodexLoginInProgress,
    #[error("Codex login failed: {0}")]
    CodexLoginFailed(String),
    #[error("Codex account storage failed: {0}")]
    AccountStore(String),
    #[error("This Codex account has already been added.")]
    AccountAlreadyExists,
    #[error("Switch to another Codex account before removing the active account.")]
    ActiveAccountRemovalBlocked,
    #[error("The live system Codex account cannot be removed while it is still signed in.")]
    LiveAccountRemovalBlocked,
    #[error("The signed-in Codex account does not match the selected account.")]
    AccountIdentityMismatch,
    #[error("Refusing to delete unsafe managed Codex home: {0}")]
    UnsafeManagedHome(String),
    #[error("Codex token refresh failed: {0}")]
    TokenRefresh(String),
    #[error("Codex usage request failed: {0}")]
    UsageFetch(String),
    #[error("Codex usage response was not usable.")]
    InvalidUsageResponse,
}

impl AppError {
    fn code(&self) -> &'static str {
        match self {
            Self::AuthNotFound => "auth_not_found",
            Self::AuthRead(_) => "auth_read",
            Self::AuthDecode(_) => "auth_decode",
            Self::MissingTokens => "missing_tokens",
            Self::UnknownAccount(_) => "unknown_account",
            Self::CodexBinaryNotFound => "codex_binary_not_found",
            Self::CodexLoginTimedOut => "codex_login_timed_out",
            Self::CodexLoginCancelled => "codex_login_cancelled",
            Self::CodexLoginInProgress => "codex_login_in_progress",
            Self::CodexLoginFailed(_) => "codex_login_failed",
            Self::AccountStore(_) => "account_store",
            Self::AccountAlreadyExists => "account_already_exists",
            Self::ActiveAccountRemovalBlocked => "active_account_removal_blocked",
            Self::LiveAccountRemovalBlocked => "live_account_removal_blocked",
            Self::AccountIdentityMismatch => "account_identity_mismatch",
            Self::UnsafeManagedHome(_) => "unsafe_managed_home",
            Self::TokenRefresh(_) => "token_refresh",
            Self::UsageFetch(_) => "usage_fetch",
            Self::InvalidUsageResponse => "invalid_usage_response",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}
