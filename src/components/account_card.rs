use crate::codex_api::{CreditsSnapshot, UsageSnapshot, UsageWindow};
use crate::components::credential_text::CredentialText;
use crate::formatting::{
    finite_percent, format_remaining_time, format_usage_days, usage_meter_fill_class,
};
use crate::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::ui::button::{ButtonClass, ButtonSize, ButtonVariant};
use crate::ui::separator::Separator;
use crate::ui::tooltip::{Tooltip, TooltipContent, TooltipPosition};
use icons::{LoaderCircle, Trash2};
use leptos::prelude::*;
use tw_merge::IntoTailwindClass;

const SECONDS_PER_DAY: f64 = 86_400.0;
const DEFAULT_WEEKLY_WINDOW_SECONDS: i64 = 7 * 24 * 60 * 60;

#[derive(Clone, Debug, PartialEq)]
pub struct UsageRunwayEstimate {
    pub rate_percent_per_day: f64,
    pub days_until_limit: f64,
}

pub fn weekly_runway_estimate(usage: &UsageSnapshot) -> Option<UsageRunwayEstimate> {
    usage
        .secondary
        .as_ref()
        .and_then(|window| weekly_window_estimate(window, js_sys::Date::now() / 1000.0))
}

fn weekly_window_estimate(window: &UsageWindow, now: f64) -> Option<UsageRunwayEstimate> {
    let reset_at = window.reset_at?;
    let window_seconds = window
        .window_seconds
        .unwrap_or(DEFAULT_WEEKLY_WINDOW_SECONDS);
    let used_percent = finite_percent(window.used_percent)?;
    let remaining_percent = finite_percent(window.remaining_percent)?;
    if window_seconds <= 0 {
        return None;
    }

    let reset_at = reset_at as f64;
    if reset_at <= now {
        return None;
    }

    let start_at = reset_at - window_seconds as f64;
    let elapsed_seconds = (now - start_at).clamp(60.0, window_seconds as f64);
    let elapsed_days = elapsed_seconds / SECONDS_PER_DAY;
    if elapsed_days <= 0.0 {
        return None;
    }

    let rate_percent_per_day = used_percent / elapsed_days;
    if !rate_percent_per_day.is_finite() || rate_percent_per_day <= 0.0 {
        return None;
    }

    let days_until_limit = remaining_percent / rate_percent_per_day;
    if !days_until_limit.is_finite() || days_until_limit < 0.0 {
        return None;
    }

    Some(UsageRunwayEstimate {
        rate_percent_per_day,
        days_until_limit,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "Leptos component props are kept explicit at the account row boundary"
)]
#[component]
pub fn AccountCard<T, M, S, C, X, U, E, L, R, D, H, V>(
    label: T,
    is_managed: M,
    is_live_system: S,
    can_set_system: C,
    can_remove: X,
    usage: U,
    error: E,
    is_loading: L,
    reauth_required: R,
    disabled: D,
    hide_credentials: H,
    is_credential_revealed: V,
    on_reveal_credential: Box<dyn Fn(String) + Send + Sync>,
    on_set_system: Box<dyn Fn() + Send + Sync>,
    on_remove: Box<dyn Fn() + Send + Sync>,
    on_reauth: Box<dyn Fn() + Send + Sync>,
) -> impl IntoView
where
    T: Fn() -> String + Send + Sync + 'static,
    M: Fn() -> bool + Send + Sync + 'static,
    S: Fn() -> bool + Send + Sync + 'static,
    C: Fn() -> bool + Send + Sync + 'static,
    X: Fn() -> bool + Send + Sync + 'static,
    U: Fn() -> Option<UsageSnapshot> + Send + Sync + 'static,
    E: Fn() -> Option<String> + Send + Sync + 'static,
    L: Fn() -> bool + Send + Sync + 'static,
    R: Fn() -> bool + Send + Sync + 'static,
    D: Fn() -> bool + Send + Sync + 'static,
    H: Fn() -> bool + Send + Sync + 'static,
    V: Fn(&str) -> bool + Send + Sync + 'static,
{
    let label = StoredValue::new(label);
    let is_managed = StoredValue::new(is_managed);
    let is_live_system = StoredValue::new(is_live_system);
    let can_set_system = StoredValue::new(can_set_system);
    let can_remove = StoredValue::new(can_remove);
    let usage = StoredValue::new(usage);
    let error = StoredValue::new(error);
    let is_loading = StoredValue::new(is_loading);
    let reauth_required = StoredValue::new(reauth_required);
    let disabled = StoredValue::new(disabled);
    let hide_credentials = StoredValue::new(hide_credentials);
    let is_credential_revealed = StoredValue::new(is_credential_revealed);
    let on_reveal_credential = StoredValue::new(on_reveal_credential);
    let on_set_system = StoredValue::new(on_set_system);
    let on_remove = StoredValue::new(on_remove);
    let on_reauth = StoredValue::new(on_reauth);

    let disabled_for_set_system = disabled;
    let disabled_for_reauth = disabled;
    let disabled_for_remove = disabled;
    let is_loading_call = move || is_loading.with_value(|f| f());
    let reauth_required_call = move || reauth_required.with_value(|f| f());

    let label_call = move || label.with_value(|f| f());
    let is_managed_call = move || is_managed.with_value(|f| f());
    let is_live_system_call = move || is_live_system.with_value(|f| f());
    let can_set_system_call = move || can_set_system.with_value(|f| f());
    let can_remove_call = move || can_remove.with_value(|f| f());
    let plan_label = move || usage.with_value(|f| f().and_then(|s| s.plan_type));
    let usage_source = move || usage.with_value(|f| f().map(|s| s.source));
    let primary = move || usage.with_value(|f| f().and_then(|s| s.primary));
    let secondary = move || usage.with_value(|f| f().and_then(|s| s.secondary));
    let credits = move || usage.with_value(|f| f().and_then(|s| s.credits));
    let has_usage = move || usage.with_value(|f| f().is_some());
    let has_weekly_window = move || usage.with_value(|f| f().and_then(|s| s.secondary).is_some());
    let weekly_estimate =
        move || usage.with_value(|f| f().and_then(|snapshot| weekly_runway_estimate(&snapshot)));

    // Left border color based on primary window usage severity
    let border_color = move || {
        let pct = primary().map(|w| w.used_percent).unwrap_or(0.0);
        if pct >= 90.0 {
            "border-l-[var(--critical)]"
        } else if pct >= 75.0 {
            "border-l-[var(--warning)]"
        } else if is_live_system_call() {
            "border-l-primary"
        } else {
            "border-l-border"
        }
    };

    view! {
        <div class=move || format!(
            "flex flex-col gap-3 border-l-4 border-b border-border bg-card px-4 py-4 {}",
            border_color()
        )>
            // Header row
            <div class="flex items-center gap-2">
                // Label + badges
                <div class="flex min-w-0 flex-1 flex-wrap items-center gap-2">
                    <h2 class="flex min-w-0 items-center font-mono text-sm font-semibold leading-none tracking-tight">
                        <CredentialText
                            value=label_call
                            hide_credentials=move || hide_credentials.with_value(|f| f())
                            is_revealed=move |value| is_credential_revealed.with_value(|f| f(value))
                            on_reveal=Box::new(move |value| on_reveal_credential.with_value(|f| f(value)))
                        />
                    </h2>
                    {move || is_live_system_call().then(|| view! {
                        <Badge variant=BadgeVariant::Default size=BadgeSize::Sm class="h-5 shrink-0 uppercase tracking-wide">
                            "System"
                        </Badge>
                    })}
                    {move || plan_label().map(|plan| view! {
                        <Badge variant=BadgeVariant::Muted size=BadgeSize::Sm class="h-5 shrink-0 uppercase tracking-wide">
                            {plan}
                        </Badge>
                    })}
                    {move || usage_source().map(|source| view! {
                        <Badge variant=BadgeVariant::Muted size=BadgeSize::Sm class="h-5 shrink-0 uppercase tracking-wide">
                            {source}
                        </Badge>
                    })}
                </div>

                // Action buttons
                <div class="flex shrink-0 items-center gap-2">
                    {move || can_set_system_call().then(|| view! {
                        <Tooltip>
                            <button
                                class=ButtonClass { variant: ButtonVariant::Outline, size: ButtonSize::Sm }.with_class("")
                                type="button"
                                aria-label="Set as system account"
                                aria-disabled=move || disabled_for_set_system.with_value(|f| f()).to_string()
                                disabled=move || disabled_for_set_system.with_value(|f| f())
                                on:click=move |_| on_set_system.with_value(|f| f())
                            >
                                "Set as System"
                            </button>
                            <TooltipContent position=TooltipPosition::Bottom>
                                "Set as system account"
                            </TooltipContent>
                        </Tooltip>
                    })}
                    {move || reauth_required_call().then(|| {
                        let trigger = move |_| on_reauth.with_value(|f| f());
                        view! {
                            <Tooltip>
                                <button
                                    class=ButtonClass { variant: ButtonVariant::Outline, size: ButtonSize::Sm }.with_class("")
                                    type="button"
                                    aria-label="Re-authenticate account"
                                    aria-disabled=move || disabled_for_reauth.with_value(|f| f()).to_string()
                                    disabled=move || disabled_for_reauth.with_value(|f| f())
                                    on:click=trigger
                                >
                                    "Re-auth"
                                </button>
                                <TooltipContent position=TooltipPosition::Bottom>
                                    "Re-authenticate account"
                                </TooltipContent>
                            </Tooltip>
                        }
                    })}
                    {move || (is_managed_call() && can_remove_call()).then(|| view! {
                        <Tooltip>
                            <button
                                class=ButtonClass { variant: ButtonVariant::Ghost, size: ButtonSize::Icon }.with_class("text-muted-foreground hover:text-destructive-foreground hover:bg-destructive")
                                type="button"
                                aria-label="Remove account"
                                aria-disabled=move || disabled_for_remove.with_value(|f| f()).to_string()
                                disabled=move || disabled_for_remove.with_value(|f| f())
                                on:click=move |_| on_remove.with_value(|f| f())
                            >
                                <Trash2 class="size-4"/>
                            </button>
                            <TooltipContent position=TooltipPosition::Bottom>
                                "Remove account"
                            </TooltipContent>
                        </Tooltip>
                    })}
                </div>
            </div>

            <Separator/>

            // Usage section
            <div class="grid gap-3">
                {move || primary().map(|window| view! { <UsageMeter window=window/> })}
                {move || secondary().map(|window| view! {
                    <div class="border-t border-border/50 pt-3">
                        <UsageMeter window=window/>
                    </div>
                })}
                {move || {
                    if has_usage() {
                        view! { <span></span> }.into_any()
                    } else if is_loading_call() {
                        view! {
                            <div class="flex items-center gap-2 text-xs text-muted-foreground">
                                <LoaderCircle class="size-3.5 animate-spin"/>
                                <span>"Loading usage..."</span>
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <p class="text-xs text-muted-foreground">"No usage yet."</p>
                        }.into_any()
                    }
                }}
            </div>

            {move || credits().and_then(render_credits)}

            {move || error.with_value(|f| f()).map(|message| view! {
                <p class="text-xs font-medium text-[var(--critical)]">{message}</p>
            })}

            {move || {
                weekly_estimate()
                    .map(|estimate| view! { <UsageRunway estimate=estimate/> }.into_any())
                    .unwrap_or_else(|| {
                        if has_usage() && has_weekly_window() {
                            view! {
                                <p class="text-[11px] font-medium text-muted-foreground">
                                    "Weekly estimate unavailable"
                                </p>
                            }
                            .into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }
                    })
            }}
        </div>
    }
}

#[component]
fn UsageMeter(window: UsageWindow) -> impl IntoView {
    let used = window.used_percent.clamp(0.0, 100.0);
    let remaining = window.remaining_percent.clamp(0.0, 100.0);
    let width = format!("width: {:.1}%;", used);
    let fill_class = usage_meter_fill_class(used);
    let label = window.label.clone();
    let reset_label = window
        .reset_at
        .map(format_remaining_time)
        .unwrap_or_else(|| "reset unknown".to_string());

    let pct_class = if used >= 90.0 {
        "text-base leading-none tabular-nums text-[var(--critical)] font-bold"
    } else if used >= 75.0 {
        "text-base leading-none tabular-nums text-[var(--warning)] font-bold"
    } else {
        "text-base leading-none tabular-nums text-foreground font-semibold"
    };

    view! {
        <div class="grid gap-1.5">
            // Label + large percentage hero
            <div class="flex items-baseline justify-between gap-3">
                <span class="text-xs text-muted-foreground">{label.clone()}</span>
                <strong class=pct_class>{format!("{:.0}%", used)}</strong>
            </div>
            // Thin bar (h-1.5) — subordinate to the number
            <div
                class="h-1.5 w-full overflow-hidden bg-secondary"
                role="progressbar"
                aria-label={format!("{} usage", label)}
                aria-valuemin="0"
                aria-valuemax="100"
                aria-valuenow={format!("{:.0}", used)}
            >
                <div class=fill_class style=width></div>
            </div>
            // Reset time + remaining
            <div class="flex items-center justify-between gap-3 text-[11px]">
                <span class="font-medium text-foreground">{reset_label}</span>
                <span class="text-muted-foreground">{format!("{:.0}% remaining", remaining)}</span>
            </div>
        </div>
    }
}

#[component]
fn UsageRunway(estimate: UsageRunwayEstimate) -> impl IntoView {
    let (label, class) = if estimate.days_until_limit < 1.0 {
        (
                format!("Runway: {}", format_usage_days(estimate.days_until_limit)),
                "flex items-center justify-between border-t border-[var(--warning)] bg-[var(--warning-muted)] px-3 py-1.5 text-[11px] text-[var(--warning-foreground)] -mx-4 -mb-4",
            )
    } else {
        (
                format!("Runway: {}", format_usage_days(estimate.days_until_limit)),
                "flex items-center justify-between border-t border-border bg-secondary px-3 py-1.5 text-[11px] text-muted-foreground -mx-4 -mb-4",
            )
    };

    let rate = format!(
        "{:.1}% used/day",
        estimate.rate_percent_per_day.clamp(0.0, 100.0)
    );

    view! {
        <div class=class>
            <span class="font-medium text-foreground">{label}</span>
            <span>{rate}</span>
        </div>
    }
}

fn render_credits(credits: CreditsSnapshot) -> Option<impl IntoView> {
    let label = if credits.unlimited {
        Some("Credits: unlimited".to_string())
    } else if credits.has_credits {
        credits
            .balance
            .map(|balance| format!("Credits: {:.2}", balance))
    } else {
        None
    }?;

    Some(view! {
        <Badge variant=BadgeVariant::Outline size=BadgeSize::Sm>
            {label}
        </Badge>
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weekly_window(window_seconds: Option<i64>) -> UsageWindow {
        UsageWindow {
            label: "Weekly limit".to_string(),
            used_percent: 20.0,
            remaining_percent: 80.0,
            reset_at: Some(1_700_259_200),
            window_seconds,
        }
    }

    #[test]
    fn weekly_window_estimate_defaults_missing_window_seconds_to_seven_days() {
        let estimate = weekly_window_estimate(&weekly_window(None), 1_700_000_000.0).unwrap();

        assert!((estimate.rate_percent_per_day - 5.0).abs() < f64::EPSILON);
        assert!((estimate.days_until_limit - 16.0).abs() < f64::EPSILON);
    }

    #[test]
    fn weekly_window_estimate_rejects_non_positive_window_seconds() {
        assert!(weekly_window_estimate(&weekly_window(Some(0)), 1_700_000_000.0).is_none());
    }
}
