use crate::codex::settings::{
    DEFAULT_AUTO_SWITCH_THRESHOLD_PERCENT, DEFAULT_WEEKLY_PENALTY_THRESHOLD_PERCENT,
};
use crate::domain::account::{AccountSourceKind, AccountSummary};
use crate::domain::usage::{AccountIssue, UsageSnapshot, UsageWindow};
use std::cmp::Ordering;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AutoSwitchPolicy {
    pub(crate) auto_switch_threshold_percent: f64,
    pub(crate) weekly_penalty_threshold_percent: f64,
}

impl Default for AutoSwitchPolicy {
    fn default() -> Self {
        Self {
            auto_switch_threshold_percent: DEFAULT_AUTO_SWITCH_THRESHOLD_PERCENT,
            weekly_penalty_threshold_percent: DEFAULT_WEEKLY_PENALTY_THRESHOLD_PERCENT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UsageWindowKey {
    Primary,
    Secondary,
    Tertiary,
}

impl UsageWindowKey {
    fn all() -> [Self; 3] {
        [Self::Primary, Self::Secondary, Self::Tertiary]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AutoSwitchNotification {
    pub(crate) current_account_label: String,
    pub(crate) target_account_label: String,
    pub(crate) window_label: String,
    pub(crate) threshold_percent: f64,
    pub(crate) target_primary_remaining: Option<f64>,
    pub(crate) target_weekly_remaining: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AutoSwitchCandidate {
    pub(crate) current_account_id: String,
    pub(crate) target_account_id: String,
    pub(crate) window_key: String,
    pub(crate) trigger_reset_at: Option<i64>,
    pub(crate) notification: AutoSwitchNotification,
}

struct ScoredCandidate<'a> {
    account: &'a AccountSummary,
    score: f64,
    window_remaining: f64,
    primary_remaining: Option<f64>,
    weekly_remaining: f64,
}

fn compute_switch_score(
    usage: &UsageSnapshot,
    now_secs: i64,
    weekly_penalty_threshold: f64,
) -> (f64, Option<f64>, f64) {
    let primary_remaining = usage
        .primary
        .as_ref()
        .map(|w| normalized_percent(w.remaining_percent));

    let weekly_remaining_opt = constrained_weekly_remaining(usage);
    let weekly_remaining = weekly_remaining_opt.unwrap_or(100.0);
    let score_base_remaining = primary_remaining.or(weekly_remaining_opt).unwrap_or(0.0);

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
        .map(|w| w * 0.05 * (score_base_remaining / 100.0))
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

    let score = score_base_remaining * weekly_multiplier + weekly_tiebreaker + reset_bonus;
    (score, primary_remaining, weekly_remaining)
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "kept as the default-policy helper for tests and stable internal call sites"
    )
)]
pub(crate) fn auto_switch_candidate(
    accounts: &[AccountSummary],
    usage_by_account_id: &HashMap<String, UsageSnapshot>,
    errors_by_account_id: &HashMap<String, AccountIssue>,
) -> Option<AutoSwitchCandidate> {
    auto_switch_candidate_with_policy(
        accounts,
        usage_by_account_id,
        errors_by_account_id,
        AutoSwitchPolicy::default(),
    )
}

pub(crate) fn auto_switch_candidate_with_policy(
    accounts: &[AccountSummary],
    usage_by_account_id: &HashMap<String, UsageSnapshot>,
    errors_by_account_id: &HashMap<String, AccountIssue>,
    policy: AutoSwitchPolicy,
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

    let trigger_threshold_percent = normalized_percent(policy.auto_switch_threshold_percent);
    let weekly_penalty_threshold_percent =
        normalized_percent(policy.weekly_penalty_threshold_percent);
    let trigger_remaining = 100.0 - trigger_threshold_percent;
    let min_target_remaining = trigger_remaining + 5.0;

    for window_key in UsageWindowKey::all() {
        let trigger_threshold = match window_key {
            UsageWindowKey::Primary => trigger_threshold_percent,
            UsageWindowKey::Secondary | UsageWindowKey::Tertiary => 100.0,
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
            let primary_remaining = usage_window_by_key(usage, UsageWindowKey::Primary)
                .map(|w| normalized_percent(w.remaining_percent));
            let weekly_remaining = constrained_weekly_remaining(usage);

            if window_used >= candidate_threshold
                || window_remaining <= 0.0
                || matches!(primary_remaining, Some(remaining) if remaining < min_target_remaining)
                || matches!(weekly_remaining, Some(remaining) if remaining <= 0.0)
            {
                continue;
            }

            let (score, prem, wrem) =
                compute_switch_score(usage, now_secs, weekly_penalty_threshold_percent);

            match best_target {
                Some(ref b)
                    if !candidate_score_is_better(score, window_remaining, b, window_key) => {}
                _ => {
                    best_target = Some(ScoredCandidate {
                        account,
                        score,
                        window_remaining,
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
                window_key: window_key.as_str().to_string(),
                trigger_reset_at: exhausted_window.reset_at,
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

impl UsageWindowKey {
    fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Secondary => "secondary",
            Self::Tertiary => "tertiary",
        }
    }
}

fn candidate_score_is_better(
    score: f64,
    window_remaining: f64,
    best: &ScoredCandidate<'_>,
    window_key: UsageWindowKey,
) -> bool {
    match score.partial_cmp(&best.score).unwrap_or(Ordering::Less) {
        Ordering::Greater => true,
        Ordering::Equal if window_key == UsageWindowKey::Tertiary => {
            window_remaining > best.window_remaining
        }
        Ordering::Equal | Ordering::Less => false,
    }
}

fn usage_window_by_key(usage: &UsageSnapshot, window_key: UsageWindowKey) -> Option<&UsageWindow> {
    match window_key {
        UsageWindowKey::Primary => usage.primary.as_ref(),
        UsageWindowKey::Secondary => usage.secondary.as_ref(),
        UsageWindowKey::Tertiary => usage.tertiary.as_ref(),
    }
}

fn constrained_weekly_remaining(usage: &UsageSnapshot) -> Option<f64> {
    [UsageWindowKey::Secondary, UsageWindowKey::Tertiary]
        .into_iter()
        .filter_map(|window_key| usage_window_by_key(usage, window_key))
        .map(|window| normalized_percent(window.remaining_percent))
        .reduce(f64::min)
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
