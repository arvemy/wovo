use crate::auto_switch::auto_switch_candidate;
use crate::claude::account_commands::{
    list_claude_accounts_inner, set_system_claude_account_in_store,
};
use crate::claude::account_store::managed_account_store;
use crate::claude::auth_store::system_claude_home_path;
use crate::claude::settings;
use crate::claude::snapshot_cache;
use crate::claude::usage_commands::refresh_claude_usage_with_mode;
use crate::claude::{cost_usage, quota_events};
use crate::domain::usage::{AccountIssue, ClaudeOverviewSnapshot, CostUsageSnapshot};
use crate::error::AppError;
use crate::notifications::send_claude_notifications;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, State};
use time::OffsetDateTime;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const SNAPSHOT_EVENT: &str = "claude:snapshot-updated";
const REMOTE_USAGE_REFRESH_SECONDS: u64 = 5 * 60;
const COST_USAGE_REFRESH_SECONDS: u64 = 60 * 60;
const COST_USAGE_SCAN_TIMEOUT_SECONDS: u64 = 30;

#[tauri::command]
pub(crate) async fn get_cached_claude_snapshot(
    coordinator: State<'_, Arc<ClaudeSnapshotCoordinator>>,
) -> Result<Option<ClaudeOverviewSnapshot>, AppError> {
    Ok(coordinator.cached_snapshot().await)
}

#[tauri::command]
pub(crate) async fn refresh_claude_snapshot(
    app: AppHandle,
    coordinator: State<'_, Arc<ClaudeSnapshotCoordinator>>,
    force: bool,
) -> Result<ClaudeOverviewSnapshot, AppError> {
    coordinator.refresh_manual(app, force).await
}

#[derive(Default)]
pub(crate) struct ClaudeSnapshotCoordinator {
    latest: Mutex<Option<ClaudeOverviewSnapshot>>,
    latest_generation: Mutex<u64>,
    refresh_lock: Mutex<()>,
    last_cost_refresh_at: Mutex<Option<i64>>,
    cost_scan_running: Arc<AtomicBool>,
}

impl ClaudeSnapshotCoordinator {
    async fn cached_snapshot(&self) -> Option<ClaudeOverviewSnapshot> {
        if let Some(snapshot) = self.latest.lock().await.clone() {
            return Some(snapshot);
        }

        let snapshot = snapshot_cache::load_snapshot();
        if let Some(snapshot) = snapshot.as_ref() {
            *self.latest.lock().await = Some(snapshot.clone());
        }
        snapshot
    }

    pub(crate) async fn refresh_manual(
        &self,
        app: AppHandle,
        force: bool,
    ) -> Result<ClaudeOverviewSnapshot, AppError> {
        let observed_generation = if force {
            None
        } else {
            Some(*self.latest_generation.lock().await)
        };
        let _guard = self.refresh_lock.lock().await;
        if !force {
            if let Some(snapshot) = self.latest.lock().await.clone() {
                if Some(*self.latest_generation.lock().await) != observed_generation {
                    return Ok(snapshot);
                }
            }
        }
        self.refresh_locked(&app, force).await
    }

    async fn refresh_scheduled(&self, app: AppHandle, force_cost: bool) {
        let Ok(_guard) = self.refresh_lock.try_lock() else {
            return;
        };
        let _ = self.refresh_locked(&app, force_cost).await;
    }

    async fn refresh_locked(
        &self,
        app: &AppHandle,
        refresh_cost_now: bool,
    ) -> Result<ClaudeOverviewSnapshot, AppError> {
        let previous = self
            .latest
            .lock()
            .await
            .clone()
            .or_else(snapshot_cache::load_snapshot);
        let settings = settings::load_settings()?;
        let mode = settings.usage_source_mode;
        let mut accounts = list_claude_accounts_inner()?;
        let mut usage_by_account_id = HashMap::new();
        let mut errors_by_account_id = HashMap::new();

        for account in &accounts {
            match refresh_claude_usage_with_mode(account.id.clone(), mode).await {
                Ok(snapshot) => {
                    usage_by_account_id.insert(account.id.clone(), snapshot);
                }
                Err(error) => {
                    if let Some(snapshot) = previous
                        .as_ref()
                        .and_then(|previous| previous.usage_by_account_id.get(&account.id))
                    {
                        usage_by_account_id.insert(account.id.clone(), snapshot.clone());
                    }
                    errors_by_account_id.insert(
                        account.id.clone(),
                        AccountIssue::new(
                            error.code(),
                            error.user_message().into_owned(),
                            error.auth_related(),
                        ),
                    );
                }
            }
        }

        let auto_switch_notification = if settings.auto_account_switching_enabled {
            match auto_switch_candidate(&accounts, &usage_by_account_id, &errors_by_account_id) {
                Some(candidate) => {
                    let latest_settings = settings::load_settings()?;
                    if !latest_settings.auto_account_switching_enabled {
                        None
                    } else {
                        match set_system_claude_account_in_store(
                            &managed_account_store(),
                            &candidate.target_account_id,
                            &system_claude_home_path(),
                        ) {
                            Ok(_) => {
                                accounts = list_claude_accounts_inner()?;
                                Some(candidate.notification)
                            }
                            Err(error) => {
                                errors_by_account_id.insert(
                                    candidate.current_account_id,
                                    AccountIssue::new(
                                        "claude_auto_switch_failed",
                                        "Claude auto switch failed.",
                                        error.auth_related(),
                                    ),
                                );
                                None
                            }
                        }
                    }
                }
                None => None,
            }
        } else {
            None
        };

        let (cost_usage, cost_error) = self
            .refresh_cost_usage_if_needed(
                settings.cost_usage_enabled,
                refresh_cost_now,
                previous
                    .as_ref()
                    .and_then(|snapshot| snapshot.cost_usage.clone()),
            )
            .await;

        let generated_at = OffsetDateTime::now_utc().unix_timestamp();
        let mut snapshot = ClaudeOverviewSnapshot {
            accounts,
            usage_by_account_id,
            errors_by_account_id,
            quota_events: Vec::new(),
            cost_usage,
            cost_error,
            generated_at,
            stale: false,
        };
        snapshot.quota_events = quota_events::detect_quota_events(previous.as_ref(), &snapshot);

        self.store_and_emit(app, snapshot.clone()).await;
        send_claude_notifications(
            app,
            &snapshot.quota_events,
            auto_switch_notification.as_ref(),
            settings.notifications_enabled,
            settings.hide_account_credentials,
        );
        let _ = snapshot_cache::save_snapshot(&snapshot);
        Ok(snapshot)
    }

