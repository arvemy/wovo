use crate::codex::atomic_file::{replace_file, temporary_file_path, write_new_file};
use crate::domain::account::AccountSummary;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

const CREDENTIALS_FILE_NAME: &str = ".credentials.json";
const CONFIG_FILE_NAME: &str = ".claude.json";

#[derive(Debug, Clone)]
pub struct ClaudeOAuthCredentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<OffsetDateTime>,
    pub scopes: Vec<String>,
    pub rate_limit_tier: Option<String>,
    pub subscription_type: Option<String>,
    pub client_id: Option<String>,
    pub home_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ClaudeAccountProfile {
    pub email: Option<String>,
    pub organization: Option<String>,
}

impl ClaudeOAuthCredentials {
    pub fn plan_type(&self) -> Option<String> {
        self.subscription_type
            .clone()
            .and_then(normalize_plan_label)
            .or_else(|| self.rate_limit_tier.clone())
    }

    pub fn provider_account_id(&self) -> Option<String> {
        self.refresh_token
            .as_deref()
            .filter(|token| !token.trim().is_empty())
            .or(Some(self.access_token.as_str()))
            .map(token_fingerprint)
    }

    pub fn has_profile_scope(&self) -> bool {
        self.scopes.iter().any(|scope| scope == "user:profile")
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|expires_at| OffsetDateTime::now_utc() >= expires_at)
            .unwrap_or(false)
    }
}

#[derive(Debug, Deserialize)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<ClaudeOAuthPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeConfigFile {
    oauth_account: Option<ClaudeConfigOAuthAccount>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeConfigOAuthAccount {
    email_address: Option<String>,
    organization_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeOAuthPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scopes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rate_limit_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subscription_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_id: Option<String>,
}

