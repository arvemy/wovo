use crate::account_commands::{
    list_codex_accounts, managed_account_store, set_system_codex_account_in_store,
};
use crate::auto_switch::{auto_switch_candidate_with_policy, AutoSwitchPolicy};
use crate::codex::auth_store::system_codex_home_path;
use crate::codex::settings;
use crate::codex::snapshot_cache;
use crate::codex::{cost_usage, quota_events};
use crate::domain::usage::{AccountIssue, CodexOverviewSnapshot, CostUsageSnapshot};
use crate::error::AppError;
use crate::notifications::send_codex_notifications;
use crate::provider::{ProviderId, ProviderSourceMode};
use crate::provider_state;
use crate::usage_commands::refresh_codex_usage_with_diagnostics;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, State};
use time::OffsetDateTime;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const SNAPSHOT_EVENT: &str = "codex:snapshot-updated";
const REMOTE_USAGE_REFRESH_SECONDS: u64 = 5 * 60;
const COST_USAGE_REFRESH_SECONDS: u64 = 60 * 60;
const COST_USAGE_SCAN_TIMEOUT_SECONDS: u64 = 30;

#[tauri::command]
pub(crate) async fn get_cached_codex_snapshot(
    coordinator: State<'_, Arc<CodexSnapshotCoordinator>>,
) -> Result<Option<CodexOverviewSnapshot>, AppError> {
    Ok(coordinator.cached_snapshot().await)
}

#[tauri::command]
pub(crate) async fn refresh_codex_snapshot(
    app: AppHandle,
    coordinator: State<'_, Arc<CodexSnapshotCoordinator>>,
    force: bool,
) -> Result<CodexOverviewSnapshot, AppError> {
    coordinator.refresh_manual(app, force).await
}

#[derive(Default)]
pub(crate) struct CodexSnapshotCoordinator {
    latest: Mutex<Option<CodexOverviewSnapshot>>,
    latest_generation: Mutex<u64>,
    refresh_lock: Mutex<()>,
    last_cost_refresh_at: Mutex<Option<i64>>,
    cost_scan_running: Arc<AtomicBool>,
}

