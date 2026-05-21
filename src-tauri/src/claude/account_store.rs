use crate::claude::auth_store::load_credentials_from_home;
use crate::codex::atomic_file::{replace_file, temporary_file_path, write_new_file};
use crate::domain::account::AccountSummary;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::Error as IoError;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use time::OffsetDateTime;
use uuid::Uuid;

const STORE_VERSION: u16 = 1;
const STORE_FILE_NAME: &str = "managed-claude-accounts.json";
const HOMES_DIR_NAME: &str = "accounts";
static ACCOUNT_STORE_MUTATION_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedClaudeAccountRecord {
    pub id: Uuid,
    pub email: Option<String>,
    pub provider_account_id: Option<String>,
    #[serde(default)]
    pub workspace_account_id: Option<String>,
    #[serde(default)]
    pub workspace_label: Option<String>,
    pub home_path: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_authenticated_at: Option<i64>,
}

impl ManagedClaudeAccountRecord {
    pub fn summary(&self) -> AccountSummary {
        self.summary_with_status(false)
    }

    pub fn summary_with_status(&self, is_live_system: bool) -> AccountSummary {
        AccountSummary::managed(
            self.id.to_string(),
            self.email.clone(),
            self.provider_account_id.clone(),
            self.workspace_account_id.clone(),
            self.workspace_label.clone(),
            self.home_path.clone(),
            self.created_at,
            self.updated_at,
            self.last_authenticated_at,
            is_live_system,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedClaudeAccountSet {
    version: u16,
    accounts: Vec<ManagedClaudeAccountRecord>,
}

#[derive(Debug, Clone)]
pub struct ManagedClaudeAccountStore {
    root: PathBuf,
}

impl ManagedClaudeAccountStore {
    #[cfg(test)]
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
        self.prepare_home(&home)?;
        Ok(home)
    }

    pub fn load_accounts(&self) -> Result<Vec<ManagedClaudeAccountRecord>, AppError> {
        let _guard = account_store_mutation_lock()?;
        self.load_accounts_unlocked()
    }

    fn load_accounts_unlocked(&self) -> Result<Vec<ManagedClaudeAccountRecord>, AppError> {
        let path = self.store_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let contents = fs::read_to_string(path)
            .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
        let decoded: ManagedClaudeAccountSet = serde_json::from_str(&contents)
            .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
        if decoded.version != STORE_VERSION {
            return Err(AppError::ClaudeAccountStore(format!(
                "unsupported account store version {}",
                decoded.version
            )));
        }
        Ok(sanitized_accounts(decoded.accounts))
    }

    pub fn find_matching_account(
        &self,
        email: Option<&str>,
        provider_account_id: Option<&str>,
        workspace_account_id: Option<&str>,
    ) -> Result<Option<ManagedClaudeAccountRecord>, AppError> {
        let normalized_email = normalize_optional_email(email.map(str::to_string));
        let normalized_provider_account_id =
            normalize_optional(provider_account_id.map(str::to_string));
        let normalized_workspace_account_id =
            normalize_optional(workspace_account_id.map(str::to_string));
        let accounts = self.load_accounts()?;
        Ok(find_matching_account_index(
            &accounts,
            normalized_email.as_deref(),
            normalized_provider_account_id.as_deref(),
            normalized_workspace_account_id.as_deref(),
        )
        .map(|index| accounts[index].clone()))
    }

    pub fn find_account(&self, account_id: &str) -> Result<ManagedClaudeAccountRecord, AppError> {
        let id = Uuid::parse_str(account_id)
            .map_err(|_| AppError::ClaudeUnknownAccount(account_id.to_string()))?;
        self.load_accounts()?
            .into_iter()
            .find(|account| account.id == id)
            .ok_or_else(|| AppError::ClaudeUnknownAccount(account_id.to_string()))
    }

    pub fn update_account_provider_id(
        &self,
        account_id: Uuid,
        provider_account_id: Option<String>,
    ) -> Result<(), AppError> {
        let _guard = account_store_mutation_lock()?;
        let mut accounts = self.load_accounts_unlocked()?;
        let Some(account) = accounts.iter_mut().find(|account| account.id == account_id) else {
            return Err(AppError::ClaudeUnknownAccount(account_id.to_string()));
        };

        account.provider_account_id = normalize_optional(provider_account_id);
        account.updated_at = OffsetDateTime::now_utc().unix_timestamp();
        self.store_accounts_unlocked(accounts)
    }

    pub fn upsert_authenticated_account(
        &self,
        preferred_id: Uuid,
        email: Option<String>,
        provider_account_id: Option<String>,
        workspace_account_id: Option<String>,
        workspace_label: Option<String>,
        home_path: PathBuf,
    ) -> Result<(ManagedClaudeAccountRecord, Vec<PathBuf>), AppError> {
        let _guard = account_store_mutation_lock()?;
        self.upsert_authenticated_account_unlocked(
            preferred_id,
            email,
            provider_account_id,
            workspace_account_id,
            workspace_label,
            home_path,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "transactional upsert keeps identity fields explicit before rollback handling"
    )]
    pub fn upsert_authenticated_account_and_then<F>(
        &self,
        preferred_id: Uuid,
        email: Option<String>,
        provider_account_id: Option<String>,
        workspace_account_id: Option<String>,
        workspace_label: Option<String>,
        home_path: PathBuf,
        after_upsert: F,
    ) -> Result<(ManagedClaudeAccountRecord, Vec<PathBuf>), AppError>
    where
        F: FnOnce(&ManagedClaudeAccountRecord) -> Result<(), AppError>,
    {
        let _guard = account_store_mutation_lock()?;
        let previous_accounts = self.load_accounts_unlocked()?;
        let (record, replaced_home_paths) = self.upsert_authenticated_account_unlocked(
            preferred_id,
            email,
            provider_account_id,
            workspace_account_id,
            workspace_label,
            home_path,
        )?;

        if let Err(error) = after_upsert(&record) {
            let message = error.to_string();
            self.store_accounts_unlocked(previous_accounts)
                .map_err(|rollback_error| {
                    AppError::ClaudeAccountStore(format!(
                        "account update failed ({message}); rollback failed ({rollback_error})"
                    ))
                })?;
            return Err(error);
        }

        Ok((record, replaced_home_paths))
    }

    fn upsert_authenticated_account_unlocked(
        &self,
        preferred_id: Uuid,
        email: Option<String>,
        provider_account_id: Option<String>,
        workspace_account_id: Option<String>,
        workspace_label: Option<String>,
        home_path: PathBuf,
    ) -> Result<(ManagedClaudeAccountRecord, Vec<PathBuf>), AppError> {
        let normalized_email = normalize_optional_email(email);
        let normalized_provider_account_id = normalize_optional(provider_account_id);
        let normalized_workspace_account_id = normalize_optional(workspace_account_id);
        let normalized_workspace_label = normalize_optional(workspace_label);
        let normalized_identity_id = normalized_workspace_account_id
            .as_deref()
            .or(normalized_provider_account_id.as_deref());
        if normalized_email.is_none() && normalized_identity_id.is_none() {
            return Err(AppError::ClaudeLoginFailed(
                "login did not produce an account identity".to_string(),
            ));
        }

        let now = OffsetDateTime::now_utc().unix_timestamp();
        let mut accounts = self.load_accounts_unlocked()?;
        let matched_index = if let Some(index) = accounts
            .iter()
            .position(|account| account.id == preferred_id)
        {
            if !authenticated_identity_matches(
                &accounts[index],
                normalized_email.as_deref(),
                normalized_provider_account_id.as_deref(),
                normalized_workspace_account_id.as_deref(),
            ) {
                return Err(AppError::ClaudeAccountIdentityMismatch);
            }
            Some(index)
        } else {
            find_matching_account_index(
                &accounts,
                normalized_email.as_deref(),
                normalized_provider_account_id.as_deref(),
                normalized_workspace_account_id.as_deref(),
            )
        };

        let existing = matched_index.map(|index| accounts.remove(index));
        let id = existing
            .as_ref()
            .map(|account| account.id)
            .unwrap_or(preferred_id);
        let email = normalized_email
            .or_else(|| existing.as_ref().and_then(|account| account.email.clone()));
        let provider_account_id = normalized_provider_account_id.or_else(|| {
            existing
                .as_ref()
                .and_then(|account| account.provider_account_id.clone())
        });
        let preserve_existing_workspace =
            normalized_workspace_account_id.is_none() && normalized_workspace_label.is_none();
        let record = ManagedClaudeAccountRecord {
            id,
            email,
            provider_account_id,
            workspace_account_id: if preserve_existing_workspace {
                existing
                    .as_ref()
                    .and_then(|account| account.workspace_account_id.clone())
            } else {
                normalized_workspace_account_id
            },
            workspace_label: if preserve_existing_workspace {
                existing
                    .as_ref()
                    .and_then(|account| account.workspace_label.clone())
            } else {
                normalized_workspace_label
            },
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
                .then_with(|| left.workspace_account_id.cmp(&right.workspace_account_id))
                .then_with(|| left.provider_account_id.cmp(&right.provider_account_id))
                .then_with(|| left.id.cmp(&right.id))
        });
        self.store_accounts_unlocked(accounts)?;
        Ok((record, replaced_home_paths))
    }

    pub fn remove_account(&self, account_id: &str) -> Result<(), AppError> {
        let id = Uuid::parse_str(account_id)
            .map_err(|_| AppError::ClaudeUnknownAccount(account_id.to_string()))?;

        let _guard = account_store_mutation_lock()?;
        let mut accounts = self.load_accounts_unlocked()?;
        let Some(index) = accounts.iter().position(|account| account.id == id) else {
            return Err(AppError::ClaudeUnknownAccount(account_id.to_string()));
        };
        let home = PathBuf::from(&accounts[index].home_path);
        self.validate_managed_home(&home)?;

        let staged_home = if home.exists() {
            let staged_home = self.removing_home_path(id);
            move_managed_home(&home, &staged_home)?;
            Some(staged_home)
        } else {
            None
        };

        let previous_accounts = accounts.clone();
        accounts.remove(index);
        if let Err(error) = self.store_accounts_unlocked(accounts) {
            if let Some(staged_home) = staged_home.as_ref() {
                if let Err(restore_error) = move_managed_home(staged_home, &home) {
                    return Err(AppError::ClaudeAccountStore(format!(
                        "account removal failed ({error}); home restore failed ({restore_error})"
                    )));
                }
            }
            return Err(error);
        }

        if let Some(staged_home) = staged_home {
            if staged_home.exists() {
                if let Err(error) = fs::remove_dir_all(&staged_home) {
                    return Err(self.rollback_removed_account_after_cleanup_failure(
                        previous_accounts,
                        id,
                        &staged_home,
                        &home,
                        error,
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn remove_home_if_safe(&self, home: &Path) -> Result<(), AppError> {
        self.validate_managed_home(home)?;
        if home.exists() {
            fs::remove_dir_all(home)
                .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
        }
        Ok(())
    }

    fn prepare_home(&self, home: &Path) -> Result<(), AppError> {
        fs::create_dir_all(home).map_err(|error| AppError::ClaudeAccountStore(error.to_string()))
    }

    fn store_accounts_unlocked(
        &self,
        accounts: Vec<ManagedClaudeAccountRecord>,
    ) -> Result<(), AppError> {
        fs::create_dir_all(&self.root)
            .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
        let payload = ManagedClaudeAccountSet {
            version: STORE_VERSION,
            accounts: sanitized_accounts(accounts),
        };
        let contents = serde_json::to_vec_pretty(&payload)
            .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
        let store_path = self.store_path();
        let parent = store_path.parent().ok_or_else(|| {
            AppError::ClaudeAccountStore(format!(
                "account store path has no parent: {}",
                store_path.to_string_lossy()
            ))
        })?;
        let tmp = temporary_file_path(parent, STORE_FILE_NAME);
        write_new_file(&tmp, &contents)
            .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
        if let Err(error) = apply_secure_file_permissions(&tmp) {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        replace_file(&tmp, &store_path)
            .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))
    }

    fn store_path(&self) -> PathBuf {
        self.root.join(STORE_FILE_NAME)
    }

    fn removing_home_path(&self, id: Uuid) -> PathBuf {
        self.managed_homes_dir()
            .join(format!(".{id}.removing.{}", Uuid::new_v4()))
    }

    fn rollback_removed_account_after_cleanup_failure(
        &self,
        mut previous_accounts: Vec<ManagedClaudeAccountRecord>,
        account_id: Uuid,
        staged_home: &Path,
        home: &Path,
        cleanup_error: IoError,
    ) -> AppError {
        let cleanup_message = cleanup_error.to_string();
        let mut rollback_failures = Vec::new();
        if let Err(error) = move_managed_home(staged_home, home) {
            rollback_failures.push(format!("home restore failed ({error})"));
            if let Some(account) = previous_accounts
                .iter_mut()
                .find(|account| account.id == account_id)
            {
                account.home_path = staged_home.to_string_lossy().to_string();
            }
        }
        if let Err(error) = self.store_accounts_unlocked(previous_accounts) {
            rollback_failures.push(format!("record restore failed ({error})"));
        }

        if rollback_failures.is_empty() {
            AppError::ClaudeAccountStore(format!(
                "managed Claude home cleanup failed; account record was restored ({cleanup_message})"
            ))
        } else {
            AppError::ClaudeAccountStore(format!(
                "managed Claude home cleanup failed ({cleanup_message}); {}",
                rollback_failures.join("; ")
            ))
        }
    }

    fn validate_managed_home(&self, home: &Path) -> Result<(), AppError> {
        let root = self.managed_homes_dir();
        let root = if root.exists() {
            root.canonicalize()
                .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?
        } else {
            root
        };
        let target = if home.exists() {
            home.canonicalize()
                .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?
        } else {
            home.to_path_buf()
        };

        if target == root || !target.starts_with(&root) {
            return Err(AppError::ClaudeUnsafeManagedHome(
                home.to_string_lossy().to_string(),
            ));
        }
        Ok(())
    }
}

fn move_managed_home(source: &Path, target: &Path) -> Result<(), AppError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
    }
    fs::rename(source, target).map_err(|error| AppError::ClaudeAccountStore(error.to_string()))
}

pub fn default_wovo_claude_root() -> PathBuf {
    dirs_home().join(".wovo").join("claude")
}

pub fn managed_account_store() -> ManagedClaudeAccountStore {
    ManagedClaudeAccountStore {
        root: default_wovo_claude_root(),
    }
}

fn sanitized_accounts(
    accounts: Vec<ManagedClaudeAccountRecord>,
) -> Vec<ManagedClaudeAccountRecord> {
    accounts
        .into_iter()
        .filter(|account| !account.home_path.trim().is_empty())
        .map(|mut account| {
            account.email = normalize_optional_email(account.email);
            account.provider_account_id = normalize_optional(account.provider_account_id);
            account.workspace_account_id = normalize_optional(account.workspace_account_id);
            account.workspace_label = normalize_optional(account.workspace_label);
            upgrade_legacy_provider_account_id(&mut account);
            account
        })
        .collect()
}

fn upgrade_legacy_provider_account_id(account: &mut ManagedClaudeAccountRecord) {
    if !is_legacy_token_fingerprint(account.provider_account_id.as_deref()) {
        return;
    }

    let Ok(credentials) = load_credentials_from_home(Path::new(&account.home_path)) else {
        return;
    };
    if let Some(provider_account_id) = credentials.provider_account_id() {
        account.provider_account_id = Some(provider_account_id);
    }
}

fn is_legacy_token_fingerprint(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };
    let Some(fingerprint) = value.strip_prefix("claude-token-") else {
        return false;
    };
    fingerprint.len() == 16 && fingerprint.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn find_matching_account_index(
    accounts: &[ManagedClaudeAccountRecord],
    email: Option<&str>,
    provider_account_id: Option<&str>,
    workspace_account_id: Option<&str>,
) -> Option<usize> {
    if let Some(workspace_account_id) = workspace_account_id {
        if let Some(index) = accounts.iter().position(|account| {
            account.workspace_account_id.as_deref() == Some(workspace_account_id)
        }) {
            return Some(index);
        }

        return provider_account_id.and_then(|provider_account_id| {
            accounts.iter().position(|account| {
                account.workspace_account_id.is_none()
                    && account.provider_account_id.as_deref() == Some(provider_account_id)
            })
        });
    }

    if let Some(provider_account_id) = provider_account_id {
        return accounts.iter().position(|account| {
            account.provider_account_id.as_deref() == Some(provider_account_id)
        });
    }

    let email = email?;
    accounts.iter().position(|account| {
        account.workspace_account_id.is_none()
            && account.provider_account_id.is_none()
            && account
                .email
                .as_deref()
                .map(|account_email| email.eq_ignore_ascii_case(account_email))
                .unwrap_or(false)
    })
}

fn authenticated_identity_matches(
    account: &ManagedClaudeAccountRecord,
    email: Option<&str>,
    provider_account_id: Option<&str>,
    workspace_account_id: Option<&str>,
) -> bool {
    if let Some(workspace_account_id) = workspace_account_id {
        if account.workspace_account_id.as_deref() == Some(workspace_account_id) {
            return true;
        }
        return account.workspace_account_id.is_none()
            && provider_account_id.is_some()
            && account.provider_account_id.as_deref() == provider_account_id;
    }

    if let Some(provider_account_id) = provider_account_id {
        return account.provider_account_id.as_deref() == Some(provider_account_id);
    }

    account.workspace_account_id.is_none()
        && account.provider_account_id.is_none()
        && account
            .email
            .as_deref()
            .map(|account_email| {
                email
                    .map(|email| email.eq_ignore_ascii_case(account_email))
                    .unwrap_or(false)
            })
            .unwrap_or(true)
}

fn normalize_optional_email(value: Option<String>) -> Option<String> {
    normalize_optional(value).map(|value| value.to_ascii_lowercase())
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
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

fn account_store_mutation_lock() -> Result<MutexGuard<'static, ()>, AppError> {
    ACCOUNT_STORE_MUTATION_LOCK
        .lock()
        .map_err(|_| AppError::ClaudeAccountStore("account store lock was poisoned".to_string()))
}

#[cfg(unix)]
fn apply_secure_file_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions)
        .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))
}

