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
    pub home_path: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_authenticated_at: Option<i64>,
}

impl ManagedCodexAccountRecord {
    pub fn summary(&self) -> AccountSummary {
        self.summary_with_status(false, false)
    }

    pub fn summary_with_status(&self, is_active: bool, is_live_system: bool) -> AccountSummary {
        AccountSummary::managed(
            self.id.to_string(),
            self.email.clone(),
            self.provider_account_id.clone(),
            self.home_path.clone(),
            self.created_at,
            self.updated_at,
            self.last_authenticated_at,
            is_active,
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

    pub fn has_materialized_current_directory(&self) -> Result<bool, AppError> {
        match fs::symlink_metadata(self.current_link_path()) {
            Ok(metadata) => {
                let is_replaceable_directory = {
                    #[cfg(windows)]
                    {
                        is_reparse_point(&metadata)
                    }
                    #[cfg(not(windows))]
                    {
                        false
                    }
                };
                Ok(metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && !is_replaceable_directory)
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(AppError::AccountStore(error.to_string())),
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
    ) -> Result<Option<ManagedCodexAccountRecord>, AppError> {
        let normalized_email = normalize_optional_email(email.map(str::to_string));
        let normalized_provider_account_id =
            normalize_optional(provider_account_id.map(str::to_string));
        let accounts = self.load_accounts()?;
        Ok(find_matching_account_index(
            &accounts,
            normalized_email.as_deref(),
            normalized_provider_account_id.as_deref(),
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
        } else {
            find_matching_account_index(
                &accounts,
                normalized_email.as_deref(),
                normalized_provider_account_id.as_deref(),
            )
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

    pub fn upsert_authenticated_account_and_switch_if(
        &self,
        preferred_id: Uuid,
        email: Option<String>,
        provider_account_id: Option<String>,
        home_path: PathBuf,
        selected_before: Option<Uuid>,
    ) -> Result<(ManagedCodexAccountRecord, Vec<PathBuf>), AppError> {
        let previous_accounts = self.load_accounts()?;
        let (record, replaced_home_paths) =
            self.upsert_authenticated_account(preferred_id, email, provider_account_id, home_path)?;

        let switch_current = selected_before.is_none() || selected_before == Some(record.id);
        if switch_current {
            if let Err(error) = self.switch_to_account(&record.id.to_string()) {
                let error_message = error.to_string();
                self.store_accounts(previous_accounts)
                    .map_err(|rollback_error| {
                        AppError::AccountStore(format!(
                            "switch failed ({error_message}); rollback failed ({rollback_error})"
                        ))
                    })?;
                return Err(error);
            }
        }

        Ok((record, replaced_home_paths))
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

    pub fn switch_to_account(
        &self,
        account_id: &str,
    ) -> Result<ManagedCodexAccountRecord, AppError> {
        let account = self.find_account(account_id)?;
        let home = PathBuf::from(&account.home_path);
        self.validate_managed_home(&home)?;
        self.prepare_home(&home)?;
        self.write_current_link(&home)?;
        self.write_current_account_id(account.id)?;
        Ok(account)
    }

    pub fn selected_account_id(&self) -> Result<Option<Uuid>, AppError> {
        let Some(current_home) = self.current_home_target()? else {
            return Ok(None);
        };
        let current_home = canonical_or_original(&current_home)?;
        let accounts = self.load_accounts()?;
        for account in &accounts {
            let account_home = canonical_or_original(Path::new(&account.home_path))?;
            if account_home == current_home {
                return Ok(Some(account.id));
            }
        }
        Ok(None)
    }

    pub fn remove_account(&self, account_id: &str) -> Result<(), AppError> {
        let id = Uuid::parse_str(account_id)
            .map_err(|_| AppError::UnknownAccount(account_id.to_string()))?;
        if self.selected_account_id()? == Some(id) {
            return Err(AppError::ActiveAccountRemovalBlocked);
        }

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

    fn current_home_target(&self) -> Result<Option<PathBuf>, AppError> {
        let current = self.current_link_path();
        match fs::read_link(&current) {
            Ok(target) => Ok(Some(if target.is_absolute() {
                target
            } else {
                self.root.join(target)
            })),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(_) if current.exists() => Ok(Some(current)),
            Err(error) => Err(AppError::AccountStore(error.to_string())),
        }
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

    fn write_current_link(&self, home: &Path) -> Result<(), AppError> {
        fs::create_dir_all(&self.root)
            .map_err(|error| AppError::AccountStore(error.to_string()))?;
        let current = self.current_link_path();
        let tmp = self.root.join(format!(".current-{}.tmp", Uuid::new_v4()));
        remove_path_if_exists(&tmp)?;
        create_symlink(home, &tmp)?;
        match fs::rename(&tmp, &current) {
            Ok(()) => {}
            Err(error) => {
                #[cfg(not(unix))]
                {
                    if let Err(remove_error) = remove_current_link_if_replaceable(&current) {
                        let _ = remove_path_if_exists(&tmp);
                        return Err(remove_error);
                    }
                    fs::rename(&tmp, &current).map_err(|retry_error| {
                        let _ = remove_path_if_exists(&tmp);
                        AppError::AccountStore(format!("{error}; retry failed: {retry_error}"))
                    })?;
                }

                #[cfg(unix)]
                {
                    let _ = remove_path_if_exists(&tmp);
                    return Err(AppError::AccountStore(error.to_string()));
                }
            }
        }
        Ok(())
    }

    fn write_current_account_id(&self, account_id: Uuid) -> Result<(), AppError> {
        let target = self.current_account_id_path();
        let tmp = self
            .root
            .join(format!(".current-account-id-{}.tmp", Uuid::new_v4()));
        fs::write(&tmp, account_id.to_string())
            .map_err(|error| AppError::AccountStore(error.to_string()))?;
        apply_secure_file_permissions(&tmp)?;
        fs::rename(&tmp, &target).map_err(|error| {
            let _ = remove_path_if_exists(&tmp);
            AppError::AccountStore(error.to_string())
        })?;
        Ok(())
    }
}

#[cfg(windows)]
fn create_symlink(source: &Path, target: &Path) -> Result<(), AppError> {
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(source, target)
            .or_else(|_| create_directory_junction(source, target))
            .map_err(|error| AppError::AccountStore(error.to_string()))
    } else {
        std::os::windows::fs::symlink_file(source, target).map_err(|error| {
            AppError::AccountStore(format!(
                "failed to create shared Codex file link from {} to {}: {error}",
                target.to_string_lossy(),
                source.to_string_lossy()
            ))
        })
    }
}

#[cfg(windows)]
fn create_directory_junction(source: &Path, target: &Path) -> std::io::Result<()> {
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(target)
        .arg(source)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            ErrorKind::Other,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

#[cfg(all(not(unix), not(windows)))]
fn create_symlink(_source: &Path, _target: &Path) -> Result<(), AppError> {
    Err(AppError::AccountStore(
        "directory links are not supported on this platform".to_string(),
    ))
}

#[cfg(unix)]
fn create_symlink(source: &Path, target: &Path) -> Result<(), AppError> {
    std::os::unix::fs::symlink(source, target)
        .map_err(|error| AppError::AccountStore(error.to_string()))
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

fn find_matching_account_index(
    accounts: &[ManagedCodexAccountRecord],
    email: Option<&str>,
    provider_account_id: Option<&str>,
) -> Option<usize> {
    if let Some(provider_account_id) = provider_account_id {
        if let Some(index) = accounts
            .iter()
            .position(|account| account.provider_account_id.as_deref() == Some(provider_account_id))
        {
            return Some(index);
        }

        if let Some(email) = email {
            return accounts.iter().position(|account| {
                account.provider_account_id.is_none() && account.email.as_deref() == Some(email)
            });
        }

        return None;
    }

    let email = email?;
    accounts.iter().position(|account| {
        account.provider_account_id.is_none() && account.email.as_deref() == Some(email)
    })
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

fn is_account_local_entry(name: &str) -> bool {
    if ACCOUNT_LOCAL_CODEX_ENTRIES.contains(&name) {
        return true;
    }

    let lower = name.to_ascii_lowercase();
    lower.starts_with("auth.") || lower.contains("token") || lower.contains("credential")
}

fn shared_link_points_to(path: &Path, expected_target: &Path) -> Result<bool, AppError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(AppError::AccountStore(error.to_string())),
    };
    if !metadata.file_type().is_symlink() {
        return Ok(false);
    }

    let link_target =
        fs::read_link(path).map_err(|error| AppError::AccountStore(error.to_string()))?;
    let normalized_target = if link_target.is_absolute() {
        link_target
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new(""))
            .join(link_target)
    };
    Ok(canonical_or_original(&normalized_target)? == canonical_or_original(expected_target)?)
}

fn promote_entry_to_shared(
    source: &Path,
    target: &Path,
    backup_root: &Path,
) -> Result<(), AppError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| AppError::AccountStore(error.to_string()))?;
    }

    let source_metadata =
        fs::symlink_metadata(source).map_err(|error| AppError::AccountStore(error.to_string()))?;
    let target_metadata = match fs::symlink_metadata(target) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(AppError::AccountStore(error.to_string())),
    };

    match target_metadata {
        None => move_path(source, target),
        Some(target_metadata)
            if source_metadata.is_dir()
                && !source_metadata.file_type().is_symlink()
                && target_metadata.is_dir()
                && !target_metadata.file_type().is_symlink() =>
        {
            merge_dir_contents(source, target, &backup_root.join(path_file_name(source)))?;
            fs::remove_dir_all(source).map_err(|error| AppError::AccountStore(error.to_string()))
        }
        Some(target_metadata)
            if source_metadata.is_file()
                && target_metadata.is_file()
                && same_file_contents(source, target)? =>
        {
            fs::remove_file(source).map_err(|error| AppError::AccountStore(error.to_string()))
        }
        Some(_) => backup_and_remove_path(source, backup_root),
    }
}

fn merge_dir_contents(source: &Path, target: &Path, backup_root: &Path) -> Result<(), AppError> {
    fs::create_dir_all(target).map_err(|error| AppError::AccountStore(error.to_string()))?;
    for entry in fs::read_dir(source).map_err(|error| AppError::AccountStore(error.to_string()))? {
        let entry = entry.map_err(|error| AppError::AccountStore(error.to_string()))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let source_metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| AppError::AccountStore(error.to_string()))?;
        let target_metadata = match fs::symlink_metadata(&target_path) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(AppError::AccountStore(error.to_string())),
        };

        match target_metadata {
            None => move_path(&source_path, &target_path)?,
            Some(target_metadata)
                if source_metadata.is_dir()
                    && !source_metadata.file_type().is_symlink()
                    && target_metadata.is_dir()
                    && !target_metadata.file_type().is_symlink() =>
            {
                merge_dir_contents(
                    &source_path,
                    &target_path,
                    &backup_root.join(entry.file_name()),
                )?;
                fs::remove_dir_all(&source_path)
                    .map_err(|error| AppError::AccountStore(error.to_string()))?;
            }
            Some(target_metadata)
                if source_metadata.is_file()
                    && target_metadata.is_file()
                    && same_file_contents(&source_path, &target_path)? =>
            {
                fs::remove_file(&source_path)
                    .map_err(|error| AppError::AccountStore(error.to_string()))?;
            }
            Some(_) => backup_and_remove_path(&source_path, backup_root)?,
        }
    }
    Ok(())
}

fn copy_account_local_entries(source_home: &Path, target_home: &Path) -> Result<(), AppError> {
    fs::create_dir_all(target_home).map_err(|error| AppError::AccountStore(error.to_string()))?;
    for entry in
        fs::read_dir(source_home).map_err(|error| AppError::AccountStore(error.to_string()))?
    {
        let entry = entry.map_err(|error| AppError::AccountStore(error.to_string()))?;
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !is_account_local_entry(name_str) {
            continue;
        }
        let target = target_home.join(&name);
        remove_path_if_exists(&target)?;
        copy_path(&entry.path(), &target)?;
        if name_str == "auth.json" {
            apply_secure_file_permissions(&target)?;
        }
    }
    Ok(())
}

fn move_path(source: &Path, target: &Path) -> Result<(), AppError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| AppError::AccountStore(error.to_string()))?;
    }
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_path(source, target)?;
            remove_path_if_exists(source)
        }
    }
}

