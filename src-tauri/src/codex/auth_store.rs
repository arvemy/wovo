use crate::domain::account::AccountSummary;
use crate::error::AppError;
use base64::prelude::{Engine as _, BASE64_URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, Clone)]
pub struct CodexOAuthCredentials {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: Option<String>,
    pub account_id: Option<String>,
    pub last_refresh: Option<OffsetDateTime>,
    pub home_path: PathBuf,
}

impl CodexOAuthCredentials {
    pub fn needs_refresh(&self) -> bool {
        let Some(last_refresh) = self.last_refresh else {
            return true;
        };

        let age = OffsetDateTime::now_utc() - last_refresh;
        age.whole_seconds() > 8 * 24 * 60 * 60
    }

    pub fn email(&self) -> Option<String> {
        self.id_token.as_deref().and_then(email_from_id_token)
    }

    pub fn provider_account_id(&self) -> Option<String> {
        self.account_id.clone().or_else(|| {
            self.id_token
                .as_deref()
                .and_then(provider_account_id_from_id_token)
        })
    }
}

#[derive(Debug, Deserialize)]
struct AuthFile {
    #[serde(rename = "OPENAI_API_KEY")]
    _openai_api_key: Option<String>,
    tokens: Option<AuthTokens>,
    last_refresh: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AuthTokens {
    #[serde(alias = "accessToken")]
    access_token: Option<String>,
    #[serde(alias = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(alias = "idToken")]
    id_token: Option<String>,
    #[serde(alias = "accountId")]
    account_id: Option<String>,
}

pub fn system_codex_home_path() -> PathBuf {
    if let Ok(raw) = env::var("CODEX_HOME") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    dirs_home().join(".codex")
}

pub fn detected_ambient_account() -> Result<Option<AccountSummary>, AppError> {
    match load_ambient_credentials() {
        Ok(credentials) => Ok(Some(AccountSummary::ambient(
            credentials.home_path.to_string_lossy().to_string(),
            credentials.email(),
            credentials.provider_account_id(),
            None,
            None,
        ))),
        Err(AppError::AuthNotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn load_ambient_credentials() -> Result<CodexOAuthCredentials, AppError> {
    load_credentials_from_home(&system_codex_home_path())
}

pub fn load_credentials_from_home(home_path: &Path) -> Result<CodexOAuthCredentials, AppError> {
    let auth_path = home_path.join("auth.json");
    if !auth_path.exists() {
        return Err(AppError::AuthNotFound);
    }

    let contents =
        fs::read_to_string(&auth_path).map_err(|error| AppError::AuthRead(error.to_string()))?;
    parse_auth_json(&contents, home_path.to_path_buf())
}

pub fn save_credentials(credentials: &CodexOAuthCredentials) -> Result<(), AppError> {
    let auth_path = credentials.home_path.join("auth.json");
    if let Some(parent) = auth_path.parent() {
        fs::create_dir_all(parent).map_err(|error| AppError::AuthRead(error.to_string()))?;
    }
    let contents =
        fs::read_to_string(&auth_path).map_err(|error| AppError::AuthRead(error.to_string()))?;
    let mut root: Value =
        serde_json::from_str(&contents).map_err(|error| AppError::AuthDecode(error.to_string()))?;

    let tokens = AuthTokens {
        access_token: Some(credentials.access_token.clone()),
        refresh_token: Some(credentials.refresh_token.clone()),
        id_token: credentials.id_token.clone(),
        account_id: credentials.account_id.clone(),
    };

    root["tokens"] =
        serde_json::to_value(tokens).map_err(|error| AppError::AuthDecode(error.to_string()))?;
    root["last_refresh"] = Value::String(
        credentials
            .last_refresh
            .unwrap_or_else(OffsetDateTime::now_utc)
            .format(&Rfc3339)
            .map_err(|error| AppError::AuthDecode(error.to_string()))?,
    );

    let next = serde_json::to_string_pretty(&root)
        .map_err(|error| AppError::AuthDecode(error.to_string()))?;
    write_auth_json(&auth_path, next.as_bytes())
}

pub fn replace_auth_json_from_home(source_home: &Path, target_home: &Path) -> Result<(), AppError> {
    let contents = fs::read(source_home.join("auth.json"))
        .map_err(|error| AppError::AuthRead(error.to_string()))?;
    let target_auth = target_home.join("auth.json");
    write_auth_json(&target_auth, &contents)
}

fn write_auth_json(auth_path: &Path, contents: &[u8]) -> Result<(), AppError> {
    let parent = auth_path.parent().ok_or_else(|| {
        AppError::AuthRead(format!(
            "auth path has no parent: {}",
            auth_path.to_string_lossy()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| AppError::AuthRead(error.to_string()))?;

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let tmp = parent.join(format!(".auth.json.{nonce}.tmp"));
    fs::write(&tmp, contents).map_err(|error| AppError::AuthRead(error.to_string()))?;
    apply_secure_file_permissions(&tmp)?;

    match fs::rename(&tmp, auth_path) {
        Ok(()) => Ok(()),
        Err(error) => {
            #[cfg(windows)]
            {
                if auth_path.exists() {
                    fs::remove_file(auth_path)
                        .map_err(|remove_error| AppError::AuthRead(remove_error.to_string()))?;
                    fs::rename(&tmp, auth_path)
                        .map_err(|rename_error| AppError::AuthRead(rename_error.to_string()))
                } else {
                    let _ = fs::remove_file(&tmp);
                    Err(AppError::AuthRead(error.to_string()))
                }
            }

            #[cfg(not(windows))]
            {
                let _ = fs::remove_file(&tmp);
                Err(AppError::AuthRead(error.to_string()))
            }
        }
    }
}

fn parse_auth_json(contents: &str, home_path: PathBuf) -> Result<CodexOAuthCredentials, AppError> {
    let auth: AuthFile =
        serde_json::from_str(contents).map_err(|error| AppError::AuthDecode(error.to_string()))?;
    let tokens = auth.tokens.ok_or(AppError::MissingTokens)?;
    let access_token = required_token(tokens.access_token)?;
    let refresh_token = tokens.refresh_token.unwrap_or_default();
    let last_refresh = auth
        .last_refresh
        .as_deref()
        .and_then(|value| OffsetDateTime::parse(value, &Rfc3339).ok());

    Ok(CodexOAuthCredentials {
        access_token,
        refresh_token,
        id_token: tokens.id_token,
        account_id: tokens.account_id,
        last_refresh,
        home_path,
    })
}

fn required_token(value: Option<String>) -> Result<String, AppError> {
    let Some(token) = value else {
        return Err(AppError::MissingTokens);
    };
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(AppError::MissingTokens);
    }
    Ok(trimmed.to_string())
}

fn email_from_id_token(id_token: &str) -> Option<String> {
    jwt_claims(id_token)?
        .get("email")?
        .as_str()
        .map(str::to_string)
}

fn provider_account_id_from_id_token(id_token: &str) -> Option<String> {
    let claims = jwt_claims(id_token)?;
    claims
        .get("https://api.openai.com/auth")
        .and_then(|value| value.get("chatgpt_account_id"))
        .or_else(|| claims.get("chatgpt_account_id"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn jwt_claims(id_token: &str) -> Option<Value> {
    let payload = id_token.split('.').nth(1)?;
    let decoded = BASE64_URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&decoded).ok()
}

fn dirs_home() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
        })
}

#[cfg(unix)]
fn apply_secure_file_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions).map_err(|error| AppError::AuthRead(error.to_string()))
}

#[cfg(not(unix))]
fn apply_secure_file_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_auth_json() {
        let auth = r#"{
            "tokens": {
                "access_token": "access",
                "refresh_token": "refresh",
                "id_token": "header.eyJlbWFpbCI6InRlc3RAZXhhbXBsZS5jb20ifQ.signature",
                "account_id": "account-123"
            },
            "last_refresh": "2026-05-01T12:00:00Z"
        }"#;

        let credentials = parse_auth_json(auth, PathBuf::from("/tmp/codex")).unwrap();
        assert_eq!(credentials.access_token, "access");
        assert_eq!(credentials.refresh_token, "refresh");
        assert_eq!(credentials.account_id.as_deref(), Some("account-123"));
        assert_eq!(credentials.email().as_deref(), Some("test@example.com"));
        assert!(credentials.last_refresh.is_some());
    }

    #[test]
    fn rejects_tokenless_auth_json() {
        let error = parse_auth_json(r#"{"tokens": {}}"#, PathBuf::from("/tmp/codex")).unwrap_err();
        assert!(matches!(error, AppError::MissingTokens));
    }
}
