use crate::codex::atomic_file::{replace_file, temporary_file_path, write_new_file};
use crate::domain::usage::{QuotaEvent, QuotaEventKind};
use crate::error::AppError;
use crate::provider::ProviderId;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const PROVIDER_STATE_FILE_NAME: &str = "provider-state.json";
const PROVIDER_STATE_VERSION: u16 = 1;
const PRIMARY_FALLBACK_COOLDOWN_SECONDS: i64 = 5 * 60 * 60;
const WEEKLY_FALLBACK_COOLDOWN_SECONDS: i64 = 7 * 24 * 60 * 60;
const WARNING_DEDUPE_NULL_RESET_TTL_SECONDS: i64 = 14 * 24 * 60 * 60;
const PROVIDER_STATE_MAX_ENTRIES: usize = 500;

static PROVIDER_STATE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderState {
    pub version: u16,
    #[serde(default)]
    pub warning_dedupe: Vec<WarningDedupeRecord>,
    #[serde(default)]
    pub auto_switch_history: Vec<AutoSwitchHistoryRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WarningDedupeRecord {
    pub account_id: String,
    pub window_key: String,
    pub threshold_percent: i64,
    pub reset_at: Option<i64>,
    pub fired_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AutoSwitchHistoryRecord {
    pub from_account_id: String,
    pub to_account_id: String,
    pub window_key: String,
    pub reset_at: Option<i64>,
    pub switched_at: i64,
    pub unlock_at: i64,
}

impl ProviderState {
    fn new() -> Self {
        Self {
            version: PROVIDER_STATE_VERSION,
            warning_dedupe: Vec::new(),
            auto_switch_history: Vec::new(),
        }
    }
}

impl Default for ProviderState {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn dedupe_quota_events(
    provider_id: ProviderId,
    events: Vec<QuotaEvent>,
    now: i64,
) -> Result<Vec<QuotaEvent>, AppError> {
    // The common refresh has no events — skip touching provider-state.json so a
    // read-only/full sidecar cannot block snapshot delivery. Pruning is just
    // cleanup; the next event-bearing call will catch up.
    if events.is_empty() {
        return Ok(events);
    }
    update_state(provider_id, |state| {
        prune_provider_state(state, now);
        let mut retained = Vec::new();
        for event in events {
            match event.kind {
                QuotaEventKind::Warning => {
                    if record_warning_if_new(state, &event, now) {
                        retained.push(event);
                    }
                }
                QuotaEventKind::Reset => {
                    clear_warning_records_for_reset(state, &event);
                    retained.push(event);
                }
            }
        }
        retained
    })
}

pub(crate) fn auto_switch_is_allowed(
    provider_id: ProviderId,
    from_account_id: &str,
    to_account_id: &str,
    now: i64,
) -> Result<bool, AppError> {
    let state = load_state(provider_id)?;
    Ok(!state.auto_switch_history.iter().any(|record| {
        record.from_account_id == to_account_id
            && record.to_account_id == from_account_id
            && now < record.unlock_at
    }))
}

pub(crate) fn record_auto_switch(
    provider_id: ProviderId,
    from_account_id: String,
    to_account_id: String,
    window_key: String,
    reset_at: Option<i64>,
    now: i64,
) -> Result<(), AppError> {
    update_state(provider_id, |state| {
        prune_provider_state(state, now);
        let fallback = if window_key == "primary" {
            PRIMARY_FALLBACK_COOLDOWN_SECONDS
        } else {
            WEEKLY_FALLBACK_COOLDOWN_SECONDS
        };
        // unlock_at matches reset_at on purpose: once the triggering quota
        // resets, an automatic switch back to the displaced account is
        // legitimate again, so the ping-pong block lifts at the same instant.
        let unlock_at = reset_at
            .filter(|reset_at| *reset_at > now)
            .unwrap_or(now + fallback);
        state.auto_switch_history.push(AutoSwitchHistoryRecord {
            from_account_id,
            to_account_id,
            window_key,
            reset_at,
            switched_at: now,
            unlock_at,
        });
        cap_provider_state(state);
    })
    .map(|_| ())
}

pub(crate) fn purge_account_state(
    provider_id: ProviderId,
    account_id: &str,
) -> Result<(), AppError> {
    update_state(provider_id, |state| {
        state
            .warning_dedupe
            .retain(|record| record.account_id != account_id);
        state.auto_switch_history.retain(|record| {
            record.from_account_id != account_id && record.to_account_id != account_id
        });
    })
    .map(|_| ())
}

fn record_warning_if_new(state: &mut ProviderState, event: &QuotaEvent, now: i64) -> bool {
    let threshold_percent = event.threshold_percent.unwrap_or_default().round() as i64;
    if let Some(record) = state.warning_dedupe.iter_mut().find(|record| {
        record.account_id == event.account_id
            && record.window_key == event.window_key
            && record.threshold_percent == threshold_percent
    }) {
        if record.reset_at.is_none() && event.reset_at.is_some() {
            record.reset_at = event.reset_at;
        }
        return false;
    }

    state.warning_dedupe.push(WarningDedupeRecord {
        account_id: event.account_id.clone(),
        window_key: event.window_key.clone(),
        threshold_percent,
        reset_at: event.reset_at,
        fired_at: now,
    });
    true
}

fn clear_warning_records_for_reset(state: &mut ProviderState, event: &QuotaEvent) {
    state.warning_dedupe.retain(|record| {
        !(record.account_id == event.account_id && record.window_key == event.window_key)
    });
}

fn prune_provider_state(state: &mut ProviderState, now: i64) {
    state.warning_dedupe.retain(|record| match record.reset_at {
        Some(reset_at) => reset_at > now,
        None => now - record.fired_at < WARNING_DEDUPE_NULL_RESET_TTL_SECONDS,
    });
    state
        .auto_switch_history
        .retain(|record| record.unlock_at > now);
    cap_provider_state(state);
}

fn cap_provider_state(state: &mut ProviderState) {
    if state.warning_dedupe.len() > PROVIDER_STATE_MAX_ENTRIES {
        state.warning_dedupe.sort_by_key(|record| record.fired_at);
        let overflow = state.warning_dedupe.len() - PROVIDER_STATE_MAX_ENTRIES;
        state.warning_dedupe.drain(..overflow);
    }
    if state.auto_switch_history.len() > PROVIDER_STATE_MAX_ENTRIES {
        state
            .auto_switch_history
            .sort_by_key(|record| record.switched_at);
        let overflow = state.auto_switch_history.len() - PROVIDER_STATE_MAX_ENTRIES;
        state.auto_switch_history.drain(..overflow);
    }
}

fn update_state<T>(
    provider_id: ProviderId,
    update: impl FnOnce(&mut ProviderState) -> T,
) -> Result<T, AppError> {
    let _guard = provider_state_lock()
        .lock()
        .map_err(|_| AppError::AccountStore("provider state lock was poisoned".to_string()))?;
    let path = provider_state_path(provider_id);
    let mut state = load_state_from_path(&path)?;
    let result = update(&mut state);
    save_state_to_path(provider_id, &path, &state)?;
    Ok(result)
}

fn load_state(provider_id: ProviderId) -> Result<ProviderState, AppError> {
    load_state_from_path(&provider_state_path(provider_id))
}

fn load_state_from_path(path: &Path) -> Result<ProviderState, AppError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(ProviderState::new()),
        Err(error) => return Err(AppError::AccountStore(error.to_string())),
    };
    match serde_json::from_str::<ProviderState>(&contents) {
        Ok(mut state) => {
            if state.version != PROVIDER_STATE_VERSION {
                state.version = PROVIDER_STATE_VERSION;
            }
            Ok(state)
        }
        Err(error) => {
            // Corrupt provider-state.json only loses dedupe + ping-pong memory;
            // a warning may re-fire once, but the snapshot refresh must not be
            // broken by it. Back up the bad file and start fresh, like settings.
            let _ = backup_corrupt_provider_state(path);
            eprintln!(
                "provider state at {} was corrupt and has been reset to defaults: {error}",
                path.to_string_lossy()
            );
            Ok(ProviderState::new())
        }
    }
}

fn backup_corrupt_provider_state(path: &Path) -> Result<(), AppError> {
    if !path.exists() {
        return Ok(());
    }
    let timestamp = time::OffsetDateTime::now_utc().unix_timestamp();
    let backup = path.with_extension(format!("json.bad-{timestamp}"));
    fs::copy(path, backup)
        .map(|_| ())
        .map_err(|error| AppError::AccountStore(error.to_string()))
}

fn save_state_to_path(
    provider_id: ProviderId,
    path: &Path,
    state: &ProviderState,
) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(|| {
        AppError::AccountStore(format!(
            "provider state path has no parent: {}",
            path.to_string_lossy()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| state_error(provider_id, error.to_string()))?;
    let contents = serde_json::to_vec_pretty(state)
        .map_err(|error| state_error(provider_id, error.to_string()))?;
    let tmp = temporary_file_path(parent, PROVIDER_STATE_FILE_NAME);
    write_new_file(&tmp, &contents).map_err(|error| state_error(provider_id, error.to_string()))?;
    if let Err(error) = apply_secure_file_permissions(&tmp, provider_id) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    replace_file(&tmp, path).map_err(|error| state_error(provider_id, error.to_string()))
}

fn provider_state_path(provider_id: ProviderId) -> PathBuf {
    dirs_home()
        .join(".wovo")
        .join(provider_id.root_dir_name())
        .join(PROVIDER_STATE_FILE_NAME)
}

fn provider_state_lock() -> &'static Mutex<()> {
    PROVIDER_STATE_LOCK.get_or_init(|| Mutex::new(()))
}

fn state_error(provider_id: ProviderId, message: String) -> AppError {
    match provider_id {
        ProviderId::Codex => AppError::AccountStore(message),
        ProviderId::Claude => AppError::ClaudeAccountStore(message),
    }
}

fn dirs_home() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
        })
}

