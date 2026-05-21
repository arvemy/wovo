use std::collections::{HashMap, HashSet};

use crate::codex_api::{AccountIssue, AccountSummary, CostUsageSnapshot, UsageSnapshot};
use leptos::prelude::*;

#[derive(Clone, Copy)]
pub(crate) struct AppUiState {
    pub(crate) is_listing: Signal<bool>,
}

#[derive(Clone, Copy)]
pub(crate) struct CodexOverviewState {
    pub(crate) accounts: Signal<Vec<AccountSummary>>,
    pub(crate) usage_by_id: Signal<HashMap<String, UsageSnapshot>>,
    pub(crate) errors_by_id: Signal<HashMap<String, AccountIssue>>,
    pub(crate) loading_ids: Signal<HashSet<String>>,
    pub(crate) reauth_ids: Signal<HashSet<String>>,
    pub(crate) cost_usage: Signal<Option<CostUsageSnapshot>>,
    pub(crate) cost_error: Signal<Option<String>>,
    pub(crate) snapshot_stale: Signal<bool>,
    pub(crate) revealed_credential: Signal<Option<String>>,
}

#[derive(Clone, Copy)]
pub(crate) struct SettingsState {
    pub(crate) hide_account_credentials: Signal<bool>,
}
