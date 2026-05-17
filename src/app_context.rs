use std::collections::{HashMap, HashSet};

use crate::codex_api::{AccountIssue, AccountSummary, CostUsageSnapshot, UsageSnapshot};
use leptos::prelude::*;

#[derive(Clone, Copy)]
pub(crate) struct AppUiState {
    pub(crate) is_listing: ReadSignal<bool>,
}

#[derive(Clone, Copy)]
pub(crate) struct CodexOverviewState {
    pub(crate) accounts: ReadSignal<Vec<AccountSummary>>,
    pub(crate) usage_by_id: ReadSignal<HashMap<String, UsageSnapshot>>,
    pub(crate) errors_by_id: ReadSignal<HashMap<String, AccountIssue>>,
    pub(crate) loading_ids: ReadSignal<HashSet<String>>,
    pub(crate) reauth_ids: ReadSignal<HashSet<String>>,
    pub(crate) cost_usage: ReadSignal<Option<CostUsageSnapshot>>,
    pub(crate) cost_error: ReadSignal<Option<String>>,
    pub(crate) snapshot_stale: ReadSignal<bool>,
    pub(crate) revealed_credential: ReadSignal<Option<String>>,
}

#[derive(Clone, Copy)]
pub(crate) struct SettingsState {
    pub(crate) hide_account_credentials: ReadSignal<bool>,
}
