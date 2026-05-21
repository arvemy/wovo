use serde::ser::{Serialize, SerializeStruct, Serializer};
use std::borrow::Cow;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Codex auth.json was not found. Run `codex login` first.")]
    AuthNotFound,
    #[error("Codex auth.json could not be read: {0}")]
    AuthRead(String),
    #[error("Codex auth.json could not be written: {0}")]
    AuthWrite(String),
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
    #[error("Claude credentials were not found. Run `claude /login` first.")]
    ClaudeAuthNotFound,
    #[error("Claude credentials could not be read: {0}")]
    ClaudeAuthRead(String),
    #[error("Claude credentials could not be written: {0}")]
    ClaudeAuthWrite(String),
    #[error("Claude credentials could not be decoded: {0}")]
    ClaudeAuthDecode(String),
    #[error("Claude credentials do not contain OAuth tokens.")]
    ClaudeMissingTokens,
    #[error("Unknown Claude account: {0}")]
    ClaudeUnknownAccount(String),
    #[error("The Claude CLI was not found on PATH.")]
    ClaudeBinaryNotFound,
    #[error("Claude login timed out.")]
    ClaudeLoginTimedOut,
    #[error("Claude login was cancelled.")]
    ClaudeLoginCancelled,
    #[error("Claude login is already running.")]
    ClaudeLoginInProgress,
    #[error("Claude login failed: {0}")]
    ClaudeLoginFailed(String),
    #[error("Claude account storage failed: {0}")]
    ClaudeAccountStore(String),
    #[error("This Claude account has already been added.")]
    ClaudeAccountAlreadyExists,
    #[error("The live system Claude account cannot be removed while it is still signed in.")]
    ClaudeLiveAccountRemovalBlocked,
    #[error("The signed-in Claude account does not match the selected account.")]
    ClaudeAccountIdentityMismatch,
    #[error("Refusing to delete unsafe managed Claude home: {0}")]
    ClaudeUnsafeManagedHome(String),
    #[error("Claude token refresh failed: {0}")]
    ClaudeTokenRefresh(String),
    #[error("Claude usage request failed: {0}")]
    ClaudeUsageFetch(String),
    #[error("Claude usage response was not usable.")]
    ClaudeInvalidUsageResponse,
    #[error("Launch-on-login registration failed: {0}")]
    LaunchOnLogin(String),
    #[error("Notification failed: {0}")]
    Notification(String),
    #[error("App update failed: {0}")]
    AppUpdate(String),
}