pub fn system_claude_home_path() -> PathBuf {
    if let Ok(raw) = env::var("CLAUDE_CONFIG_DIR") {
        for part in raw.split(',') {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
    }

    dirs_home().join(".claude")
}

pub fn detected_ambient_account() -> Result<Option<AccountSummary>, AppError> {
    match load_ambient_credentials() {
        Ok(credentials) => {
            let profile = load_account_profile_from_home(&credentials.home_path);
            Ok(Some(AccountSummary::ambient(
                credentials.home_path.to_string_lossy().to_string(),
                profile.as_ref().and_then(|profile| profile.email.clone()),
                credentials.provider_account_id(),
                None,
                profile
                    .and_then(|profile| profile.organization)
                    .or_else(|| credentials.plan_type()),
            )))
        }
        Err(AppError::ClaudeAuthNotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn load_ambient_credentials() -> Result<ClaudeOAuthCredentials, AppError> {
    load_credentials_from_home(&system_claude_home_path())
}

pub fn load_credentials_from_home(home_path: &Path) -> Result<ClaudeOAuthCredentials, AppError> {
    let credentials_path = home_path.join(CREDENTIALS_FILE_NAME);
    if !credentials_path.exists() {
        return Err(AppError::ClaudeAuthNotFound);
    }

    let contents = fs::read_to_string(&credentials_path)
        .map_err(|error| AppError::ClaudeAuthRead(error.to_string()))?;
    parse_credentials_json(&contents, home_path.to_path_buf())
}

pub fn load_account_profile_from_home(home_path: &Path) -> Option<ClaudeAccountProfile> {
    let contents = fs::read_to_string(home_path.join(CONFIG_FILE_NAME)).ok()?;
    let decoded: ClaudeConfigFile = serde_json::from_str(&contents).ok()?;
    let oauth_account = decoded.oauth_account?;
    let profile = ClaudeAccountProfile {
        email: oauth_account
            .email_address
            .and_then(normalize_optional_email),
        organization: oauth_account.organization_name.and_then(normalize_optional),
    };

    (profile.email.is_some() || profile.organization.is_some()).then_some(profile)
}

pub fn replace_credentials_from_home(
    source_home: &Path,
    target_home: &Path,
) -> Result<(), AppError> {
    let target_credentials = target_home.join(CREDENTIALS_FILE_NAME);
    let next = replacement_credentials_json(source_home, target_home)?;
    write_credentials_json(&target_credentials, &next)
}

pub fn replace_credentials_and_profile_from_home(
    source_home: &Path,
    target_home: &Path,
) -> Result<(), AppError> {
    replace_credentials_and_profile_from_home_with_writer(
        source_home,
        target_home,
        write_credentials_json,
    )
}

fn replace_credentials_and_profile_from_home_with_writer(
    source_home: &Path,
    target_home: &Path,
    write_credentials: impl FnOnce(&Path, &[u8]) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let target_config = target_home.join(CONFIG_FILE_NAME);
    let target_credentials = target_home.join(CREDENTIALS_FILE_NAME);
    let next_credentials = replacement_credentials_json(source_home, target_home)?;
    let original_profile = target_file_state(&target_config)?;

    replace_account_profile_from_home(source_home, target_home)?;
    if let Err(error) = write_credentials(&target_credentials, &next_credentials) {
        restore_target_file_state(&target_config, original_profile).map_err(|rollback_error| {
            AppError::ClaudeAuthWrite(format!(
                "credentials update failed: {error}; Claude profile rollback failed: {rollback_error}"
            ))
        })?;
        return Err(error);
    }

    Ok(())
}

fn replacement_credentials_json(
    source_home: &Path,
    target_home: &Path,
) -> Result<Vec<u8>, AppError> {
    let source_credentials = source_home.join(CREDENTIALS_FILE_NAME);
    let source_root = read_credentials_json(&source_credentials)?;
    let claude_oauth = claude_oauth_payload(&source_root)?.clone();

    let target_credentials = target_home.join(CREDENTIALS_FILE_NAME);
    let mut target_root = read_credentials_json_or_empty(&target_credentials)?;
    set_claude_oauth_payload(&mut target_root, claude_oauth)?;

    let next = serde_json::to_string_pretty(&target_root)
        .map_err(|error| AppError::ClaudeAuthDecode(error.to_string()))?;
    Ok(next.into_bytes())
}

pub fn replace_account_profile_from_home(
    source_home: &Path,
    target_home: &Path,
) -> Result<(), AppError> {
    let target_config = target_home.join(CONFIG_FILE_NAME);
    let replacement = replacement_account_profile(source_home, target_home)?;
    apply_account_profile_replacement(&target_config, replacement)
}

enum AccountProfileReplacement {
    Unchanged,
    RemoveFile,
    Write(Vec<u8>),
}

enum TargetFileState {
    Missing,
    Present(Vec<u8>),
}

fn target_file_state(path: &Path) -> Result<TargetFileState, AppError> {
    match fs::read(path) {
        Ok(contents) => Ok(TargetFileState::Present(contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(TargetFileState::Missing),
        Err(error) => Err(AppError::ClaudeAuthRead(error.to_string())),
    }
}

fn restore_target_file_state(path: &Path, state: TargetFileState) -> Result<(), AppError> {
    match state {
        TargetFileState::Missing => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::ClaudeAuthWrite(error.to_string())),
        },
        TargetFileState::Present(contents) => write_claude_config_json(path, &contents),
    }
}

fn replacement_account_profile(
    source_home: &Path,
    target_home: &Path,
) -> Result<AccountProfileReplacement, AppError> {
    let source_account = read_claude_config_oauth_account(&source_home.join(CONFIG_FILE_NAME))?;
    let target_config = target_home.join(CONFIG_FILE_NAME);
    let target_exists = target_config.exists();
    if source_account.is_none() && !target_exists {
        return Ok(AccountProfileReplacement::Unchanged);
    }

    let mut target_root = read_claude_config_json_or_empty(&target_config)?;
    let target_object = claude_config_root_object_mut(&mut target_root)?;
    if let Some(source_account) = source_account {
        target_object.insert("oauthAccount".to_string(), source_account);
    } else {
        target_object.remove("oauthAccount");
        if target_object.is_empty() {
            return Ok(AccountProfileReplacement::RemoveFile);
        }
    }

    let next = serde_json::to_string_pretty(&target_root)
        .map_err(|error| AppError::ClaudeAuthDecode(error.to_string()))?;
    Ok(AccountProfileReplacement::Write(next.into_bytes()))
}

fn apply_account_profile_replacement(
    target_config: &Path,
    replacement: AccountProfileReplacement,
) -> Result<(), AppError> {
    match replacement {
        AccountProfileReplacement::Unchanged => Ok(()),
        AccountProfileReplacement::RemoveFile => match fs::remove_file(target_config) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::ClaudeAuthWrite(error.to_string())),
        },
        AccountProfileReplacement::Write(contents) => {
            write_claude_config_json(target_config, &contents)
        }
    }
}

pub fn save_credentials(credentials: &ClaudeOAuthCredentials) -> Result<(), AppError> {
    let credentials_path = credentials.home_path.join(CREDENTIALS_FILE_NAME);
    let mut root = read_credentials_json_or_empty(&credentials_path)?;

    let payload = ClaudeOAuthPayload {
        access_token: Some(credentials.access_token.clone()),
        refresh_token: credentials.refresh_token.clone(),
        expires_at: credentials
            .expires_at
            .map(|expires_at| expires_at.unix_timestamp().saturating_mul(1_000)),
        scopes: if credentials.scopes.is_empty() {
            None
        } else {
            Some(credentials.scopes.clone())
        },
        rate_limit_tier: credentials.rate_limit_tier.clone(),
        subscription_type: credentials.subscription_type.clone(),
        client_id: credentials.client_id.clone(),
    };

    let payload = serde_json::to_value(payload)
        .map_err(|error| AppError::ClaudeAuthDecode(error.to_string()))?;
    set_claude_oauth_payload(&mut root, payload)?;

    let next = serde_json::to_string_pretty(&root)
        .map_err(|error| AppError::ClaudeAuthDecode(error.to_string()))?;
    write_credentials_json(&credentials_path, next.as_bytes())
}

pub(crate) fn credentials_file_lacks_claude_oauth_payload(
    home_path: &Path,
) -> Result<bool, AppError> {
    let root = read_credentials_json(&home_path.join(CREDENTIALS_FILE_NAME))?;
    let object = credentials_root_object(&root)?;
    Ok(!object.contains_key("claudeAiOauth"))
}

fn parse_credentials_json(
    contents: &str,
    home_path: PathBuf,
) -> Result<ClaudeOAuthCredentials, AppError> {
    let decoded: CredentialsFile = serde_json::from_str(contents)
        .map_err(|error| AppError::ClaudeAuthDecode(error.to_string()))?;
    let payload = decoded
        .claude_ai_oauth
        .ok_or(AppError::ClaudeMissingTokens)?;
    let access_token = required_token(payload.access_token)?;
    let expires_at = payload
        .expires_at
        .and_then(|millis| OffsetDateTime::from_unix_timestamp(millis / 1000).ok());

    Ok(ClaudeOAuthCredentials {
        access_token,
        refresh_token: payload.refresh_token.and_then(normalize_optional),
        expires_at,
        scopes: payload.scopes.unwrap_or_default(),
        rate_limit_tier: payload.rate_limit_tier.and_then(normalize_optional),
        subscription_type: payload.subscription_type.and_then(normalize_optional),
        client_id: payload.client_id.and_then(normalize_optional),
        home_path,
    })
}

fn required_token(value: Option<String>) -> Result<String, AppError> {
    let Some(token) = value else {
        return Err(AppError::ClaudeMissingTokens);
    };
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(AppError::ClaudeMissingTokens);
    }
    Ok(trimmed.to_string())
}

fn normalize_optional(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn normalize_optional_email(value: String) -> Option<String> {
    normalize_optional(value).map(|email| email.to_ascii_lowercase())
}

fn normalize_plan_label(value: String) -> Option<String> {
    let lower = value.trim().to_ascii_lowercase();
    match lower.as_str() {
        "pro" => Some("Claude Pro".to_string()),
        "max" => Some("Claude Max".to_string()),
        "team" => Some("Claude Team".to_string()),
        "enterprise" => Some("Claude Enterprise".to_string()),
        _ => normalize_optional(value),
    }
}

fn read_credentials_json(path: &Path) -> Result<Value, AppError> {
    let contents =
        fs::read_to_string(path).map_err(|error| AppError::ClaudeAuthRead(error.to_string()))?;
    serde_json::from_str(&contents).map_err(|error| AppError::ClaudeAuthDecode(error.to_string()))
}

fn read_credentials_json_or_empty(path: &Path) -> Result<Value, AppError> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|error| AppError::ClaudeAuthDecode(error.to_string())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Value::Object(Default::default())),
        Err(error) => Err(AppError::ClaudeAuthRead(error.to_string())),
    }
}