#[cfg(unix)]
fn apply_secure_file_permissions(path: &Path, provider_id: ProviderId) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions)
        .map_err(|error| state_error(provider_id, error.to_string()))
}

#[cfg(not(unix))]
fn apply_secure_file_permissions(_path: &Path, _provider_id: ProviderId) -> Result<(), AppError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::usage::{QuotaEventKind, QuotaEventSeverity};
    use uuid::Uuid;

    fn warning(reset_at: Option<i64>) -> QuotaEvent {
        QuotaEvent {
            id: Uuid::new_v4().to_string(),
            kind: QuotaEventKind::Warning,
            severity: QuotaEventSeverity::Warning,
            account_id: "account-1".to_string(),
            account_label: "user@example.com".to_string(),
            window_key: "primary".to_string(),
            window_label: "5h limit".to_string(),
            used_percent: 90.0,
            threshold_percent: Some(90.0),
            reset_at,
            title: "Codex quota at 90%".to_string(),
            body: "body".to_string(),
            generated_at: 10,
        }
    }

    #[test]
    fn warning_dedupe_normalizes_none_reset_to_concrete_reset_without_refiring() {
        let mut state = ProviderState::new();
        assert!(record_warning_if_new(&mut state, &warning(None), 10));
        assert!(!record_warning_if_new(&mut state, &warning(Some(20)), 11));
        assert_eq!(state.warning_dedupe[0].reset_at, Some(20));
    }

    #[test]
    fn reverse_auto_switch_is_blocked_until_unlock() {
        let mut state = ProviderState::new();
        let now = 10;
        state.auto_switch_history.push(AutoSwitchHistoryRecord {
            from_account_id: "a".to_string(),
            to_account_id: "b".to_string(),
            window_key: "primary".to_string(),
            reset_at: Some(20),
            switched_at: now,
            unlock_at: 20,
        });

        assert!(state.auto_switch_history.iter().any(|record| {
            record.from_account_id == "a" && record.to_account_id == "b" && 19 < record.unlock_at
        }));
        prune_provider_state(&mut state, 21);
        assert!(state.auto_switch_history.is_empty());
    }

    #[test]
    fn warning_dedupe_with_no_reset_expires_after_ttl() {
        let mut state = ProviderState::new();
        let fired_at = 1_000_000;
        assert!(record_warning_if_new(&mut state, &warning(None), fired_at));
        assert_eq!(state.warning_dedupe.len(), 1);

        prune_provider_state(
            &mut state,
            fired_at + WARNING_DEDUPE_NULL_RESET_TTL_SECONDS - 1,
        );
        assert_eq!(state.warning_dedupe.len(), 1);

        prune_provider_state(&mut state, fired_at + WARNING_DEDUPE_NULL_RESET_TTL_SECONDS);
        assert!(state.warning_dedupe.is_empty());
    }

    #[test]
    fn corrupt_provider_state_is_backed_up_and_reset_to_defaults() {
        let dir = std::env::temp_dir().join(format!("wovo-state-recovery-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("provider-state.json");
        fs::write(&path, b"{not valid json").unwrap();

        let state = load_state_from_path(&path).unwrap();

        assert_eq!(state.version, PROVIDER_STATE_VERSION);
        assert!(state.warning_dedupe.is_empty());
        assert!(state.auto_switch_history.is_empty());
        let backup_present = fs::read_dir(&dir).unwrap().any(|entry| {
            entry
                .map(|entry| {
                    entry
                        .path()
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(|ext| ext.starts_with("bad-"))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        });
        assert!(backup_present, "expected a .bad-<ts> backup file");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn cap_drops_oldest_entries_when_over_max() {
        let mut state = ProviderState::new();
        for index in 0..(PROVIDER_STATE_MAX_ENTRIES + 5) {
            state.warning_dedupe.push(WarningDedupeRecord {
                account_id: format!("account-{index}"),
                window_key: "primary".to_string(),
                threshold_percent: 90,
                reset_at: Some(i64::try_from(index).unwrap_or(i64::MAX) + 1_000_000),
                fired_at: i64::try_from(index).unwrap_or(i64::MAX),
            });
            state.auto_switch_history.push(AutoSwitchHistoryRecord {
                from_account_id: "a".to_string(),
                to_account_id: "b".to_string(),
                window_key: "primary".to_string(),
                reset_at: Some(i64::try_from(index).unwrap_or(i64::MAX) + 1_000_000),
                switched_at: i64::try_from(index).unwrap_or(i64::MAX),
                unlock_at: i64::try_from(index).unwrap_or(i64::MAX) + 1_000_000,
            });
        }

        cap_provider_state(&mut state);

        assert_eq!(state.warning_dedupe.len(), PROVIDER_STATE_MAX_ENTRIES);
        assert_eq!(state.warning_dedupe[0].account_id, "account-5");
        assert_eq!(state.auto_switch_history.len(), PROVIDER_STATE_MAX_ENTRIES);
        assert_eq!(state.auto_switch_history[0].switched_at, 5);
    }
}