fn copy_path(source: &Path, target: &Path) -> Result<(), AppError> {
    let metadata =
        fs::symlink_metadata(source).map_err(|error| AppError::AccountStore(error.to_string()))?;
    if metadata.file_type().is_symlink() {
        let link_target =
            fs::read_link(source).map_err(|error| AppError::AccountStore(error.to_string()))?;
        create_symlink(&link_target, target)
    } else if metadata.is_dir() {
        copy_dir_contents(source, target)
    } else {
        fs::copy(source, target)
            .map(|_| ())
            .map_err(|error| AppError::AccountStore(error.to_string()))
    }
}

fn backup_and_remove_path(source: &Path, backup_root: &Path) -> Result<(), AppError> {
    let backup_path = backup_root.join(format!("{}-{}", path_file_name(source), Uuid::new_v4()));
    move_path(source, &backup_path)
}

fn path_file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn same_file_contents(left: &Path, right: &Path) -> Result<bool, AppError> {
    let left = fs::read(left).map_err(|error| AppError::AccountStore(error.to_string()))?;
    let right = fs::read(right).map_err(|error| AppError::AccountStore(error.to_string()))?;
    Ok(left == right)
}

fn copy_dir_contents(source: &Path, target: &Path) -> Result<(), AppError> {
    fs::create_dir_all(target).map_err(|error| AppError::AccountStore(error.to_string()))?;
    for entry in fs::read_dir(source).map_err(|error| AppError::AccountStore(error.to_string()))? {
        let entry = entry.map_err(|error| AppError::AccountStore(error.to_string()))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());

        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| AppError::AccountStore(error.to_string()))?;
        if let Ok(target_metadata) = fs::symlink_metadata(&target_path) {
            if metadata.is_dir()
                && target_metadata.is_dir()
                && !target_metadata.file_type().is_symlink()
            {
                copy_dir_contents(&source_path, &target_path)?;
            }
            continue;
        }

        if metadata.file_type().is_symlink() {
            let link_target = fs::read_link(&source_path)
                .map_err(|error| AppError::AccountStore(error.to_string()))?;
            create_symlink(&link_target, &target_path)?;
        } else if metadata.is_dir() {
            copy_dir_contents(&source_path, &target_path)?;
        } else {
            fs::copy(&source_path, &target_path)
                .map_err(|error| AppError::AccountStore(error.to_string()))?;
        }
    }
    Ok(())
}

