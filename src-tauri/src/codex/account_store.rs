use crate::domain::account::AccountSummary;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use uuid::Uuid;

mod fs_ops;
mod identity;

use fs_ops::{
    apply_secure_file_permissions, canonical_or_original, cleanup_error_if_unsafe,
    copy_account_local_entries, create_symlink, directory_is_empty, is_account_local_entry,
    move_path, promote_entry_to_shared, remove_path_if_exists, remove_symlink_path,
    shared_link_points_to,
};
use identity::{
    authenticated_identity_matches, find_matching_account_index, normalize_optional,
    normalize_optional_email, sanitized_accounts,
};

const STORE_VERSION: u16 = 1;
const STORE_FILE_NAME: &str = "managed-codex-accounts.json";
const HOMES_DIR_NAME: &str = "accounts";
const CURRENT_LINK_NAME: &str = "current";
const CURRENT_ACCOUNT_ID_FILE_NAME: &str = "current-account-id";
const ACCOUNT_LOCAL_CODEX_ENTRIES: &[&str] = &["auth.json"];
const KNOWN_SHARED_CODEX_DIRS: &[&str] = &[
    ".tmp",
    "sessions",
    "archived_sessions",
    "log",
    "logs",
    "sqlite",
    "tmp",
    "cache",
    "memories",
    "plugins",
    "rules",
    "shell_snapshots",
    "skills",
    "vendor_imports",
];
const KNOWN_SHARED_CODEX_FILES: &[&str] = &[
    ".codex-global-state.json",
    ".codex-global-state.json.bak",
    ".personality_migration",
    "AGENTS.md",
    "config.toml",
    "history.jsonl",
    "installation_id",
    "logs_2.sqlite",
    "logs_2.sqlite-shm",
    "logs_2.sqlite-wal",
    "models_cache.json",
    "session_index.jsonl",
    "state_5.sqlite",
    "state_5.sqlite-shm",
    "state_5.sqlite-wal",
    "version.json",
];
const BACKUPS_DIR_NAME: &str = "backups";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedCodexAccountRecord {
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

impl ManagedCodexAccountRecord {
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
struct ManagedCodexAccountSet {
    version: u16,
    accounts: Vec<ManagedCodexAccountRecord>,
}

#[derive(Debug, Clone)]
pub struct ManagedCodexAccountStore {
    root: PathBuf,
    legacy_root: Option<PathBuf>,
    shared_codex_home: PathBuf,
}

impl ManagedCodexAccountStore {
    #[cfg(test)]
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            legacy_root: None,
            shared_codex_home: real_codex_home_path(),
        }
    }

    pub fn with_legacy_root(root: PathBuf, legacy_root: PathBuf) -> Self {
        Self {
            root,
            legacy_root: Some(legacy_root),
            shared_codex_home: real_codex_home_path(),
        }
    }

    #[cfg(test)]
    pub fn with_shared_codex_home(mut self, shared_codex_home: PathBuf) -> Self {
        self.shared_codex_home = shared_codex_home;
        self
    }

    pub fn managed_homes_dir(&self) -> PathBuf {
        self.root.join(HOMES_DIR_NAME)
    }

    pub fn current_link_path(&self) -> PathBuf {
        self.root.join(CURRENT_LINK_NAME)
    }

    pub fn cleanup_legacy_current_state(&self) -> Result<(), AppError> {
        cleanup_error_if_unsafe(remove_path_if_exists(&self.current_account_id_path()))?;

        let current = self.current_link_path();
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return cleanup_error_if_unsafe(Err(AppError::AccountStore(error.to_string())))
            }
        };

        if metadata.file_type().is_symlink() {
            return cleanup_error_if_unsafe(remove_symlink_path(&current));
        }

        #[cfg(windows)]
        if metadata.is_dir() && fs_ops::is_reparse_point(&metadata) {
            return cleanup_error_if_unsafe(
                fs::remove_dir(&current).map_err(|error| AppError::AccountStore(error.to_string())),
            );
        }

        if metadata.is_dir() {
            if let Err(error) = self.validate_root_child(&current) {
                return cleanup_error_if_unsafe(Err(error));
            }
            let is_empty = match directory_is_empty(&current) {
                Ok(is_empty) => is_empty,
                Err(error) => return cleanup_error_if_unsafe(Err(error)),
            };
            if is_empty {
                cleanup_error_if_unsafe(
                    fs::remove_dir(&current)
                        .map_err(|error| AppError::AccountStore(error.to_string())),
                )
            } else {
                cleanup_error_if_unsafe(self.backup_legacy_current_directory(&current))
            }
        } else {
            cleanup_error_if_unsafe(
                fs::remove_file(&current)
                    .map_err(|error| AppError::AccountStore(error.to_string())),
            )
        }
    }

    pub fn make_home_path(&self, id: Uuid) -> PathBuf {
        self.managed_homes_dir().join(id.to_string())
    }

    pub fn create_home(&self, id: Uuid) -> Result<PathBuf, AppError> {
        let home = self.make_home_path(id);
        self.prepare_home(&home)?;
        Ok(home)
    }

    pub fn load_accounts(&self) -> Result<Vec<ManagedCodexAccountRecord>, AppError> {
        self.migrate_legacy_store_if_needed()?;
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

    pub fn find_matching_account(
        &self,
        email: Option<&str>,
        provider_account_id: Option<&str>,
        workspace_account_id: Option<&str>,
    ) -> Result<Option<ManagedCodexAccountRecord>, AppError> {
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

    pub fn find_account(&self, account_id: &str) -> Result<ManagedCodexAccountRecord, AppError> {
        let id = Uuid::parse_str(account_id)
            .map_err(|_| AppError::UnknownAccount(account_id.to_string()))?;
        self.load_accounts()?
            .into_iter()
            .find(|account| account.id == id)
            .ok_or_else(|| AppError::UnknownAccount(account_id.to_string()))
    }

    #[allow(dead_code)]
    pub fn upsert_authenticated_account(
        &self,
        preferred_id: Uuid,
        email: Option<String>,
        provider_account_id: Option<String>,
        home_path: PathBuf,
    ) -> Result<(ManagedCodexAccountRecord, Vec<PathBuf>), AppError> {
        self.upsert_authenticated_account_with_workspace(
            preferred_id,
            email,
            provider_account_id,
            None,
            None,
            home_path,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_authenticated_account_with_workspace(
        &self,
        preferred_id: Uuid,
        email: Option<String>,
        provider_account_id: Option<String>,
        workspace_account_id: Option<String>,
        workspace_label: Option<String>,
        home_path: PathBuf,
    ) -> Result<(ManagedCodexAccountRecord, Vec<PathBuf>), AppError> {
        let normalized_email = normalize_optional_email(email);
        let normalized_provider_account_id = normalize_optional(provider_account_id);
        let normalized_workspace_account_id = normalize_optional(workspace_account_id);
        let normalized_workspace_label = normalize_optional(workspace_label);
        let normalized_identity_id = normalized_workspace_account_id
            .as_deref()
            .or(normalized_provider_account_id.as_deref());
        if normalized_email.is_none() && normalized_identity_id.is_none() {
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
                normalized_workspace_account_id.as_deref(),
            ) {
                return Err(AppError::AccountIdentityMismatch);
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
        let preserve_existing_workspace =
            normalized_workspace_account_id.is_none() && normalized_workspace_label.is_none();
        let record = ManagedCodexAccountRecord {
            id,
            email: normalized_email,
            provider_account_id: normalized_provider_account_id,
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
        self.store_accounts(accounts)?;

        Ok((record, replaced_home_paths))
    }

    #[allow(dead_code)]
    pub fn upsert_authenticated_account_and_then<F>(
        &self,
        preferred_id: Uuid,
        email: Option<String>,
        provider_account_id: Option<String>,
        home_path: PathBuf,
        after_upsert: F,
    ) -> Result<(ManagedCodexAccountRecord, Vec<PathBuf>), AppError>
    where
        F: FnOnce(&ManagedCodexAccountRecord) -> Result<(), AppError>,
    {
        self.upsert_authenticated_account_with_workspace_and_then(
            preferred_id,
            email,
            provider_account_id,
            None,
            None,
            home_path,
            after_upsert,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_authenticated_account_with_workspace_and_then<F>(
        &self,
        preferred_id: Uuid,
        email: Option<String>,
        provider_account_id: Option<String>,
        workspace_account_id: Option<String>,
        workspace_label: Option<String>,
        home_path: PathBuf,
        after_upsert: F,
    ) -> Result<(ManagedCodexAccountRecord, Vec<PathBuf>), AppError>
    where
        F: FnOnce(&ManagedCodexAccountRecord) -> Result<(), AppError>,
    {
        let previous_accounts = self.load_accounts()?;
        let (record, replaced_home_paths) = self.upsert_authenticated_account_with_workspace(
            preferred_id,
            email,
            provider_account_id,
            workspace_account_id,
            workspace_label,
            home_path,
        )?;

        if let Err(error) = after_upsert(&record) {
            let error_message = error.to_string();
            self.store_accounts(previous_accounts)
                .map_err(|rollback_error| {
                    AppError::AccountStore(format!(
                        "account update failed ({error_message}); rollback failed ({rollback_error})"
                    ))
                })?;
            return Err(error);
        }

        Ok((record, replaced_home_paths))
    }

    pub fn update_account_workspace(
        &self,
        account_id: Uuid,
        workspace_account_id: Option<String>,
        workspace_label: Option<String>,
    ) -> Result<(), AppError> {
        let normalized_workspace_account_id = normalize_optional(workspace_account_id);
        let normalized_workspace_label = normalize_optional(workspace_label);
        if normalized_workspace_account_id.is_none() && normalized_workspace_label.is_none() {
            return Ok(());
        }

        let mut accounts = self.load_accounts()?;
        if let Some(workspace_account_id) = normalized_workspace_account_id.as_deref() {
            let duplicate = accounts.iter().any(|account| {
                account.id != account_id
                    && account.workspace_account_id.as_deref() == Some(workspace_account_id)
            });
            if duplicate {
                return Ok(());
            }
        }

        let Some(account) = accounts.iter_mut().find(|account| account.id == account_id) else {
            return Err(AppError::UnknownAccount(account_id.to_string()));
        };

        account.workspace_account_id = normalized_workspace_account_id;
        account.workspace_label = normalized_workspace_label;
        account.updated_at = OffsetDateTime::now_utc().unix_timestamp();
        self.store_accounts(accounts)
    }

    pub fn import_auth_from_home(
        &self,
        source_home: &Path,
        target_home: &Path,
    ) -> Result<(), AppError> {
        self.validate_managed_home(target_home)?;
        self.prepare_home(target_home)?;
        let source_auth = source_home.join("auth.json");
        let target_auth = target_home.join("auth.json");
        fs::copy(&source_auth, &target_auth)
            .map_err(|error| AppError::AuthRead(error.to_string()))?;
        apply_secure_file_permissions(&target_auth)?;
        Ok(())
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

    fn prepare_home(&self, home: &Path) -> Result<(), AppError> {
        fs::create_dir_all(home).map_err(|error| AppError::AccountStore(error.to_string()))?;
        fs::create_dir_all(&self.shared_codex_home)
            .map_err(|error| AppError::AccountStore(error.to_string()))?;
        self.promote_managed_state_to_shared(home)?;
        self.link_shared_codex_entries(home)?;
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

    fn current_account_id_path(&self) -> PathBuf {
        self.root.join(CURRENT_ACCOUNT_ID_FILE_NAME)
    }

    fn legacy_store_path(&self) -> Option<PathBuf> {
        self.legacy_root
            .as_ref()
            .map(|root| root.join(STORE_FILE_NAME))
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

    fn validate_root_child(&self, path: &Path) -> Result<(), AppError> {
        let root = canonical_or_original(&self.root)?;
        let target = canonical_or_original(path)?;

        if target == root || !target.starts_with(&root) {
            return Err(AppError::UnsafeManagedHome(
                path.to_string_lossy().to_string(),
            ));
        }
        Ok(())
    }

    fn migrate_legacy_store_if_needed(&self) -> Result<(), AppError> {
        if self.store_path().exists() {
            return Ok(());
        }
        let Some(legacy_store_path) = self.legacy_store_path() else {
            return Ok(());
        };
        if !legacy_store_path.exists() {
            return Ok(());
        }

        let contents = fs::read_to_string(&legacy_store_path)
            .map_err(|error| AppError::AccountStore(error.to_string()))?;
        let decoded: ManagedCodexAccountSet = serde_json::from_str(&contents)
            .map_err(|error| AppError::AccountStore(error.to_string()))?;
        if decoded.version != STORE_VERSION {
            return Err(AppError::AccountStore(format!(
                "unsupported legacy account store version {}",
                decoded.version
            )));
        }

        let mut migrated = Vec::new();
        for mut account in sanitized_accounts(decoded.accounts) {
            let source_home = PathBuf::from(&account.home_path);
            let target_home = self.make_home_path(account.id);
            fs::create_dir_all(&target_home)
                .map_err(|error| AppError::AccountStore(error.to_string()))?;
            if source_home.exists()
                && canonical_or_original(&source_home)? != canonical_or_original(&target_home)?
            {
                self.promote_managed_state_to_shared(&source_home)?;
                copy_account_local_entries(&source_home, &target_home)?;
            }
            self.prepare_home(&target_home)?;
            account.home_path = target_home.to_string_lossy().to_string();
            migrated.push(account);
        }

        self.store_accounts(migrated)?;
        Ok(())
    }

    fn link_shared_codex_entries(&self, home: &Path) -> Result<(), AppError> {
        for entry in self.shared_codex_entry_names()? {
            let source = self.shared_codex_home.join(&entry);
            if is_account_local_entry(&entry) {
                continue;
            }
            let source_exists = source.exists() || fs::symlink_metadata(&source).is_ok();
            if KNOWN_SHARED_CODEX_DIRS.contains(&entry.as_str()) && !source_exists {
                fs::create_dir_all(&source)
                    .map_err(|error| AppError::AccountStore(error.to_string()))?;
            } else if !source_exists {
                continue;
            }
            let target = home.join(entry);
            if shared_link_points_to(&target, &source)? {
                continue;
            }
            remove_path_if_exists(&target)?;
            create_symlink(&source, &target)?;
        }
        Ok(())
    }

    fn promote_managed_state_to_shared(&self, home: &Path) -> Result<(), AppError> {
        if !home.exists() {
            return Ok(());
        }

        for entry in
            fs::read_dir(home).map_err(|error| AppError::AccountStore(error.to_string()))?
        {
            let entry = entry.map_err(|error| AppError::AccountStore(error.to_string()))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if is_account_local_entry(name) {
                continue;
            }

            let source = entry.path();
            let target = self.shared_codex_home.join(name);
            if shared_link_points_to(&source, &target)? {
                continue;
            }

            let backup_root = self.backup_root_for_home(home);
            promote_entry_to_shared(&source, &target, &backup_root)?;
        }

        Ok(())
    }

    fn shared_codex_entry_names(&self) -> Result<Vec<String>, AppError> {
        let mut entries = BTreeSet::new();
        for entry in KNOWN_SHARED_CODEX_DIRS {
            entries.insert((*entry).to_string());
        }
        for entry in KNOWN_SHARED_CODEX_FILES {
            entries.insert((*entry).to_string());
        }

        match fs::read_dir(&self.shared_codex_home) {
            Ok(shared_entries) => {
                for entry in shared_entries {
                    let entry = entry.map_err(|error| AppError::AccountStore(error.to_string()))?;
                    if let Some(name) = entry.file_name().to_str() {
                        entries.insert(name.to_string());
                    }
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(AppError::AccountStore(error.to_string())),
        }

        Ok(entries.into_iter().collect())
    }

    fn backup_root_for_home(&self, home: &Path) -> PathBuf {
        let home_name = home
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown-home".to_string());
        self.root.join(BACKUPS_DIR_NAME).join(home_name)
    }

    fn backup_legacy_current_directory(&self, current: &Path) -> Result<(), AppError> {
        let backup_root = self.root.join(BACKUPS_DIR_NAME);
        fs::create_dir_all(&backup_root)
            .map_err(|error| AppError::AccountStore(error.to_string()))?;
        let timestamp = OffsetDateTime::now_utc().unix_timestamp();
        let mut backup = backup_root.join(format!("current-legacy-{timestamp}"));
        let mut suffix = 1;
        while backup.exists() {
            backup = backup_root.join(format!("current-legacy-{timestamp}-{suffix}"));
            suffix += 1;
        }
        move_path(current, &backup)
    }
}

pub fn default_wovo_codex_root() -> PathBuf {
    dirs_home().join(".wovo").join("codex")
}

fn real_codex_home_path() -> PathBuf {
    dirs_home().join(".codex")
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

#[cfg(test)]
mod tests;
