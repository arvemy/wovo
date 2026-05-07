use crate::domain::account::AccountSummary;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use uuid::Uuid;

const STORE_VERSION: u16 = 1;
const STORE_FILE_NAME: &str = "managed-codex-accounts.json";
const HOMES_DIR_NAME: &str = "managed-codex-homes";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedCodexAccountRecord {
    pub id: Uuid,
    pub email: Option<String>,
    pub provider_account_id: Option<String>,
    pub home_path: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_authenticated_at: Option<i64>,
}

impl ManagedCodexAccountRecord {
    pub fn summary(&self) -> AccountSummary {
        AccountSummary::managed(
            self.id.to_string(),
            self.email.clone(),
            self.provider_account_id.clone(),
            self.home_path.clone(),
            self.created_at,
            self.updated_at,
            self.last_authenticated_at,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedCodexAccountSet {
    version: u16,
    accounts: Vec<ManagedCodexAccountRecord>,
}

#[derive(Debug, Clone)]
pub struct ManagedCodexAccountStore {
    root: PathBuf,
}

impl ManagedCodexAccountStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn managed_homes_dir(&self) -> PathBuf {
        self.root.join(HOMES_DIR_NAME)
    }

    pub fn make_home_path(&self, id: Uuid) -> PathBuf {
        self.managed_homes_dir().join(id.to_string())
    }

    pub fn create_home(&self, id: Uuid) -> Result<PathBuf, AppError> {
        let home = self.make_home_path(id);
        fs::create_dir_all(&home).map_err(|error| AppError::AccountStore(error.to_string()))?;
        Ok(home)
    }

    pub fn load_accounts(&self) -> Result<Vec<ManagedCodexAccountRecord>, AppError> {
        let path = self.store_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let contents =
            fs::read_to_string(path).map_err(|error| AppError::AccountStore(error.to_string()))?;
        let decoded: ManagedCodexAccountSet = serde_json::from_str(&contents)
            .map_err(|error| AppError::AccountStore(error.to_string()))?;

        if decoded.version != STORE_VERSION {
            return Err(AppError::AccountStore(format!(
                "unsupported account store version {}",
                decoded.version
            )));
        }

        Ok(sanitized_accounts(decoded.accounts))
    }

    pub fn load_summaries(&self) -> Result<Vec<AccountSummary>, AppError> {
        Ok(self
            .load_accounts()?
            .into_iter()
            .map(|account| account.summary())
            .collect())
    }

    pub fn find_account(&self, account_id: &str) -> Result<ManagedCodexAccountRecord, AppError> {
        let id = Uuid::parse_str(account_id)
            .map_err(|_| AppError::UnknownAccount(account_id.to_string()))?;
        self.load_accounts()?
            .into_iter()
            .find(|account| account.id == id)
            .ok_or_else(|| AppError::UnknownAccount(account_id.to_string()))
    }

    pub fn upsert_authenticated_account(
        &self,
        preferred_id: Uuid,
        email: Option<String>,
        provider_account_id: Option<String>,
        home_path: PathBuf,
    ) -> Result<(ManagedCodexAccountRecord, Vec<PathBuf>), AppError> {
        let normalized_email = normalize_optional_email(email);
        let normalized_provider_account_id = normalize_optional(provider_account_id);
        if normalized_email.is_none() && normalized_provider_account_id.is_none() {
            return Err(AppError::CodexLoginFailed(
                "login did not produce an account identity".to_string(),
            ));
        }

        let now = OffsetDateTime::now_utc().unix_timestamp();
        let mut accounts = self.load_accounts()?;
        let matched_index = if let Some(index) = accounts
            .iter()
            .position(|account| account.id == preferred_id)
        {
            if !authenticated_identity_matches(
                &accounts[index],
                normalized_email.as_deref(),
                normalized_provider_account_id.as_deref(),
            ) {
                return Err(AppError::AccountIdentityMismatch);
            }
            Some(index)
        } else if let Some(provider_account_id) = normalized_provider_account_id.as_deref() {
            accounts.iter().position(|account| {
                account.provider_account_id.as_deref() == Some(provider_account_id)
            })
        } else if let Some(email) = normalized_email.as_deref() {
            accounts
                .iter()
                .position(|account| account.email.as_deref() == Some(email))
        } else {
            None
        };

        let existing = matched_index.map(|index| accounts.remove(index));
        let id = existing
            .as_ref()
            .map(|account| account.id)
            .unwrap_or(preferred_id);
        let record = ManagedCodexAccountRecord {
            id,
            email: normalized_email,
            provider_account_id: normalized_provider_account_id,
            home_path: home_path.to_string_lossy().to_string(),
            created_at: existing
                .as_ref()
                .map(|account| account.created_at)
                .unwrap_or(now),
            updated_at: now,
            last_authenticated_at: Some(now),
        };

        let mut replaced_home_paths = Vec::new();
        if let Some(existing) = existing {
            if existing.home_path != record.home_path {
                replaced_home_paths.push(PathBuf::from(existing.home_path));
            }
        }

        accounts.push(record.clone());
        accounts.sort_by(|left, right| {
            left.email
                .cmp(&right.email)
                .then_with(|| left.provider_account_id.cmp(&right.provider_account_id))
                .then_with(|| left.id.cmp(&right.id))
        });
        self.store_accounts(accounts)?;

        Ok((record, replaced_home_paths))
    }

    pub fn remove_account(&self, account_id: &str) -> Result<(), AppError> {
        let id = Uuid::parse_str(account_id)
            .map_err(|_| AppError::UnknownAccount(account_id.to_string()))?;
        let mut accounts = self.load_accounts()?;
        let Some(index) = accounts.iter().position(|account| account.id == id) else {
            return Err(AppError::UnknownAccount(account_id.to_string()));
        };
        let account = accounts.remove(index);
        self.store_accounts(accounts)?;
        self.remove_home_if_safe(Path::new(&account.home_path))?;
        Ok(())
    }

    pub fn remove_home_if_safe(&self, home: &Path) -> Result<(), AppError> {
        self.validate_managed_home(home)?;
        if home.exists() {
            fs::remove_dir_all(home).map_err(|error| AppError::AccountStore(error.to_string()))?;
        }
        Ok(())
    }

    fn store_accounts(&self, accounts: Vec<ManagedCodexAccountRecord>) -> Result<(), AppError> {
        fs::create_dir_all(&self.root)
            .map_err(|error| AppError::AccountStore(error.to_string()))?;
        let payload = ManagedCodexAccountSet {
            version: STORE_VERSION,
            accounts: sanitized_accounts(accounts),
        };
        let contents = serde_json::to_string_pretty(&payload)
            .map_err(|error| AppError::AccountStore(error.to_string()))?;
        fs::write(self.store_path(), contents)
            .map_err(|error| AppError::AccountStore(error.to_string()))?;
        apply_secure_file_permissions(&self.store_path())?;
        Ok(())
    }

    fn store_path(&self) -> PathBuf {
        self.root.join(STORE_FILE_NAME)
    }

    fn validate_managed_home(&self, home: &Path) -> Result<(), AppError> {
        let root = self.managed_homes_dir();
        let root = if root.exists() {
            root.canonicalize()
                .map_err(|error| AppError::AccountStore(error.to_string()))?
        } else {
            root
        };
        let target = if home.exists() {
            home.canonicalize()
                .map_err(|error| AppError::AccountStore(error.to_string()))?
        } else {
            home.to_path_buf()
        };

        if target == root || !target.starts_with(&root) {
            return Err(AppError::UnsafeManagedHome(
                home.to_string_lossy().to_string(),
            ));
        }
        Ok(())
    }
}

fn sanitized_accounts(accounts: Vec<ManagedCodexAccountRecord>) -> Vec<ManagedCodexAccountRecord> {
    let mut sanitized = Vec::new();
    for account in accounts {
        let duplicate = sanitized
            .iter()
            .any(|existing: &ManagedCodexAccountRecord| {
                existing.id == account.id
                    || (account.provider_account_id.is_some()
                        && existing.provider_account_id == account.provider_account_id)
                    || (account.provider_account_id.is_none()
                        && existing.provider_account_id.is_none()
                        && existing.email == account.email)
            });
        if !duplicate {
            sanitized.push(account);
        }
    }
    sanitized
}

fn authenticated_identity_matches(
    existing: &ManagedCodexAccountRecord,
    email: Option<&str>,
    provider_account_id: Option<&str>,
) -> bool {
    if let Some(existing_provider_account_id) = existing.provider_account_id.as_deref() {
        return provider_account_id == Some(existing_provider_account_id);
    }

    if let Some(existing_email) = existing.email.as_deref() {
        return email
            .map(|email| email.eq_ignore_ascii_case(existing_email))
            .unwrap_or(false);
    }

    true
}

fn normalize_optional_email(value: Option<String>) -> Option<String> {
    normalize_optional(value).map(|value| value.to_ascii_lowercase())
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    let trimmed = value?.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(unix)]
fn apply_secure_file_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions)
        .map_err(|error| AppError::AccountStore(error.to_string()))
}

