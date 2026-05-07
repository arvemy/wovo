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