fn read_claude_config_json_or_empty(path: &Path) -> Result<Value, AppError> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|error| AppError::ClaudeAuthDecode(error.to_string())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Value::Object(Default::default())),
        Err(error) => Err(AppError::ClaudeAuthRead(error.to_string())),
    }
}

fn credentials_root_object(root: &Value) -> Result<&serde_json::Map<String, Value>, AppError> {
    match root {
        Value::Object(object) => Ok(object),
        _ => Err(AppError::ClaudeAuthDecode(
            "credentials root must be a JSON object".to_string(),
        )),
    }
}

fn credentials_root_object_mut(
    root: &mut Value,
) -> Result<&mut serde_json::Map<String, Value>, AppError> {
    match root {
        Value::Object(object) => Ok(object),
        _ => Err(AppError::ClaudeAuthDecode(
            "credentials root must be a JSON object".to_string(),
        )),
    }
}

fn claude_config_root_object(root: &Value) -> Result<&serde_json::Map<String, Value>, AppError> {
    match root {
        Value::Object(object) => Ok(object),
        _ => Err(AppError::ClaudeAuthDecode(
            "Claude config root must be a JSON object".to_string(),
        )),
    }
}

fn claude_config_root_object_mut(
    root: &mut Value,
) -> Result<&mut serde_json::Map<String, Value>, AppError> {
    match root {
        Value::Object(object) => Ok(object),
        _ => Err(AppError::ClaudeAuthDecode(
            "Claude config root must be a JSON object".to_string(),
        )),
    }
}

