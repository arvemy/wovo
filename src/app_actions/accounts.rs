use std::collections::HashSet;

use crate::codex_api::{account_action, invoke_tauri, AccountSummary};
use crate::request_epoch::RequestEpoch;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;

use super::SnapshotActions;

#[derive(Clone, Copy)]
pub(crate) struct AccountActions {
    pub(crate) account_epoch: RequestEpoch,
    pub(crate) is_account_login_loading: ReadSignal<bool>,
    pub(crate) set_is_account_action_loading: WriteSignal<bool>,
    pub(crate) set_is_account_login_loading: WriteSignal<bool>,
    pub(crate) set_global_error: WriteSignal<Option<String>>,
    pub(crate) set_usage_by_id:
        WriteSignal<std::collections::HashMap<String, crate::codex_api::UsageSnapshot>>,
    pub(crate) set_errors_by_id:
        WriteSignal<std::collections::HashMap<String, crate::codex_api::AccountIssue>>,
    pub(crate) set_loading_ids: WriteSignal<HashSet<String>>,
    pub(crate) set_reauth_ids: WriteSignal<HashSet<String>>,
    pub(crate) quota_events: ReadSignal<Vec<crate::codex_api::QuotaEvent>>,
    pub(crate) set_quota_events: WriteSignal<Vec<crate::codex_api::QuotaEvent>>,
    pub(crate) set_dismissed_quota_event_ids: WriteSignal<HashSet<String>>,
    pub(crate) snapshot_actions: SnapshotActions,
}

impl AccountActions {
    pub(crate) fn add(&self) {
        let actions = *self;
        let ticket = actions.account_epoch.next();
        actions.set_is_account_action_loading.set(true);
        actions.set_is_account_login_loading.set(true);
        spawn_local(async move {
            actions.set_global_error.set(None);

            let result =
                invoke_tauri::<AccountSummary>("add_codex_account", JsValue::UNDEFINED).await;
            if !actions.account_epoch.is_current(ticket) {
                return;
            }

            match result {
                Ok(_) => actions.snapshot_actions.refresh(true),
                Err(error) => actions.set_global_error.set(Some(error.user_message)),
            }

            actions.set_is_account_login_loading.set(false);
            actions.set_is_account_action_loading.set(false);
        });
    }

    pub(crate) fn cancel_login(&self) {
        if !self.is_account_login_loading.get_untracked() {
            return;
        }

        let actions = *self;
        spawn_local(async move {
            let result =
                invoke_tauri::<bool>("cancel_codex_account_login", JsValue::UNDEFINED).await;

            match result {
                Ok(true) => {
                    actions.account_epoch.next();
                    actions
                        .set_global_error
                        .set(Some("Codex login cancelled.".to_string()));
                    actions.set_is_account_login_loading.set(false);
                    actions.set_is_account_action_loading.set(false);
                }
                Ok(false) => {}
                Err(error) => actions.set_global_error.set(Some(error.user_message)),
            }
        });
    }

    pub(crate) fn reauthenticate(&self, account_id: String) {
        let actions = *self;
        let ticket = actions.account_epoch.next();
        actions.set_is_account_action_loading.set(true);
        actions.set_is_account_login_loading.set(true);
        spawn_local(async move {
            actions.set_global_error.set(None);

            let result =
                account_action::<AccountSummary>("reauthenticate_codex_account", &account_id).await;
            if !actions.account_epoch.is_current(ticket) {
                return;
            }

            match result {
                Ok(account) => {
                    actions.set_reauth_ids.update(|set| {
                        set.remove(&account.id);
                    });
                    actions.snapshot_actions.refresh(true);
                }
                Err(error) => actions.set_global_error.set(Some(error.user_message)),
            }

            actions.set_is_account_login_loading.set(false);
            actions.set_is_account_action_loading.set(false);
        });
    }

    pub(crate) fn remove(&self, account_id: String) {
        let actions = *self;
        let ticket = actions.account_epoch.next();
        spawn_local(async move {
            actions.set_is_account_action_loading.set(true);
            actions.set_global_error.set(None);

            let result = account_action::<()>("remove_codex_account", &account_id).await;
            if !actions.account_epoch.is_current(ticket) {
                return;
            }

            match result {
                Ok(()) => {
                    actions.set_usage_by_id.update(|map| {
                        map.remove(&account_id);
                    });
                    actions.set_errors_by_id.update(|map| {
                        map.remove(&account_id);
                    });
                    actions.set_loading_ids.update(|set| {
                        set.remove(&account_id);
                    });
                    actions.set_reauth_ids.update(|set| {
                        set.remove(&account_id);
                    });
                    actions.set_quota_events.update(|events| {
                        events.retain(|event| event.account_id != account_id);
                    });
                    let remaining_quota_event_ids: HashSet<String> = actions
                        .quota_events
                        .with(|events| events.iter().map(|event| event.id.clone()).collect());
                    actions.set_dismissed_quota_event_ids.update(|set| {
                        set.retain(|id| remaining_quota_event_ids.contains(id));
                    });
                    actions.snapshot_actions.refresh(true);
                }
                Err(error) => actions.set_global_error.set(Some(error.user_message)),
            }

            actions.set_is_account_action_loading.set(false);
        });
    }

    pub(crate) fn set_system(&self, account_id: String) {
        let actions = *self;
        let ticket = actions.account_epoch.next();
        spawn_local(async move {
            actions.set_is_account_action_loading.set(true);
            actions.set_global_error.set(None);

            let result =
                account_action::<AccountSummary>("set_system_codex_account", &account_id).await;
            if !actions.account_epoch.is_current(ticket) {
                return;
            }

            match result {
                Ok(_) => actions.snapshot_actions.refresh(true),
                Err(error) => actions.set_global_error.set(Some(error.user_message)),
            }

            actions.set_is_account_action_loading.set(false);
        });
    }
}