#[cfg(not(unix))]
fn apply_secure_file_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("wovo-{name}-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn stores_and_loads_managed_account() {
        let root = temp_root("store-load");
        let store = ManagedCodexAccountStore::new(root.clone());
        let id = Uuid::new_v4();
        let home = store.create_home(id).unwrap();

        let (record, _) = store
            .upsert_authenticated_account(
                id,
                Some("USER@Example.COM".to_string()),
                Some("account-1".to_string()),
                home,
            )
            .unwrap();

        let loaded = store.load_accounts().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, record.id);
        assert_eq!(loaded[0].email.as_deref(), Some("user@example.com"));
        assert_eq!(loaded[0].provider_account_id.as_deref(), Some("account-1"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_to_remove_home_outside_managed_root() {
        let root = temp_root("unsafe-remove");
        let store = ManagedCodexAccountStore::new(root.clone());
        let outside = std::env::temp_dir().join(format!("wovo-outside-{}", Uuid::new_v4()));
        fs::create_dir_all(&outside).unwrap();

        let error = store.remove_home_if_safe(&outside).unwrap_err();
        assert!(matches!(error, AppError::UnsafeManagedHome(_)));

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn explicit_reauth_id_replaces_existing_home_after_success() {
        let root = temp_root("reauth-replace");
        let store = ManagedCodexAccountStore::new(root.clone());
        let id = Uuid::new_v4();
        let first_home = store.create_home(id).unwrap();
        store
            .upsert_authenticated_account(
                id,
                Some("first@example.com".to_string()),
                Some("account-1".to_string()),
                first_home.clone(),
            )
            .unwrap();

        let second_home = store.create_home(Uuid::new_v4()).unwrap();
        let (record, replaced) = store
            .upsert_authenticated_account(
                id,
                Some("second@example.com".to_string()),
                Some("account-1".to_string()),
                second_home.clone(),
            )
            .unwrap();

        assert_eq!(record.id, id);
        assert_eq!(record.email.as_deref(), Some("second@example.com"));
        assert_eq!(replaced, vec![first_home]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn explicit_reauth_id_rejects_mismatched_identity() {
        let root = temp_root("reauth-mismatch");
        let store = ManagedCodexAccountStore::new(root.clone());
        let id = Uuid::new_v4();
        let first_home = store.create_home(id).unwrap();
        store
            .upsert_authenticated_account(
                id,
                Some("first@example.com".to_string()),
                Some("account-1".to_string()),
                first_home,
            )
            .unwrap();

        let second_home = store.create_home(Uuid::new_v4()).unwrap();
        let error = store
            .upsert_authenticated_account(
                id,
                Some("second@example.com".to_string()),
                Some("account-2".to_string()),
                second_home,
            )
            .unwrap_err();

        assert!(matches!(error, AppError::AccountIdentityMismatch));

        let _ = fs::remove_dir_all(root);
    }
}