fn remove_path_if_exists(path: &Path) -> Result<(), AppError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => remove_symlink_path(path),
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path).map_err(|error| AppError::AccountStore(error.to_string()))
        }
        Ok(_) => fs::remove_file(path).map_err(|error| AppError::AccountStore(error.to_string())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::AccountStore(error.to_string())),
    }
}

#[cfg_attr(unix, allow(dead_code))]
fn remove_current_link_if_replaceable(path: &Path) -> Result<(), AppError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => remove_symlink_path(path),
        #[cfg(windows)]
        Ok(metadata) if metadata.is_dir() && is_reparse_point(&metadata) => {
            fs::remove_dir(path).map_err(|error| AppError::AccountStore(error.to_string()))
        }
        Ok(metadata) if metadata.is_dir() => Err(AppError::AccountStore(format!(
            "refusing to replace materialized Codex current directory: {}",
            path.to_string_lossy()
        ))),
        Ok(_) => fs::remove_file(path).map_err(|error| AppError::AccountStore(error.to_string())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::AccountStore(error.to_string())),
    }
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(windows)]
fn remove_symlink_path(path: &Path) -> Result<(), AppError> {
    if fs::metadata(path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
    {
        fs::remove_dir(path).map_err(|error| AppError::AccountStore(error.to_string()))
    } else {
        fs::remove_file(path).map_err(|error| AppError::AccountStore(error.to_string()))
    }
}

#[cfg(not(windows))]
fn remove_symlink_path(path: &Path) -> Result<(), AppError> {
    fs::remove_file(path).map_err(|error| AppError::AccountStore(error.to_string()))
}

fn canonical_or_original(path: &Path) -> Result<PathBuf, AppError> {
    path.canonicalize().or_else(|error| {
        if error.kind() == ErrorKind::NotFound {
            Ok(path.to_path_buf())
        } else {
            Err(AppError::AccountStore(error.to_string()))
        }
    })
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
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("wovo-{name}-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[cfg(unix)]
    fn assert_symlink_to(path: &Path, target: &Path) {
        assert!(fs::symlink_metadata(path).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_link(path).unwrap(), target);
    }

    fn path_contains_file_with_contents(path: &Path, contents: &str) -> bool {
        if path.is_file() {
            return fs::read_to_string(path)
                .map(|actual| actual == contents)
                .unwrap_or(false);
        }

        let Ok(entries) = fs::read_dir(path) else {
            return false;
        };
        entries
            .filter_map(Result::ok)
            .any(|entry| path_contains_file_with_contents(&entry.path(), contents))
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

    #[test]
    fn same_email_with_different_provider_accounts_can_coexist() {
        let root = temp_root("same-email-providers");
        let store = ManagedCodexAccountStore::new(root.clone());
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();
        let first_home = store.create_home(first_id).unwrap();
        let second_home = store.create_home(second_id).unwrap();

        store
            .upsert_authenticated_account(
                first_id,
                Some("user@example.com".to_string()),
                Some("account-1".to_string()),
                first_home,
            )
            .unwrap();
        store
            .upsert_authenticated_account(
                second_id,
                Some("user@example.com".to_string()),
                Some("account-2".to_string()),
                second_home,
            )
            .unwrap();

        let loaded = store.load_accounts().unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(loaded.iter().any(|account| {
            account.email.as_deref() == Some("user@example.com")
                && account.provider_account_id.as_deref() == Some("account-1")
        }));
        assert!(loaded.iter().any(|account| {
            account.email.as_deref() == Some("user@example.com")
                && account.provider_account_id.as_deref() == Some("account-2")
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn provider_identity_upgrades_legacy_email_account() {
        let root = temp_root("legacy-upgrade");
        let store = ManagedCodexAccountStore::new(root.clone());
        let legacy_id = Uuid::new_v4();
        let new_id = Uuid::new_v4();
        let legacy_home = store.create_home(legacy_id).unwrap();
        let new_home = store.create_home(new_id).unwrap();

        store
            .upsert_authenticated_account(
                legacy_id,
                Some("user@example.com".to_string()),
                None,
                legacy_home.clone(),
            )
            .unwrap();
        let (record, replaced) = store
            .upsert_authenticated_account(
                new_id,
                Some("user@example.com".to_string()),
                Some("account-1".to_string()),
                new_home,
            )
            .unwrap();

        let loaded = store.load_accounts().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(record.id, legacy_id);
        assert_eq!(record.provider_account_id.as_deref(), Some("account-1"));
        assert_eq!(replaced, vec![legacy_home]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(unix)]
    fn switch_updates_current_symlink() {
        let root = temp_root("switch-current");
        let shared = temp_root("shared-codex");
        fs::write(shared.join("config.toml"), "model = \"test\"").unwrap();
        let store =
            ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
        let id = Uuid::new_v4();
        let home = store.create_home(id).unwrap();
        store
            .upsert_authenticated_account(
                id,
                Some("user@example.com".to_string()),
                Some("account-1".to_string()),
                home.clone(),
            )
            .unwrap();

        store.switch_to_account(&id.to_string()).unwrap();

        assert_eq!(fs::read_link(store.current_link_path()).unwrap(), home);
        assert_eq!(store.selected_account_id().unwrap(), Some(id));
        assert!(shared.join("config.toml").exists());
        assert!(!shared.join("current").exists());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(shared);
    }

    #[test]
    #[cfg(unix)]
    fn managed_home_keeps_auth_local_and_links_shared_state() {
        let root = temp_root("auth-overlay");
        let shared = temp_root("auth-overlay-shared");
        fs::write(shared.join("history.jsonl"), "shared-history").unwrap();
        fs::write(shared.join(".codex-global-state.json"), "{}").unwrap();
        fs::write(shared.join("session_index.jsonl"), "{}").unwrap();
        fs::create_dir_all(shared.join("sessions")).unwrap();
        fs::create_dir_all(shared.join("archived_sessions")).unwrap();
        let store =
            ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());

        let home = store.create_home(Uuid::new_v4()).unwrap();
        fs::write(home.join("auth.json"), "{}").unwrap();

        assert!(!fs::symlink_metadata(home.join("auth.json"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_symlink_to(&home.join("history.jsonl"), &shared.join("history.jsonl"));
        assert_symlink_to(
            &home.join(".codex-global-state.json"),
            &shared.join(".codex-global-state.json"),
        );
        assert_symlink_to(
            &home.join("session_index.jsonl"),
            &shared.join("session_index.jsonl"),
        );
        assert_symlink_to(&home.join("sessions"), &shared.join("sessions"));
        assert_symlink_to(
            &home.join("archived_sessions"),
            &shared.join("archived_sessions"),
        );

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(shared);
    }

    #[test]
    #[cfg(unix)]
    fn prepare_home_promotes_materialized_state_without_deleting_it() {
        let root = temp_root("promote-state");
        let shared = temp_root("promote-state-shared");
        let store =
            ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
        let home = store.make_home_path(Uuid::new_v4());
        fs::create_dir_all(home.join("sessions")).unwrap();
        fs::write(home.join("history.jsonl"), "managed-history").unwrap();
        fs::write(home.join("sessions").join("session.jsonl"), "{}").unwrap();
        fs::write(home.join("auth.json"), "{}").unwrap();

        store.prepare_home(&home).unwrap();

        assert_eq!(
            fs::read_to_string(shared.join("history.jsonl")).unwrap(),
            "managed-history"
        );
        assert!(shared.join("sessions").join("session.jsonl").exists());
        assert_symlink_to(&home.join("history.jsonl"), &shared.join("history.jsonl"));
        assert_symlink_to(&home.join("sessions"), &shared.join("sessions"));
        assert!(!fs::symlink_metadata(home.join("auth.json"))
            .unwrap()
            .file_type()
            .is_symlink());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(shared);
    }

    #[test]
    #[cfg(unix)]
    fn prepare_home_backs_up_unmergeable_shared_conflicts() {
        let root = temp_root("backup-conflict");
        let shared = temp_root("backup-conflict-shared");
        fs::write(shared.join("history.jsonl"), "shared-history").unwrap();
        let store =
            ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
        let home = store.make_home_path(Uuid::new_v4());
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("history.jsonl"), "managed-history").unwrap();

        store.prepare_home(&home).unwrap();

        assert_eq!(
            fs::read_to_string(shared.join("history.jsonl")).unwrap(),
            "shared-history"
        );
        assert_symlink_to(&home.join("history.jsonl"), &shared.join("history.jsonl"));
        assert!(path_contains_file_with_contents(
            &root.join(BACKUPS_DIR_NAME),
            "managed-history"
        ));

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(shared);
    }

    #[test]
    #[cfg(unix)]
    fn switching_accounts_keeps_history_linked_to_same_shared_state() {
        let root = temp_root("shared-switching");
        let shared = temp_root("shared-switching-codex");
        fs::write(shared.join("history.jsonl"), "shared-history").unwrap();
        let store =
            ManagedCodexAccountStore::new(root.clone()).with_shared_codex_home(shared.clone());
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();
        let first_home = store.create_home(first_id).unwrap();
        let second_home = store.create_home(second_id).unwrap();
        store
            .upsert_authenticated_account(
                first_id,
                Some("one@example.com".to_string()),
                Some("account-1".to_string()),
                first_home.clone(),
            )
            .unwrap();
        store
            .upsert_authenticated_account(
                second_id,
                Some("two@example.com".to_string()),
                Some("account-2".to_string()),
                second_home.clone(),
            )
            .unwrap();

        store.switch_to_account(&first_id.to_string()).unwrap();
        assert_symlink_to(
            &store.current_link_path().join("history.jsonl"),
            &shared.join("history.jsonl"),
        );
        store.switch_to_account(&second_id.to_string()).unwrap();
        assert_symlink_to(
            &store.current_link_path().join("history.jsonl"),
            &shared.join("history.jsonl"),
        );

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(shared);
    }

    #[test]
    fn materialized_current_directory_is_not_treated_as_selected() {
        let root = temp_root("switch-current-copy");
        let store = ManagedCodexAccountStore::new(root.clone());
        let id = Uuid::new_v4();
        let home = store.create_home(id).unwrap();
        fs::write(home.join("auth.json"), "{}").unwrap();
        store
            .upsert_authenticated_account(
                id,
                Some("user@example.com".to_string()),
                Some("account-1".to_string()),
                home.clone(),
            )
            .unwrap();

        copy_dir_contents(&home, &store.current_link_path()).unwrap();
        store.write_current_account_id(id).unwrap();

        assert!(store.current_link_path().join("auth.json").exists());
        assert_eq!(store.selected_account_id().unwrap(), None);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn materialized_current_directory_is_not_removed_as_replaceable_link() {
        let root = temp_root("protect-current-directory");
        let current = root.join("current");
        fs::create_dir_all(current.join("sessions")).unwrap();
        fs::write(current.join("auth.json"), "{}").unwrap();
        fs::write(current.join("sessions").join("session.jsonl"), "{}").unwrap();

        let error = remove_current_link_if_replaceable(&current).unwrap_err();

        assert!(matches!(error, AppError::AccountStore(_)));
        assert!(current.join("auth.json").exists());
        assert!(current.join("sessions").join("session.jsonl").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(unix)]
    fn switching_failure_rolls_back_stored_account_home() {
        let root = temp_root("switch-rollback");
        let store = ManagedCodexAccountStore::new(root.clone());
        let id = Uuid::new_v4();
        let first_home = store.create_home(id).unwrap();
        store
            .upsert_authenticated_account(
                id,
                Some("user@example.com".to_string()),
                Some("account-1".to_string()),
                first_home.clone(),
            )
            .unwrap();
        fs::create_dir_all(store.current_link_path()).unwrap();

        let second_home = store.create_home(Uuid::new_v4()).unwrap();
        let error = store
            .upsert_authenticated_account_and_switch_if(
                id,
                Some("updated@example.com".to_string()),
                Some("account-1".to_string()),
                second_home,
                Some(id),
            )
            .unwrap_err();
        let loaded = store.load_accounts().unwrap();

        assert!(matches!(error, AppError::AccountStore(_)));
        assert_eq!(loaded.len(), 1);
        assert_eq!(
            loaded[0].home_path,
            first_home.to_string_lossy().to_string()
        );
        assert_eq!(loaded[0].email.as_deref(), Some("user@example.com"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(unix)]
    fn active_legacy_upgrade_relinks_current_before_old_home_removal() {
        let root = temp_root("active-legacy-upgrade");
        let store = ManagedCodexAccountStore::new(root.clone());
        let legacy_id = Uuid::new_v4();
        let preferred_id = Uuid::new_v4();
        let legacy_home = store.create_home(legacy_id).unwrap();
        store
            .upsert_authenticated_account(
                legacy_id,
                Some("user@example.com".to_string()),
                None,
                legacy_home.clone(),
            )
            .unwrap();
        store.switch_to_account(&legacy_id.to_string()).unwrap();

        let upgraded_home = store.create_home(preferred_id).unwrap();
        let (record, replaced_home_paths) = store
            .upsert_authenticated_account_and_switch_if(
                preferred_id,
                Some("user@example.com".to_string()),
                Some("account-1".to_string()),
                upgraded_home.clone(),
                Some(legacy_id),
            )
            .unwrap();

        assert_eq!(record.id, legacy_id);
        assert_eq!(replaced_home_paths, vec![legacy_home.clone()]);
        assert_eq!(
            fs::read_link(store.current_link_path()).unwrap(),
            upgraded_home
        );

        store.remove_home_if_safe(&legacy_home).unwrap();
        assert_eq!(store.selected_account_id().unwrap(), Some(legacy_id));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migrates_legacy_app_data_accounts_to_wovo_codex_root() {
        let root = temp_root("migration-new");
        let legacy_root = temp_root("migration-legacy");
        let shared = temp_root("migration-shared");
        fs::create_dir_all(shared.join("rules")).unwrap();
        let id = Uuid::new_v4();
        let legacy_home = legacy_root.join("managed-codex-homes").join(id.to_string());
        fs::create_dir_all(&legacy_home).unwrap();
        fs::write(legacy_home.join("auth.json"), "{}").unwrap();
        fs::create_dir_all(legacy_home.join("sessions")).unwrap();
        fs::write(legacy_home.join("sessions").join("session.jsonl"), "{}").unwrap();
        fs::create_dir_all(legacy_home.join("rules")).unwrap();
        fs::write(legacy_home.join("rules").join("legacy.rules"), "legacy").unwrap();

        let payload = ManagedCodexAccountSet {
            version: STORE_VERSION,
            accounts: vec![ManagedCodexAccountRecord {
                id,
                email: Some("user@example.com".to_string()),
                provider_account_id: Some("account-1".to_string()),
                home_path: legacy_home.to_string_lossy().to_string(),
                created_at: 1,
                updated_at: 2,
                last_authenticated_at: Some(3),
            }],
        };
        fs::write(
            legacy_root.join(STORE_FILE_NAME),
            serde_json::to_string_pretty(&payload).unwrap(),
        )
        .unwrap();

        let store = ManagedCodexAccountStore::with_legacy_root(root.clone(), legacy_root.clone())
            .with_shared_codex_home(shared.clone());
        let loaded = store.load_accounts().unwrap();

        assert_eq!(loaded.len(), 1);
        let migrated_home = root.join("accounts").join(id.to_string());
        assert_eq!(
            loaded[0].home_path,
            migrated_home.to_string_lossy().to_string()
        );
        assert!(migrated_home.join("auth.json").exists());
        #[cfg(unix)]
        assert_symlink_to(&migrated_home.join("sessions"), &shared.join("sessions"));
        assert!(shared.join("sessions").join("session.jsonl").exists());
        assert!(legacy_home.join("auth.json").exists());
        assert!(shared.join("rules").join("legacy.rules").exists());
        #[cfg(unix)]
        assert_symlink_to(&migrated_home.join("rules"), &shared.join("rules"));

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(legacy_root);
        let _ = fs::remove_dir_all(shared);
    }
}
