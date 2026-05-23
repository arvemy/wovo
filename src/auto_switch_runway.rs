use std::collections::HashMap;

use crate::codex_api::{
    AccountIssue, AccountSourceKind, AccountSummary, UsageSnapshot, UsageWindow,
};
use crate::components::account_card::{usage_window_runway_estimate, UsageRunwayEstimate};
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
    auto_switch_runway_estimate_at(
        accounts,
        usage_by_id,
        errors_by_id,
        js_sys::Date::now() / 1000.0,
    )
}

fn auto_switch_runway_estimate_at(
    accounts: &[AccountSummary],
    usage_by_id: &HashMap<String, UsageSnapshot>,
    errors_by_id: &HashMap<String, AccountIssue>,
    now: f64,
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
        .filter_map(constrained_weekly_remaining)
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
        .and_then(|usage| constrained_weekly_runway_estimate(usage, now))
        .map(|estimate| estimate.rate_percent_per_day);

    let rate_percent_per_day = active_rate.or_else(|| {
        let rates = managed_accounts
            .iter()
            .filter_map(|account| usage_by_id.get(&account.id))
            .filter_map(|usage| constrained_weekly_runway_estimate(usage, now))
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

fn constrained_weekly_remaining(usage: &UsageSnapshot) -> Option<f64> {
    constrained_weekly_window(usage).and_then(|window| finite_percent(window.remaining_percent))
}

fn constrained_weekly_runway_estimate(
    usage: &UsageSnapshot,
    now: f64,
) -> Option<UsageRunwayEstimate> {
    constrained_weekly_window(usage).and_then(|window| usage_window_runway_estimate(window, now))
}

fn constrained_weekly_window(usage: &UsageSnapshot) -> Option<&UsageWindow> {
    [usage.secondary.as_ref(), usage.tertiary.as_ref()]
        .into_iter()
        .flatten()
        .filter(|window| finite_percent(window.remaining_percent).is_some())
        .min_by(|left, right| {
            let left_remaining = finite_percent(left.remaining_percent).unwrap_or(0.0);
            let right_remaining = finite_percent(right.remaining_percent).unwrap_or(0.0);
            left_remaining.total_cmp(&right_remaining)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, is_live_system: bool) -> AccountSummary {
        AccountSummary {
            id: id.to_string(),
            label: format!("{id}@example.com"),
            source: AccountSourceKind::Managed,
            is_live_system,
            can_set_system: !is_live_system,
            can_remove: !is_live_system,
        }
    }

    fn weekly_window(used_percent: f64, remaining_percent: f64) -> UsageWindow {
        UsageWindow {
            label: "Weekly limit".to_string(),
            used_percent,
            remaining_percent,
            reset_at: Some(1_700_259_200),
            window_seconds: Some(7 * 24 * 60 * 60),
        }
    }

    fn usage(secondary: UsageWindow, tertiary: Option<UsageWindow>) -> UsageSnapshot {
        UsageSnapshot {
            source: "oauth".to_string(),
            source_mode: None,
            fetch_attempts: Vec::new(),
            plan_type: Some("Claude Max".to_string()),
            primary: None,
            secondary: Some(secondary),
            tertiary,
            credits: None,
            updated_at: 1,
        }
    }

    #[test]
    fn runway_estimate_uses_tertiary_when_model_limit_is_more_constrained() {
        let accounts = vec![account("current", true), account("target", false)];
        let usage_by_id = HashMap::from([
            (
                "current".to_string(),
                usage(weekly_window(20.0, 80.0), Some(weekly_window(95.0, 5.0))),
            ),
            ("target".to_string(), usage(weekly_window(10.0, 90.0), None)),
        ]);

        let estimate = auto_switch_runway_estimate_at(
            &accounts,
            &usage_by_id,
            &HashMap::new(),
            1_700_000_000.0,
        )
        .unwrap();

        assert_eq!(estimate.account_count, 2);
        assert!((estimate.days_until_limit - 4.0).abs() < f64::EPSILON);
    }
}