fn claude_oauth_payload(root: &Value) -> Result<&Value, AppError> {
    credentials_root_object(root)?
        .get("claudeAiOauth")
        .ok_or(AppError::ClaudeMissingTokens)
}

fn read_claude_config_oauth_account(path: &Path) -> Result<Option<Value>, AppError> {
    let root = read_claude_config_json_or_empty(path)?;
    Ok(claude_config_root_object(&root)?
        .get("oauthAccount")
        .cloned())
}

fn set_claude_oauth_payload(root: &mut Value, payload: Value) -> Result<(), AppError> {
    credentials_root_object_mut(root)?.insert("claudeAiOauth".to_string(), payload);
    Ok(())
}

fn write_credentials_json(path: &Path, contents: &[u8]) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(|| {
        AppError::ClaudeAuthWrite(format!(
            "credentials path has no parent: {}",
            path.to_string_lossy()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| AppError::ClaudeAuthWrite(error.to_string()))?;

    let tmp = temporary_file_path(parent, CREDENTIALS_FILE_NAME);
    write_new_file(&tmp, contents).map_err(|error| AppError::ClaudeAuthWrite(error.to_string()))?;
    if let Err(error) = apply_secure_file_permissions(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    replace_file(&tmp, path).map_err(|error| AppError::ClaudeAuthWrite(error.to_string()))
}

fn write_claude_config_json(path: &Path, contents: &[u8]) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(|| {
        AppError::ClaudeAuthWrite(format!(
            "Claude config path has no parent: {}",
            path.to_string_lossy()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| AppError::ClaudeAuthWrite(error.to_string()))?;

    let tmp = temporary_file_path(parent, CONFIG_FILE_NAME);
    write_new_file(&tmp, contents).map_err(|error| AppError::ClaudeAuthWrite(error.to_string()))?;
    if let Err(error) = apply_secure_file_permissions(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    replace_file(&tmp, path).map_err(|error| AppError::ClaudeAuthWrite(error.to_string()))
}

fn token_fingerprint(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    format!("claude-token-{}", lower_hex(&digest))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
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
    fs::set_permissions(path, permissions)
        .map_err(|error| AppError::ClaudeAuthWrite(error.to_string()))
}

#[cfg(not(unix))]
fn apply_secure_file_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn temp_home(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("wovo-claude-auth-{name}-{}", Uuid::new_v4()))
    }

    #[test]
    fn parses_claude_oauth_credentials() {
        let credentials = parse_credentials_json(
            r#"{
                "claudeAiOauth": {
                    "accessToken": "access",
                    "refreshToken": "refresh",
                    "expiresAt": 1770000000000,
                    "scopes": ["user:profile", "user:inference"],
                    "rateLimitTier": "max",
                    "subscriptionType": "Claude Max",
                    "clientId": "client-123"
                }
            }"#,
            PathBuf::from("/tmp/claude"),
        )
        .unwrap();

        assert_eq!(credentials.access_token, "access");
        assert_eq!(credentials.refresh_token.as_deref(), Some("refresh"));
        assert!(credentials.has_profile_scope());
        assert_eq!(credentials.plan_type().as_deref(), Some("Claude Max"));
        assert!(credentials.expires_at.is_some());
        assert_eq!(credentials.client_id.as_deref(), Some("client-123"));
    }

    #[test]
    fn provider_account_id_uses_stable_token_digest() {
        let credentials = parse_credentials_json(
            r#"{
                "claudeAiOauth": {
                    "accessToken": "access",
                    "refreshToken": "refresh"
                }
            }"#,
            PathBuf::from("/tmp/claude"),
        )
        .unwrap();

        assert_eq!(
            credentials.provider_account_id().as_deref(),
            Some("claude-token-d6cc0a088c07683c65cd266860cab8d94b3a1937b17420d9da30ca299c09fb77")
        );
    }

    #[test]
    fn rejects_missing_oauth_payload() {
        let error = parse_credentials_json(r#"{}"#, PathBuf::from("/tmp/claude")).unwrap_err();

        assert!(matches!(error, AppError::ClaudeMissingTokens));
    }

    #[test]
    fn loads_account_profile_from_claude_config() {
        let home = temp_home("profile");
        fs::create_dir_all(&home).unwrap();
        fs::write(
            home.join(CONFIG_FILE_NAME),
            r#"{
                "oauthAccount": {
                    "emailAddress": "USER@Example.com",
                    "organizationName": "Example Org"
                }
            }"#,
        )
        .unwrap();

        let profile = load_account_profile_from_home(&home).unwrap();

        assert_eq!(profile.email.as_deref(), Some("user@example.com"));
        assert_eq!(profile.organization.as_deref(), Some("Example Org"));

        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn saves_refreshed_oauth_credentials_without_dropping_metadata() {
        let home = temp_home("save-refreshed");
        fs::create_dir_all(&home).unwrap();
        fs::write(
            home.join(CREDENTIALS_FILE_NAME),
            r#"{
                "other": true,
                "claudeAiOauth": {
                    "accessToken": "old-access",
                    "refreshToken": "old-refresh",
                    "subscriptionType": "Claude Max"
                }
            }"#,
        )
        .unwrap();

        save_credentials(&ClaudeOAuthCredentials {
            access_token: "new-access".to_string(),
            refresh_token: Some("new-refresh".to_string()),
            expires_at: OffsetDateTime::from_unix_timestamp(1_770_000_000).ok(),
            scopes: vec!["user:profile".to_string()],
            rate_limit_tier: Some("max".to_string()),
            subscription_type: Some("Claude Max".to_string()),
            client_id: Some("client-123".to_string()),
            home_path: home.clone(),
        })
        .unwrap();

        let saved: Value =
            serde_json::from_str(&fs::read_to_string(home.join(CREDENTIALS_FILE_NAME)).unwrap())
                .unwrap();

        assert_eq!(saved["other"], Value::Bool(true));
        assert_eq!(saved["claudeAiOauth"]["accessToken"], "new-access");
        assert_eq!(saved["claudeAiOauth"]["refreshToken"], "new-refresh");
        assert_eq!(saved["claudeAiOauth"]["expiresAt"], 1_770_000_000_000_i64);
        assert_eq!(saved["claudeAiOauth"]["clientId"], "client-123");

        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn replace_credentials_from_home_preserves_target_metadata() {
        let source_home = temp_home("replace-source");
        let target_home = temp_home("replace-target");
        fs::create_dir_all(&source_home).unwrap();
        fs::create_dir_all(&target_home).unwrap();
        fs::write(
            source_home.join(CREDENTIALS_FILE_NAME),
            r#"{
                "sourceOnly": true,
                "claudeAiOauth": {
                    "accessToken": "new-access",
                    "refreshToken": "new-refresh",
                    "customOauthField": "source-oauth"
                }
            }"#,
        )
        .unwrap();
        fs::write(
            target_home.join(CREDENTIALS_FILE_NAME),
            r#"{
                "targetMetadata": {"theme": "dark"},
                "other": true,
                "claudeAiOauth": {
                    "accessToken": "old-access",
                    "refreshToken": "old-refresh"
                }
            }"#,
        )
        .unwrap();

        replace_credentials_from_home(&source_home, &target_home).unwrap();

        let saved: Value = serde_json::from_str(
            &fs::read_to_string(target_home.join(CREDENTIALS_FILE_NAME)).unwrap(),
        )
        .unwrap();

        assert_eq!(saved["targetMetadata"]["theme"], "dark");
        assert_eq!(saved["other"], Value::Bool(true));
        assert_eq!(saved["sourceOnly"], Value::Null);
        assert_eq!(saved["claudeAiOauth"]["accessToken"], "new-access");
        assert_eq!(saved["claudeAiOauth"]["refreshToken"], "new-refresh");
        assert_eq!(saved["claudeAiOauth"]["customOauthField"], "source-oauth");

        let _ = fs::remove_dir_all(source_home);
        let _ = fs::remove_dir_all(target_home);
    }

    #[test]
    fn replace_credentials_and_profile_preserves_credentials_when_profile_copy_fails() {
        let source_home = temp_home("replace-with-profile-source");
        let target_home = temp_home("replace-with-profile-target");
        fs::create_dir_all(&source_home).unwrap();
        fs::create_dir_all(&target_home).unwrap();
        fs::write(
            source_home.join(CREDENTIALS_FILE_NAME),
            r#"{
                "claudeAiOauth": {
                    "accessToken": "new-access",
                    "refreshToken": "new-refresh"
                }
            }"#,
        )
        .unwrap();
        fs::write(
            source_home.join(CONFIG_FILE_NAME),
            r#"{
                "oauthAccount": {
                    "emailAddress": "new@example.com",
                    "organizationName": "New Org"
                }
            }"#,
        )
        .unwrap();
        fs::write(
            target_home.join(CREDENTIALS_FILE_NAME),
            r#"{
                "claudeAiOauth": {
                    "accessToken": "old-access",
                    "refreshToken": "old-refresh"
                }
            }"#,
        )
        .unwrap();
        fs::write(target_home.join(CONFIG_FILE_NAME), "{not-json").unwrap();

        let error =
            replace_credentials_and_profile_from_home(&source_home, &target_home).unwrap_err();

        assert!(matches!(error, AppError::ClaudeAuthDecode(_)));
        let saved: Value = serde_json::from_str(
            &fs::read_to_string(target_home.join(CREDENTIALS_FILE_NAME)).unwrap(),
        )
        .unwrap();
        assert_eq!(saved["claudeAiOauth"]["accessToken"], "old-access");
        assert_eq!(saved["claudeAiOauth"]["refreshToken"], "old-refresh");

        let _ = fs::remove_dir_all(source_home);
        let _ = fs::remove_dir_all(target_home);
    }

    #[test]
    fn replace_credentials_and_profile_restores_profile_when_credentials_write_fails() {
        let source_home = temp_home("replace-rollback-source");
        let target_home = temp_home("replace-rollback-target");
        fs::create_dir_all(&source_home).unwrap();
        fs::create_dir_all(&target_home).unwrap();
        fs::write(
            source_home.join(CREDENTIALS_FILE_NAME),
            r#"{
                "claudeAiOauth": {
                    "accessToken": "new-access",
                    "refreshToken": "new-refresh"
                }
            }"#,
        )
        .unwrap();
        fs::write(
            source_home.join(CONFIG_FILE_NAME),
            r#"{
                "oauthAccount": {
                    "emailAddress": "new@example.com",
                    "organizationName": "New Org"
                }
            }"#,
        )
        .unwrap();
        fs::write(
            target_home.join(CREDENTIALS_FILE_NAME),
            r#"{
                "claudeAiOauth": {
                    "accessToken": "old-access",
                    "refreshToken": "old-refresh"
                }
            }"#,
        )
        .unwrap();
        fs::write(
            target_home.join(CONFIG_FILE_NAME),
            r#"{
                "oauthAccount": {
                    "emailAddress": "old@example.com",
                    "organizationName": "Old Org"
                },
                "theme": "dark"
            }"#,
        )
        .unwrap();

        let error = replace_credentials_and_profile_from_home_with_writer(
            &source_home,
            &target_home,
            |_, _| {
                let profile = load_account_profile_from_home(&target_home).unwrap();
                assert_eq!(profile.email.as_deref(), Some("new@example.com"));
                Err(AppError::ClaudeAuthWrite(
                    "forced credentials write failure".to_string(),
                ))
            },
        )
        .unwrap_err();

        assert!(matches!(error, AppError::ClaudeAuthWrite(_)));
        let saved_profile: Value =
            serde_json::from_str(&fs::read_to_string(target_home.join(CONFIG_FILE_NAME)).unwrap())
                .unwrap();
        assert_eq!(
            saved_profile["oauthAccount"]["emailAddress"],
            "old@example.com"
        );
        assert_eq!(saved_profile["oauthAccount"]["organizationName"], "Old Org");
        assert_eq!(saved_profile["theme"], "dark");

        let saved_credentials: Value = serde_json::from_str(
            &fs::read_to_string(target_home.join(CREDENTIALS_FILE_NAME)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            saved_credentials["claudeAiOauth"]["accessToken"],
            "old-access"
        );
        assert_eq!(
            saved_credentials["claudeAiOauth"]["refreshToken"],
            "old-refresh"
        );

        let _ = fs::remove_dir_all(source_home);
        let _ = fs::remove_dir_all(target_home);
    }
}