    async fn refresh_cost_usage_if_needed(
        &self,
        enabled: bool,
        refresh_now: bool,
        previous: Option<CostUsageSnapshot>,
    ) -> (Option<CostUsageSnapshot>, Option<String>) {
        if !enabled {
            *self.last_cost_refresh_at.lock().await = None;
            return (None, None);
        }

        let now = OffsetDateTime::now_utc().unix_timestamp();
        let previous_updated_at = previous.as_ref().map(|snapshot| snapshot.updated_at);
        let stored_last_refresh_at = *self.last_cost_refresh_at.lock().await;
        let last_refresh_at = stored_last_refresh_at.or(previous_updated_at);
        let due = last_refresh_at
            .map(|timestamp| now.saturating_sub(timestamp) >= COST_USAGE_REFRESH_SECONDS as i64)
            .unwrap_or(true);

        if !refresh_now && !due {
            return (previous, None);
        }

        let source_root = system_claude_home_path();
        let scan_running = self.cost_scan_running.clone();
        if scan_running.swap(true, Ordering::AcqRel) {
            return (
                previous,
                Some("Claude cost usage scan is still running.".to_string()),
            );
        }
        let result = tokio::time::timeout(
            Duration::from_secs(COST_USAGE_SCAN_TIMEOUT_SECONDS),
            tokio::task::spawn_blocking(move || {
                let _guard = CostScanGuard(scan_running);
                cost_usage::load_cost_usage_snapshot(source_root, false)
            }),
        )
        .await
        .map_err(|_| AppError::ClaudeAccountStore("cost usage scan timed out".to_string()))
        .and_then(|join_result| {
            join_result.map_err(|error| AppError::ClaudeAccountStore(error.to_string()))
        })
        .and_then(|result| result);

        match result {
            Ok(snapshot) => {
                *self.last_cost_refresh_at.lock().await = Some(snapshot.updated_at);
                (Some(snapshot), None)
            }
            Err(error) => (previous, Some(error.to_string())),
        }
    }

    async fn store_and_emit(&self, app: &AppHandle, snapshot: ClaudeOverviewSnapshot) {
        *self.latest.lock().await = Some(snapshot.clone());
        *self.latest_generation.lock().await += 1;
        let _ = app.emit(SNAPSHOT_EVENT, snapshot);
    }
}

struct CostScanGuard(Arc<AtomicBool>);

impl Drop for CostScanGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(crate) fn start_claude_snapshot_tasks(
    app: AppHandle,
    coordinator: Arc<ClaudeSnapshotCoordinator>,
    cancellation_token: CancellationToken,
) -> Vec<JoinHandle<()>> {
    let mut handles = Vec::new();
    let initial_app = app.clone();
    let initial_coordinator = coordinator.clone();
    let initial_token = cancellation_token.clone();
    handles.push(tauri::async_runtime::spawn(async move {
        if let Some(snapshot) = snapshot_cache::load_snapshot() {
            initial_coordinator
                .store_and_emit(&initial_app, snapshot)
                .await;
        }
        if initial_token.is_cancelled() {
            return;
        }
        initial_coordinator
            .refresh_scheduled(initial_app.clone(), true)
            .await;
    }));

    let remote_app = app.clone();
    let remote_coordinator = coordinator.clone();
    let remote_token = cancellation_token.clone();
    handles.push(tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = remote_token.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(REMOTE_USAGE_REFRESH_SECONDS)) => {}
            }
            remote_coordinator
                .refresh_scheduled(remote_app.clone(), false)
                .await;
        }
    }));

    let cost_token = cancellation_token;
    handles.push(tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = cost_token.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(COST_USAGE_REFRESH_SECONDS)) => {}
            }
            coordinator.refresh_scheduled(app.clone(), true).await;
        }
    }));

    handles
}