#[cfg(not(unix))]
fn apply_secure_file_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> ManagedClaudeAccountStore {
        ManagedClaudeAccountStore::new(
            std::env::temp_dir().join(format!("wovo-claude-store-{name}-{}", Uuid::new_v4())),
        )
    }

    #[test]
    fn upsert_dedupes_by_provider_account_id() {
        let store = temp_store("dedupe");
        let home1 = store.create_home(Uuid::new_v4()).unwrap();
        let home2 = store.create_home(Uuid::new_v4()).unwrap();

        let (first, _) = store
            .upsert_authenticated_account(
                Uuid::new_v4(),
                Some("USER@example.com".to_string()),
                Some("token-1".to_string()),
                None,
                None,
                home1,
            )
            .unwrap();
        let (second, replaced) = store
            .upsert_authenticated_account(
                Uuid::new_v4(),
                Some("user@example.com".to_string()),
                Some("token-1".to_string()),
                None,
                None,
                home2,
            )
            .unwrap();

        assert_eq!(first.id, second.id);
        assert_eq!(replaced.len(), 1);
        assert_eq!(store.load_accounts().unwrap().len(), 1);
        let _ = fs::remove_dir_all(store.root);
    }

    #[test]
    fn update_account_provider_id_refreshes_existing_record() {
        let store = temp_store("update-provider-id");
        let account_id = Uuid::new_v4();
        let home = store.create_home(account_id).unwrap();
        store
            .upsert_authenticated_account(
                account_id,
                Some("user@example.com".to_string()),
                Some("old-provider".to_string()),
                None,
                None,
                home,
            )
            .unwrap();

        store
            .update_account_provider_id(account_id, Some("new-provider".to_string()))
            .unwrap();

        let account = store.find_account(&account_id.to_string()).unwrap();
        assert_eq!(account.provider_account_id.as_deref(), Some("new-provider"));
        let _ = fs::remove_dir_all(store.root);
    }

    #[test]
    fn same_email_with_different_provider_accounts_can_coexist() {
        let store = temp_store("same-email-different-provider");
        let home1 = store.create_home(Uuid::new_v4()).unwrap();
        let home2 = store.create_home(Uuid::new_v4()).unwrap();

        let (first, _) = store
            .upsert_authenticated_account(
                Uuid::new_v4(),
                Some("user@example.com".to_string()),
                Some("provider-1".to_string()),
                None,
                None,
                home1,
            )
            .unwrap();
        let (second, replaced) = store
            .upsert_authenticated_account(
                Uuid::new_v4(),
                Some("USER@example.com".to_string()),
                Some("provider-2".to_string()),
                None,
                None,
                home2,
            )
            .unwrap();

        assert_ne!(first.id, second.id);
        assert!(replaced.is_empty());
        assert_eq!(store.load_accounts().unwrap().len(), 2);
        let _ = fs::remove_dir_all(store.root);
    }

    #[test]
    fn reauth_rejects_same_email_with_different_provider_id() {
        let store = temp_store("reauth-different-provider");
        let home1 = store.create_home(Uuid::new_v4()).unwrap();
        let home2 = store.create_home(Uuid::new_v4()).unwrap();

        let (first, _) = store
            .upsert_authenticated_account(
                Uuid::new_v4(),
                Some("user@example.com".to_string()),
                Some("provider-1".to_string()),
                None,
                None,
                home1,
            )
            .unwrap();

        let error = store
            .upsert_authenticated_account(
                first.id,
                Some("USER@example.com".to_string()),
                Some("provider-2".to_string()),
                None,
                None,
                home2,
            )
            .unwrap_err();

        assert!(matches!(error, AppError::ClaudeAccountIdentityMismatch));
        let accounts = store.load_accounts().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(
            accounts[0].provider_account_id.as_deref(),
            Some("provider-1")
        );
        let _ = fs::remove_dir_all(store.root);
    }

    #[test]
    fn upsert_preserves_existing_labels_when_reauth_only_has_provider_id() {
        let store = temp_store("preserve-labels");
        let home1 = store.create_home(Uuid::new_v4()).unwrap();
        let home2 = store.create_home(Uuid::new_v4()).unwrap();

        let (first, _) = store
            .upsert_authenticated_account(
                Uuid::new_v4(),
                Some("user@example.com".to_string()),
                Some("provider-1".to_string()),
                None,
                Some("Claude Team".to_string()),
                home1,
            )
            .unwrap();
        let (second, _) = store
            .upsert_authenticated_account(
                first.id,
                None,
                Some("provider-1".to_string()),
                None,
                None,
                home2,
            )
            .unwrap();

        assert_eq!(second.id, first.id);
        assert_eq!(second.email.as_deref(), Some("user@example.com"));
        assert_eq!(second.workspace_label.as_deref(), Some("Claude Team"));
        let _ = fs::remove_dir_all(store.root);
    }

    #[test]
    fn load_accounts_upgrades_legacy_token_fingerprints_from_credentials() {
        let store = temp_store("legacy-fingerprint");
        let id = Uuid::new_v4();
        let home = store.create_home(id).unwrap();
        fs::write(
            home.join(".credentials.json"),
            r#"{
                "claudeAiOauth": {
                    "accessToken": "access",
                    "refreshToken": "refresh"
                }
            }"#,
        )
        .unwrap();
        fs::create_dir_all(&store.root).unwrap();
        let stored = ManagedClaudeAccountSet {
            version: STORE_VERSION,
            accounts: vec![ManagedClaudeAccountRecord {
                id,
                email: None,
                provider_account_id: Some("claude-token-0123456789abcdef".to_string()),
                workspace_account_id: None,
                workspace_label: None,
                home_path: home.to_string_lossy().to_string(),
                created_at: 1,
                updated_at: 1,
                last_authenticated_at: Some(1),
            }],
        };
        fs::write(
            store.store_path(),
            serde_json::to_vec_pretty(&stored).unwrap(),
        )
        .unwrap();

        let accounts = store.load_accounts().unwrap();

        assert_eq!(
            accounts[0].provider_account_id.as_deref(),
            Some("claude-token-d6cc0a088c07683c65cd266860cab8d94b3a1937b17420d9da30ca299c09fb77")
        );
        let _ = fs::remove_dir_all(store.root);
    }

    #[test]
    fn remove_account_preserves_record_when_home_cleanup_fails() {
        let store = temp_store("remove-failure");
        fs::create_dir_all(store.managed_homes_dir()).unwrap();
        let home = store.managed_homes_dir().join("home-file");
        fs::write(&home, b"not a directory").unwrap();
        let (account, _) = store
            .upsert_authenticated_account(
                Uuid::new_v4(),
                Some("user@example.com".to_string()),
                Some("provider-1".to_string()),
                None,
                None,
                home.clone(),
            )
            .unwrap();

        let error = store.remove_account(&account.id.to_string()).unwrap_err();

        assert!(matches!(error, AppError::ClaudeAccountStore(_)));
        let accounts = store.load_accounts().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, account.id);
        assert!(home.exists());
        let _ = fs::remove_dir_all(store.root);
    }

    #[cfg(unix)]
    #[test]
    fn remove_account_restores_home_when_store_update_fails() {
        use std::os::unix::fs::PermissionsExt;

        let store = temp_store("store-failure");
        let home = store.create_home(Uuid::new_v4()).unwrap();
        fs::write(home.join(".credentials.json"), "{}").unwrap();
        let (account, _) = store
            .upsert_authenticated_account(
                Uuid::new_v4(),
                Some("user@example.com".to_string()),
                Some("provider-1".to_string()),
                None,
                None,
                home.clone(),
            )
            .unwrap();
        fs::set_permissions(&store.root, fs::Permissions::from_mode(0o555)).unwrap();

        let error = store.remove_account(&account.id.to_string()).unwrap_err();

        fs::set_permissions(&store.root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(error, AppError::ClaudeAccountStore(_)));
        let accounts = store.load_accounts().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, account.id);
        assert!(home.exists());
        let _ = fs::remove_dir_all(store.root);
    }
}