impl CodexSnapshotCoordinator {
    async fn cached_snapshot(&self) -> Option<CodexOverviewSnapshot> {
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
    ) -> Result<CodexOverviewSnapshot, AppError> {
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
    ) -> Result<CodexOverviewSnapshot, AppError> {
        let previous = self
            .latest
            .lock()
            .await
            .clone()
            .or_else(snapshot_cache::load_snapshot);
        let settings = settings::load_settings()?;
        let mode = settings.usage_source_mode;
        let mut accounts = list_codex_accounts(app.clone()).await?;
        let mut usage_by_account_id = HashMap::new();
        let mut errors_by_account_id = HashMap::new();
        let mut diagnostics_by_account_id = HashMap::new();

        for account in &accounts {
            match refresh_codex_usage_with_diagnostics(app, account.id.clone(), mode).await {
                Ok(result) => {
                    diagnostics_by_account_id.insert(account.id.clone(), result.diagnostics);
                    usage_by_account_id.insert(account.id.clone(), result.snapshot);
                }
                Err((error, diagnostics)) => {
                    let serving_cached = if let Some(snapshot) = previous
                        .as_ref()
                        .and_then(|previous| previous.usage_by_account_id.get(&account.id))
                    {
                        let mut cached = snapshot.clone();
                        cached.source = "cached".to_string();
                        cached.source_mode = Some(ProviderSourceMode::Cached);
                        cached.fetch_attempts = diagnostics.attempts.clone();
                        usage_by_account_id.insert(account.id.clone(), cached);
                        true
                    } else {
                        false
                    };
                    diagnostics_by_account_id.insert(
                        account.id.clone(),
                        if serving_cached {
                            diagnostics.mark_cached("refresh_failed")
                        } else {
                            diagnostics
                        },
                    );
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

        let auto_switch_candidate = auto_switch_candidate_with_policy(
            &accounts,
            &usage_by_account_id,
            &errors_by_account_id,
            AutoSwitchPolicy {
                auto_switch_threshold_percent: settings.auto_switch_threshold_percent,
                weekly_penalty_threshold_percent: settings.weekly_penalty_threshold_percent,
            },
        );
        if let Some(candidate) = auto_switch_candidate.as_ref() {
            let preview = format!(
                "{} limit would switch {} to {} at {:.0}%",
                candidate.notification.window_label,
                candidate.notification.current_account_label,
                candidate.notification.target_account_label,
                candidate.notification.threshold_percent
            );
            diagnostics_by_account_id
                .entry(candidate.current_account_id.clone())
                .or_default()
                .auto_switch_preview = Some(preview.clone());
            diagnostics_by_account_id
                .entry(candidate.target_account_id.clone())
                .or_default()
                .auto_switch_preview = Some(preview);
        }

        let auto_switch_notification = if settings.auto_account_switching_enabled {
            match auto_switch_candidate {
                Some(candidate) => {
                    let latest_settings = settings::load_settings()?;
                    let switch_allowed = latest_settings.auto_account_switching_enabled
                        && provider_state::auto_switch_is_allowed(
                            ProviderId::Codex,
                            &candidate.current_account_id,
                            &candidate.target_account_id,
                            OffsetDateTime::now_utc().unix_timestamp(),
                        )?;
                    if !switch_allowed {
                        None
                    } else {
                        match set_system_codex_account_in_store(
                            &managed_account_store(app)?,
                            &candidate.target_account_id,
                            &system_codex_home_path(),
                        ) {
                            Ok(_) => {
                                // Sidecar bookkeeping and re-listing run after
                                // the live account has already changed; failures
                                // here must not suppress the snapshot/notification
                                // that tells the user the switch happened.
                                let now = OffsetDateTime::now_utc().unix_timestamp();
                                if let Err(error) = provider_state::record_auto_switch(
                                    ProviderId::Codex,
                                    candidate.current_account_id.clone(),
                                    candidate.target_account_id.clone(),
                                    candidate.window_key.clone(),
                                    candidate.trigger_reset_at,
                                    now,
                                ) {
                                    eprintln!(
                                        "Failed to record codex auto-switch in provider state: {error}"
                                    );
                                }
                                match list_codex_accounts(app.clone()).await {
                                    Ok(refreshed) => accounts = refreshed,
                                    Err(error) => eprintln!(
                                        "Failed to re-list codex accounts after auto-switch: {error}"
                                    ),
                                }
                                Some(candidate.notification)
                            }
                            Err(error) => {
                                errors_by_account_id.insert(
                                    candidate.current_account_id,
                                    AccountIssue::new(
                                        "auto_switch_failed",
                                        "Auto switch failed.",
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
                settings.cost_usage_range_days,
                previous
                    .as_ref()
                    .and_then(|snapshot| snapshot.cost_usage.clone()),
            )
            .await;

        let generated_at = OffsetDateTime::now_utc().unix_timestamp();
        let last_successful_at = usage_by_account_id
            .values()
            .filter(|usage| usage.source != "cached")
            .map(|usage| usage.updated_at)
            .max()
            .or_else(|| {
                previous
                    .as_ref()
                    .and_then(|snapshot| snapshot.last_successful_at)
            });
        let mut snapshot = CodexOverviewSnapshot {
            accounts,
            usage_by_account_id,
            errors_by_account_id,
            diagnostics_by_account_id,
            stale_reason: None,
            last_successful_at,
            last_attempt_at: Some(generated_at),
            quota_events: Vec::new(),
            cost_usage,
            cost_error,
            generated_at,
            stale: false,
        };
        let events = quota_events::detect_quota_events(previous.as_ref(), &snapshot);
        snapshot.quota_events =
            provider_state::dedupe_quota_events(ProviderId::Codex, events, generated_at)?;

        self.store_and_emit(app, snapshot.clone()).await;
        send_codex_notifications(
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
        range_days: u16,
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
        let range_changed = previous
            .as_ref()
            .map(|snapshot| snapshot.range_days != range_days)
            .unwrap_or(false);

        if !refresh_now && !due && !range_changed {
            return (previous, None);
        }

        let source_root = system_codex_home_path();
        let scan_running = self.cost_scan_running.clone();
        if scan_running.swap(true, Ordering::AcqRel) {
            return (
                previous,
                Some("Cost usage scan is still running.".to_string()),
            );
        }
        let result = tokio::time::timeout(
            Duration::from_secs(COST_USAGE_SCAN_TIMEOUT_SECONDS),
            tokio::task::spawn_blocking(move || {
                let _guard = CostScanGuard(scan_running);
                cost_usage::load_cost_usage_snapshot_with_range(source_root, false, range_days)
            }),
        )
        .await
        .map_err(|_| AppError::AccountStore("cost usage scan timed out".to_string()))
        .and_then(|join_result| {
            join_result.map_err(|error| AppError::AccountStore(error.to_string()))
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

    async fn store_and_emit(&self, app: &AppHandle, snapshot: CodexOverviewSnapshot) {
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

pub(crate) struct SnapshotTaskSupervisor {
    cancellation_token: CancellationToken,
    handles: Vec<JoinHandle<()>>,
}

impl SnapshotTaskSupervisor {
    pub(crate) fn add_handles(&mut self, handles: Vec<JoinHandle<()>>) {
        self.handles.extend(handles);
    }

    pub(crate) fn shutdown(self, timeout: Duration) {
        self.cancellation_token.cancel();
        let completed =
            tauri::async_runtime::block_on(wait_for_snapshot_tasks(self.handles, timeout));
        if !completed {
            eprintln!("snapshot tasks did not stop before shutdown timeout");
        }
    }
}

pub(crate) fn start_codex_snapshot_tasks(
    app: AppHandle,
    coordinator: Arc<CodexSnapshotCoordinator>,
    cancellation_token: CancellationToken,
) -> SnapshotTaskSupervisor {
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

    let cost_token = cancellation_token.clone();
    handles.push(tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = cost_token.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(COST_USAGE_REFRESH_SECONDS)) => {}
            }
            coordinator.refresh_scheduled(app.clone(), true).await;
        }
    }));

    SnapshotTaskSupervisor {
        cancellation_token,
        handles,
    }
}

async fn wait_for_snapshot_tasks(mut handles: Vec<JoinHandle<()>>, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut completed = 0;

    while completed < handles.len() {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            break;
        }

        let remaining = deadline - now;
        match tokio::time::timeout(remaining, &mut handles[completed]).await {
            Ok(_) => completed += 1,
            Err(_) => break,
        }
    }

    let all_completed = completed == handles.len();
    if !all_completed {
        for handle in handles.iter().skip(completed) {
            handle.abort();
        }
    }
    all_completed
}

#[cfg(test)]
mod task_supervisor_tests {
    use super::*;

    #[test]
    fn wait_for_snapshot_tasks_reports_completed_tasks() {
        let handle = tauri::async_runtime::spawn(async {});

        let completed = tauri::async_runtime::block_on(wait_for_snapshot_tasks(
            vec![handle],
            Duration::from_secs(1),
        ));

        assert!(completed);
    }

    #[test]
    fn wait_for_snapshot_tasks_aborts_after_timeout() {
        let handle = tauri::async_runtime::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
        });

        let completed = tauri::async_runtime::block_on(wait_for_snapshot_tasks(
            vec![handle],
            Duration::from_millis(5),
        ));

        assert!(!completed);
    }
}