impl AppError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::AuthNotFound => "auth_not_found",
            Self::AuthRead(_) => "auth_read",
            Self::AuthWrite(_) => "auth_write",
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
            Self::LiveAccountRemovalBlocked => "live_account_removal_blocked",
            Self::AccountIdentityMismatch => "account_identity_mismatch",
            Self::UnsafeManagedHome(_) => "unsafe_managed_home",
            Self::TokenRefresh(_) => "token_refresh",
            Self::UsageFetch(_) => "usage_fetch",
            Self::InvalidUsageResponse => "invalid_usage_response",
            Self::ClaudeAuthNotFound => "claude_auth_not_found",
            Self::ClaudeAuthRead(_) => "claude_auth_read",
            Self::ClaudeAuthWrite(_) => "claude_auth_write",
            Self::ClaudeAuthDecode(_) => "claude_auth_decode",
            Self::ClaudeMissingTokens => "claude_missing_tokens",
            Self::ClaudeUnknownAccount(_) => "claude_unknown_account",
            Self::ClaudeBinaryNotFound => "claude_binary_not_found",
            Self::ClaudeLoginTimedOut => "claude_login_timed_out",
            Self::ClaudeLoginCancelled => "claude_login_cancelled",
            Self::ClaudeLoginInProgress => "claude_login_in_progress",
            Self::ClaudeLoginFailed(_) => "claude_login_failed",
            Self::ClaudeAccountStore(_) => "claude_account_store",
            Self::ClaudeAccountAlreadyExists => "claude_account_already_exists",
            Self::ClaudeLiveAccountRemovalBlocked => "claude_live_account_removal_blocked",
            Self::ClaudeAccountIdentityMismatch => "claude_account_identity_mismatch",
            Self::ClaudeUnsafeManagedHome(_) => "claude_unsafe_managed_home",
            Self::ClaudeTokenRefresh(_) => "claude_token_refresh",
            Self::ClaudeUsageFetch(_) => "claude_usage_fetch",
            Self::ClaudeInvalidUsageResponse => "claude_invalid_usage_response",
            Self::LaunchOnLogin(_) => "launch_on_login",
            Self::Notification(_) => "notification",
            Self::AppUpdate(_) => "app_update",
        }
    }

    pub(crate) fn user_message(&self) -> Cow<'static, str> {
        match self {
            Self::AuthNotFound => Cow::Borrowed("Codex is not signed in. Run `codex login` first."),
            Self::AuthRead(_) => Cow::Borrowed("Codex credentials could not be read."),
            Self::AuthWrite(_) => Cow::Borrowed("Codex credentials could not be saved."),
            Self::AuthDecode(_) => Cow::Borrowed("Codex credentials could not be decoded."),
            Self::MissingTokens => Cow::Borrowed("Codex credentials do not contain OAuth tokens."),
            Self::UnknownAccount(_) => Cow::Borrowed("The selected Codex account was not found."),
            Self::CodexBinaryNotFound => Cow::Borrowed("The Codex CLI was not found on PATH."),
            Self::CodexLoginTimedOut => Cow::Borrowed("Codex login timed out."),
            Self::CodexLoginCancelled => Cow::Borrowed("Codex login was cancelled."),
            Self::CodexLoginInProgress => Cow::Borrowed("Codex login is already running."),
            Self::CodexLoginFailed(_) => Cow::Borrowed("Codex login failed."),
            Self::AccountStore(_) => Cow::Borrowed("Codex account storage failed."),
            Self::AccountAlreadyExists => {
                Cow::Borrowed("This Codex account has already been added.")
            }
            Self::LiveAccountRemovalBlocked => Cow::Borrowed(
                "The live system Codex account cannot be removed while it is still signed in.",
            ),
            Self::AccountIdentityMismatch => {
                Cow::Borrowed("The signed-in Codex account does not match the selected account.")
            }
            Self::UnsafeManagedHome(_) => {
                Cow::Borrowed("Wovo refused to delete an unsafe managed Codex home.")
            }
            Self::TokenRefresh(_) => {
                Cow::Borrowed("Codex sign-in needs attention. Re-authenticate this account.")
            }
            Self::UsageFetch(_) => Cow::Borrowed("Codex usage could not be refreshed."),
            Self::InvalidUsageResponse => Cow::Borrowed("Codex usage response was not usable."),
            Self::ClaudeAuthNotFound => {
                Cow::Borrowed("Claude Code is not signed in. Run `claude /login` first.")
            }
            Self::ClaudeAuthRead(_) => Cow::Borrowed("Claude credentials could not be read."),
            Self::ClaudeAuthWrite(_) => Cow::Borrowed("Claude credentials could not be saved."),
            Self::ClaudeAuthDecode(_) => Cow::Borrowed("Claude credentials could not be decoded."),
            Self::ClaudeMissingTokens => {
                Cow::Borrowed("Claude credentials do not contain OAuth tokens.")
            }
            Self::ClaudeUnknownAccount(_) => {
                Cow::Borrowed("The selected Claude account was not found.")
            }
            Self::ClaudeBinaryNotFound => Cow::Borrowed("The Claude CLI was not found on PATH."),
            Self::ClaudeLoginTimedOut => Cow::Borrowed("Claude login timed out."),
            Self::ClaudeLoginCancelled => Cow::Borrowed("Claude login was cancelled."),
            Self::ClaudeLoginInProgress => Cow::Borrowed("Claude login is already running."),
            Self::ClaudeLoginFailed(_) => Cow::Borrowed("Claude login failed."),
            Self::ClaudeAccountStore(_) => Cow::Borrowed("Claude account storage failed."),
            Self::ClaudeAccountAlreadyExists => {
                Cow::Borrowed("This Claude account has already been added.")
            }
            Self::ClaudeLiveAccountRemovalBlocked => Cow::Borrowed(
                "The live system Claude account cannot be removed while it is still signed in.",
            ),
            Self::ClaudeAccountIdentityMismatch => {
                Cow::Borrowed("The signed-in Claude account does not match the selected account.")
            }
            Self::ClaudeUnsafeManagedHome(_) => {
                Cow::Borrowed("Wovo refused to delete an unsafe managed Claude home.")
            }
            Self::ClaudeTokenRefresh(_) => {
                Cow::Borrowed("Claude Code sign-in needs attention. Re-authenticate this account.")
            }
            Self::ClaudeUsageFetch(_) => Cow::Borrowed("Claude usage could not be refreshed."),
            Self::ClaudeInvalidUsageResponse => {
                Cow::Borrowed("Claude usage response was not usable.")
            }
            Self::LaunchOnLogin(_) => Cow::Borrowed("Launch at login could not be updated."),
            Self::Notification(_) => Cow::Borrowed("Notification setup could not be completed."),
            Self::AppUpdate(_) => Cow::Borrowed("The app update could not be completed."),
        }
    }

    pub(crate) fn auth_related(&self) -> bool {
        match self {
            Self::AuthNotFound
            | Self::MissingTokens
            | Self::AuthDecode(_)
            | Self::ClaudeAuthNotFound
            | Self::ClaudeMissingTokens
            | Self::ClaudeAuthDecode(_) => true,
            Self::TokenRefresh(message) => {
                let message = message.to_ascii_lowercase();
                message.contains("invalid_grant")
                    || message.contains("unauthorized")
                    || message.contains("revoked")
                    || message.contains("expired")
                    || message.contains("status 401")
                    || message.contains("status 403")
            }
            Self::UsageFetch(message) => {
                let message = message.to_ascii_lowercase();
                message.contains("status 401") || message.contains("status 403")
            }
            Self::ClaudeTokenRefresh(message) => {
                let message = message.to_ascii_lowercase();
                message.contains("invalid_grant")
                    || message.contains("unauthorized")
                    || message.contains("revoked")
                    || message.contains("expired")
                    || message.contains("status 401")
                    || message.contains("status 403")
            }
            Self::ClaudeUsageFetch(message) => {
                let message = message.to_ascii_lowercase();
                message.contains("status 401")
                    || message.contains("status 403")
                    || message.contains("unauthorized")
                    || message.contains("expired")
            }
            _ => false,
        }
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("AppError", 3)?;
        state.serialize_field("code", self.code())?;
        let user_message = self.user_message();
        state.serialize_field("userMessage", user_message.as_ref())?;
        state.serialize_field("message", user_message.as_ref())?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn serializes_user_message_without_internal_detail() {
        let error = AppError::UsageFetch("status 500: upstream stack detail".to_string());
        let value = serde_json::to_value(error).unwrap();

        assert_eq!(value["code"], Value::String("usage_fetch".to_string()));
        assert_eq!(
            value["userMessage"],
            Value::String("Codex usage could not be refreshed.".to_string())
        );
        assert!(!value.to_string().contains("upstream stack detail"));
    }
}
