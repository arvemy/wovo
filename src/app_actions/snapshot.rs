use std::collections::{HashMap, HashSet};

use crate::codex_api::{
    invoke_tauri, refresh_snapshot, AccountIssue, AccountSummary, CodexOverviewSnapshot,
    CostUsageSnapshot, QuotaEvent, UsageSnapshot,
};
use crate::request_epoch::RequestEpoch;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;

#[derive(Clone, Copy)]
pub(crate) struct SnapshotActions {
    pub(crate) accounts: ReadSignal<Vec<AccountSummary>>,
    pub(crate) set_accounts: WriteSignal<Vec<AccountSummary>>,
    pub(crate) set_usage_by_id: WriteSignal<HashMap<String, UsageSnapshot>>,
    pub(crate) set_errors_by_id: WriteSignal<HashMap<String, AccountIssue>>,
    pub(crate) set_quota_events: WriteSignal<Vec<QuotaEvent>>,
    pub(crate) set_dismissed_quota_event_ids: WriteSignal<HashSet<String>>,
    pub(crate) set_loading_ids: WriteSignal<HashSet<String>>,
    pub(crate) set_reauth_ids: WriteSignal<HashSet<String>>,
    pub(crate) set_cost_usage: WriteSignal<Option<CostUsageSnapshot>>,
    pub(crate) set_cost_error: WriteSignal<Option<String>>,
    pub(crate) snapshot_generated_at: ReadSignal<Option<i64>>,
    pub(crate) set_snapshot_generated_at: WriteSignal<Option<i64>>,
    pub(crate) set_snapshot_stale: WriteSignal<bool>,
    pub(crate) set_is_listing: WriteSignal<bool>,
    pub(crate) set_global_error: WriteSignal<Option<String>>,
    pub(crate) snapshot_epoch: RequestEpoch,
}

impl SnapshotActions {
    pub(crate) fn apply_snapshot(&self, snapshot: CodexOverviewSnapshot) {
        if self
            .snapshot_generated_at
            .get_untracked()
            .is_some_and(|current| current > snapshot.generated_at)
        {
            return;
        }
        let next_ids: HashSet<String> = snapshot
            .accounts
            .iter()
            .map(|account| account.id.clone())
            .collect();
        let quota_event_ids: HashSet<String> = snapshot
            .quota_events
            .iter()
            .map(|event| event.id.clone())
            .collect();
        let reauth_ids: HashSet<String> = snapshot
            .errors_by_account_id
            .iter()
            .filter(|(_, issue)| issue.auth_related)
            .map(|(id, _)| id.clone())
            .collect();

        self.set_accounts.set(snapshot.accounts);
        self.set_usage_by_id.set(snapshot.usage_by_account_id);
        self.set_errors_by_id.set(snapshot.errors_by_account_id);
        self.set_quota_events.set(snapshot.quota_events);
        self.set_dismissed_quota_event_ids.update(|set| {
            set.retain(|id| quota_event_ids.contains(id));
        });
        self.set_loading_ids
            .update(|set| set.retain(|id| next_ids.contains(id)));
        self.set_reauth_ids.set(reauth_ids);
        self.set_cost_usage.set(snapshot.cost_usage);
        self.set_cost_error.set(snapshot.cost_error);
        self.set_snapshot_generated_at
            .set(Some(snapshot.generated_at));
        self.set_snapshot_stale.set(snapshot.stale);
    }

    pub(crate) fn finish_listing(&self) {
        self.set_loading_ids.set(HashSet::new());
        self.set_is_listing.set(false);
    }

    pub(crate) fn refresh(&self, force: bool) {
        let actions = *self;
        let ticket = actions.snapshot_epoch.next();
        spawn_local(async move {
            actions.set_is_listing.set(true);
            actions.set_global_error.set(None);
            actions.set_loading_ids.set(
                actions
                    .accounts
                    .get_untracked()
                    .into_iter()
                    .map(|account| account.id)
                    .collect(),
            );

            let result = refresh_snapshot(force).await;
            if !actions.snapshot_epoch.is_current(ticket) {
                return;
            }

            match result {
                Ok(snapshot) => actions.apply_snapshot(snapshot),
                Err(error) => {
                    actions.set_global_error.set(Some(error.user_message));
                }
            }

            actions.finish_listing();
        });
    }

    pub(crate) fn load_cached(&self) {
        let actions = *self;
        spawn_local(async move {
            let result = invoke_tauri::<Option<CodexOverviewSnapshot>>(
                "get_cached_codex_snapshot",
                JsValue::UNDEFINED,
            )
            .await;

            match result {
                Ok(Some(snapshot)) => actions.apply_snapshot(snapshot),
                Ok(None) => {}
                Err(error) => actions.set_global_error.set(Some(error.user_message)),
            }
        });
    }
}
