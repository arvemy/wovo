use crate::codex::account_store::default_wovo_codex_root;
use crate::codex::atomic_file::{replace_file, temporary_file_path, write_new_file};
use crate::domain::usage::CodexOverviewSnapshot;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const SNAPSHOT_FILE_NAME: &str = "codex-snapshot.json";
const SNAPSHOT_VERSION: u16 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSnapshot {
    version: u16,
    snapshot: CodexOverviewSnapshot,
}

pub fn load_snapshot() -> Option<CodexOverviewSnapshot> {
    load_snapshot_from_path(&snapshot_path())
}

pub fn save_snapshot(snapshot: &CodexOverviewSnapshot) -> Result<(), AppError> {
    save_snapshot_to_path(&snapshot_path(), snapshot)
}

fn snapshot_path() -> PathBuf {
    default_wovo_codex_root().join(SNAPSHOT_FILE_NAME)
}

fn load_snapshot_from_path(path: &Path) -> Option<CodexOverviewSnapshot> {
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

fn save_snapshot_to_path(path: &Path, snapshot: &CodexOverviewSnapshot) -> Result<(), AppError> {
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
        AppError::AccountStore(format!(
            "snapshot cache path has no parent: {}",
            path.to_string_lossy()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| AppError::AccountStore(error.to_string()))?;
    let contents = serde_json::to_vec_pretty(&stored)
        .map_err(|error| AppError::AccountStore(error.to_string()))?;
    let tmp = temporary_file_path(parent, SNAPSHOT_FILE_NAME);
    write_new_file(&tmp, &contents).map_err(|error| AppError::AccountStore(error.to_string()))?;
    if let Err(error) = apply_secure_file_permissions(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    replace_file(&tmp, path).map_err(|error| AppError::AccountStore(error.to_string()))?;
    Ok(())
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
    use crate::domain::usage::{
        AccountIssue, CostUsageSnapshot, QuotaEvent, QuotaEventKind, QuotaEventSeverity,
    };
    use std::collections::HashMap;
    use uuid::Uuid;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("wovo-snapshot-{name}-{}", Uuid::new_v4()))
            .join(SNAPSHOT_FILE_NAME)
    }

    fn snapshot() -> CodexOverviewSnapshot {
        let mut errors = HashMap::new();
        errors.insert(
            "account-1".to_string(),
            AccountIssue::new("temporary_error", "Temporary error.", false),
        );
        CodexOverviewSnapshot {
            accounts: Vec::new(),
            usage_by_account_id: HashMap::new(),
            errors_by_account_id: errors,
            quota_events: vec![QuotaEvent {
                id: "event-1".to_string(),
                kind: QuotaEventKind::Warning,
                severity: QuotaEventSeverity::Warning,
                account_id: "account-1".to_string(),
                account_label: "user@example.com".to_string(),
                window_key: "primary".to_string(),
                window_label: "5h limit".to_string(),
                used_percent: 90.0,
                threshold_percent: Some(90.0),
                title: "Codex quota at 90%".to_string(),
                body: "user@example.com: 5h limit is 90% used.".to_string(),
                generated_at: 2,
            }],
            cost_usage: Some(CostUsageSnapshot {
                today_tokens: 10,
                today_cost_usd: Some(0.1),
                last_30_days_tokens: 10,
                last_30_days_cost_usd: Some(0.1),
                daily: Vec::new(),
                updated_at: 1,
                source_root: "/tmp/codex".to_string(),
            }),
            cost_error: Some("temporary cost error".to_string()),
            generated_at: 2,
            stale: false,
        }
    }

    #[test]
    fn missing_or_corrupt_snapshot_is_ignored() {
        let path = temp_path("missing");
        assert!(load_snapshot_from_path(&path).is_none());

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not json").unwrap();
        assert!(load_snapshot_from_path(&path).is_none());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn snapshot_round_trips_without_transient_errors() {
        let path = temp_path("round-trip");
        save_snapshot_to_path(&path, &snapshot()).unwrap();

        let loaded = load_snapshot_from_path(&path).unwrap();

        assert!(loaded.errors_by_account_id.is_empty());
        assert!(loaded.quota_events.is_empty());
        assert!(loaded.cost_error.is_none());
        assert!(loaded.stale);
        assert_eq!(loaded.cost_usage.unwrap().today_tokens, 10);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn old_snapshot_json_without_quota_events_still_loads() {
        let path = temp_path("old-without-quota-events");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{
                "version": 1,
                "snapshot": {
                    "accounts": [],
                    "usageByAccountId": {},
                    "errorsByAccountId": {},
                    "costUsage": null,
                    "costError": null,
                    "generatedAt": 2,
                    "stale": false
                }
            }"#,
        )
        .unwrap();

        let loaded = load_snapshot_from_path(&path).unwrap();

        assert!(loaded.quota_events.is_empty());
        assert!(loaded.stale);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
