use crate::codex::settings::CodexSettings;
use crate::domain::account::{AccountSourceKind, AccountSummary};
use crate::domain::usage::{UsageSnapshot, UsageWindow};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UsageWindowKey {
    Primary,
    Secondary,
}

impl UsageWindowKey {
    fn all() -> [Self; 2] {
        [Self::Primary, Self::Secondary]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AutoSwitchNotification {
    pub(crate) current_account_label: String,
    pub(crate) target_account_label: String,
    pub(crate) window_label: String,
    pub(crate) threshold_percent: f64,
    pub(crate) target_primary_remaining: f64,
    pub(crate) target_weekly_remaining: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AutoSwitchCandidate {
    pub(crate) current_account_id: String,
    pub(crate) target_account_id: String,
    pub(crate) notification: AutoSwitchNotification,
}

struct ScoredCandidate<'a> {
    account: &'a AccountSummary,
    score: f64,
    primary_remaining: f64,
    weekly_remaining: f64,
}

fn compute_switch_score(
    usage: &UsageSnapshot,
    now_secs: i64,
    weekly_penalty_threshold: f64,
) -> (f64, f64, f64) {
    let primary_remaining = usage
        .primary
        .as_ref()
        .map(|w| normalized_percent(w.remaining_percent))
        .unwrap_or(0.0);

    let weekly_remaining_opt = usage
        .secondary
        .as_ref()
        .map(|w| normalized_percent(w.remaining_percent));
    let weekly_remaining = weekly_remaining_opt.unwrap_or(100.0);

    let weekly_multiplier = if weekly_penalty_threshold <= 0.0 {
        1.0
    } else {
        let t = weekly_penalty_threshold;
        if weekly_remaining <= t * 0.10 {
            0.0
        } else if weekly_remaining <= t * 0.25 {
            0.20
        } else if weekly_remaining <= t * 0.50 {
            0.40
        } else if weekly_remaining < t {
            0.60
        } else {
            1.0
        }
    };

    let weekly_tiebreaker = weekly_remaining_opt
        .map(|w| w * 0.05 * (primary_remaining / 100.0))
        .unwrap_or(0.0);

    let reset_bonus = usage
        .primary
        .as_ref()
        .and_then(|w| w.reset_at)
        .filter(|&reset_at| reset_at > now_secs)
        .map(|reset_at| {
            let secs_left = (reset_at - now_secs) as f64;
            let fraction = 1.0 - (secs_left / 18_000.0).min(1.0);
            fraction * 5.0
        })
        .unwrap_or(0.0);

    let score = primary_remaining * weekly_multiplier + weekly_tiebreaker + reset_bonus;
    (score, primary_remaining, weekly_remaining)
}

pub(crate) fn auto_switch_candidate(
    accounts: &[AccountSummary],
    usage_by_account_id: &HashMap<String, UsageSnapshot>,
    errors_by_account_id: &HashMap<String, String>,
    settings: &CodexSettings,
) -> Option<AutoSwitchCandidate> {
    let current = accounts
        .iter()
        .find(|account| account.is_live_system && account.source == AccountSourceKind::Managed)?;
    if errors_by_account_id.contains_key(&current.id) {
        return None;
    }
    let current_usage = usage_by_account_id.get(&current.id)?;

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let trigger_remaining = 100.0 - settings.auto_switch_threshold_percent;
    let min_target_remaining = trigger_remaining + 5.0;

    for window_key in UsageWindowKey::all() {
        let trigger_threshold = match window_key {
            UsageWindowKey::Primary => settings.auto_switch_threshold_percent,
            UsageWindowKey::Secondary => 100.0,
        };
        let candidate_threshold = trigger_threshold;

        let exhausted_window = usage_window_by_key(current_usage, window_key)
            .filter(|window| normalized_percent(window.used_percent) >= trigger_threshold);
        let Some(exhausted_window) = exhausted_window else {
            continue;
        };

        let mut best_target: Option<ScoredCandidate<'_>> = None;
        for account in accounts {
            if account.id == current.id || account.source != AccountSourceKind::Managed {
                continue;
            }
            if errors_by_account_id.contains_key(&account.id) {
                continue;
            }

            let Some(usage) = usage_by_account_id.get(&account.id) else {
                continue;
            };
            let Some(window) = usage_window_by_key(usage, window_key) else {
                continue;
            };

            let window_used = normalized_percent(window.used_percent);
            let window_remaining = normalized_percent(window.remaining_percent);
            let primary_remaining = normalized_percent(
                usage_window_by_key(usage, UsageWindowKey::Primary)
                    .map(|w| w.remaining_percent)
                    .unwrap_or(0.0),
            );
            let secondary_remaining = usage_window_by_key(usage, UsageWindowKey::Secondary)
                .map(|w| normalized_percent(w.remaining_percent));

            if window_used >= candidate_threshold
                || window_remaining <= 0.0
                || primary_remaining < min_target_remaining
                || matches!(secondary_remaining, Some(remaining) if remaining <= 0.0)
            {
                continue;
            }

            let (score, prem, wrem) =
                compute_switch_score(usage, now_secs, settings.weekly_penalty_threshold);

            match best_target {
                Some(ref b) if score <= b.score => {}
                _ => {
                    best_target = Some(ScoredCandidate {
                        account,
                        score,
                        primary_remaining: prem,
                        weekly_remaining: wrem,
                    });
                }
            }
        }

        if let Some(best) = best_target {
            return Some(AutoSwitchCandidate {
                current_account_id: current.id.clone(),
                target_account_id: best.account.id.clone(),
                notification: AutoSwitchNotification {
                    current_account_label: current.label.clone(),
                    target_account_label: best.account.label.clone(),
                    window_label: exhausted_window.label.clone(),
                    threshold_percent: trigger_threshold,
                    target_primary_remaining: best.primary_remaining,
                    target_weekly_remaining: best.weekly_remaining,
                },
            });
        }
    }

    None
}

fn usage_window_by_key(usage: &UsageSnapshot, window_key: UsageWindowKey) -> Option<&UsageWindow> {
    match window_key {
        UsageWindowKey::Primary => usage.primary.as_ref(),
        UsageWindowKey::Secondary => usage.secondary.as_ref(),
    }
}

fn normalized_percent(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests;
