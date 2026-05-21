use crate::claude::account_store::default_wovo_claude_root;
use crate::codex::atomic_file::{replace_file, temporary_file_path, write_new_file};
use crate::domain::usage::ClaudeOverviewSnapshot;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const SNAPSHOT_FILE_NAME: &str = "claude-snapshot.json";
const SNAPSHOT_VERSION: u16 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSnapshot {
    version: u16,
    snapshot: ClaudeOverviewSnapshot,
}

pub fn load_snapshot() -> Option<ClaudeOverviewSnapshot> {
    load_snapshot_from_path(&snapshot_path())
}

pub fn save_snapshot(snapshot: &ClaudeOverviewSnapshot) -> Result<(), AppError> {
    save_snapshot_to_path(&snapshot_path(), snapshot)
}

fn snapshot_path() -> PathBuf {
    default_wovo_claude_root().join(SNAPSHOT_FILE_NAME)
}

fn load_snapshot_from_path(path: &Path) -> Option<ClaudeOverviewSnapshot> {
    let contents = fs::read_to_string(path).ok()?;
    let stored: StoredSnapshot = serde_json::from_str(&contents).ok()?;
    if stored.version != SNAPSHOT_VERSION {
        return None;
    }
    let mut snapshot = stored.snapshot;
    snapshot.errors_by_account_id.clear();
    snapshot.quota_events.clear();
    snapshot.cost_error = None;
    snapshot.stale = true;
    Some(snapshot)
}

fn save_snapshot_to_path(path: &Path, snapshot: &ClaudeOverviewSnapshot) -> Result<(), AppError> {
    let mut snapshot = snapshot.clone();
    snapshot.errors_by_account_id.clear();
    snapshot.quota_events.clear();
    snapshot.cost_error = None;
    snapshot.stale = false;

    let stored = StoredSnapshot {
        version: SNAPSHOT_VERSION,
        snapshot,
    };
    let parent = path.parent().ok_or_else(|| {
        AppError::ClaudeAccountStore(format!(
            "snapshot cache path has no parent: {}",
            path.to_string_lossy()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
    let contents = serde_json::to_vec_pretty(&stored)
        .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
    let tmp = temporary_file_path(parent, SNAPSHOT_FILE_NAME);
    write_new_file(&tmp, &contents)
        .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
    if let Err(error) = apply_secure_file_permissions(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    replace_file(&tmp, path).map_err(|error| AppError::ClaudeAccountStore(error.to_string()))
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
