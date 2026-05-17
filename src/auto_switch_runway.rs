use std::collections::HashMap;

use crate::codex_api::{AccountIssue, AccountSourceKind, AccountSummary, UsageSnapshot};
use crate::components::account_card::weekly_runway_estimate;
use crate::formatting::finite_percent;

#[derive(Clone, Debug, PartialEq)]
pub struct AutoSwitchRunwayEstimate {
    pub days_until_limit: f64,
    pub account_count: usize,
}

pub(crate) fn auto_switch_runway_estimate(
    accounts: &[AccountSummary],
    usage_by_id: &HashMap<String, UsageSnapshot>,
    errors_by_id: &HashMap<String, AccountIssue>,
) -> Option<AutoSwitchRunwayEstimate> {
    let managed_accounts = accounts
        .iter()
        .filter(|account| {
            account.source == AccountSourceKind::Managed && !errors_by_id.contains_key(&account.id)
        })
        .collect::<Vec<_>>();

    if managed_accounts.is_empty() {
        return None;
    }

    let total_remaining = managed_accounts
        .iter()
        .filter_map(|account| usage_by_id.get(&account.id))
        .filter_map(|usage| usage.secondary.as_ref())
        .filter_map(|window| finite_percent(window.remaining_percent))
        .sum::<f64>();

    if total_remaining <= 0.0 {
        return None;
    }

    let active_rate = accounts
        .iter()
        .find(|account| {
            account.is_live_system
                && account.source == AccountSourceKind::Managed
                && !errors_by_id.contains_key(&account.id)
        })
        .and_then(|account| usage_by_id.get(&account.id))
        .and_then(weekly_runway_estimate)
        .map(|estimate| estimate.rate_percent_per_day);

    let rate_percent_per_day = active_rate.or_else(|| {
        let rates = managed_accounts
            .iter()
            .filter_map(|account| usage_by_id.get(&account.id))
            .filter_map(weekly_runway_estimate)
            .map(|estimate| estimate.rate_percent_per_day)
            .collect::<Vec<_>>();

        (!rates.is_empty()).then(|| rates.iter().sum::<f64>() / rates.len() as f64)
    })?;

    if !rate_percent_per_day.is_finite() || rate_percent_per_day <= 0.0 {
        return None;
    }

    Some(AutoSwitchRunwayEstimate {
        days_until_limit: total_remaining / rate_percent_per_day,
        account_count: managed_accounts.len(),
    })
}
