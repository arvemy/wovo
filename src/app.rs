use std::collections::{HashMap, HashSet};

use crate::codex_api::{
    account_action, invoke_tauri, js_command_error, listen_tauri, refresh_snapshot,
    AccountSourceKind, AccountSummary, CodexOverviewSnapshot, CodexSettings, CodexUsageSourceMode,
    CommandError, CostUsageSnapshot, CreditsSnapshot, QuotaEvent,
    SetAutoAccountSwitchingEnabledArgs, SetAutoSwitchThresholdArgs, SetCostUsageEnabledArgs,
    SetHideAccountCredentialsArgs, SetNotificationsEnabledArgs, SetUsageSourceModeArgs,
    SetWeeklyPenaltyThresholdArgs, UsageSnapshot, UsageWindow,
};
use crate::cost_usage_view::CostUsageBreakdown;
use crate::formatting::{
    format_cost, format_remaining_time, format_time_ago, format_tokens, is_auth_failure_message,
    quota_event_body_suffix, quota_event_class, quota_event_kind_label, quota_event_meta_suffix,
    usage_meter_fill_class, utc_day_key,
};
use crate::theme::{
    current_theme_preference, set_theme_preference, theme_menu_item_class, ThemeMode,
};
use crate::ui::{
    alert::{Alert, AlertDescription},
    badge::{Badge, BadgeSize, BadgeVariant},
    button::{ButtonClass, ButtonSize, ButtonVariant},
    card::{Card, CardSize},
    checkbox::Checkbox,
    separator::Separator,
    tooltip::{Tooltip, TooltipAlign, TooltipContent, TooltipPosition},
};
use icons::{EllipsisVertical, Info, LoaderCircle, Monitor, Moon, Plus, RefreshCw, Sun, Trash2, X};
use leptos::prelude::*;
use leptos::task::spawn_local;
use tw_merge::IntoTailwindClass;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

const SECONDS_PER_DAY: f64 = 86_400.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderPage {
    Codex,
    Anthropic,
}

impl ProviderPage {
    fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Anthropic => "Claude Code",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct UsageRunwayEstimate {
    rate_percent_per_day: f64,
    days_until_limit: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct AutoSwitchRunwayEstimate {
    days_until_limit: f64,
    account_count: usize,
}

#[component]
pub fn App() -> impl IntoView {
    let initial_theme_mode = current_theme_preference();
    let (active_provider, set_active_provider) = signal(ProviderPage::Codex);
    let (accounts, set_accounts) = signal::<Vec<AccountSummary>>(Vec::new());
    let (usage_by_id, set_usage_by_id) = signal::<HashMap<String, UsageSnapshot>>(HashMap::new());
    let (errors_by_id, set_errors_by_id) = signal::<HashMap<String, String>>(HashMap::new());
    let (quota_events, set_quota_events) = signal::<Vec<QuotaEvent>>(Vec::new());
    let (dismissed_quota_event_ids, set_dismissed_quota_event_ids) =
        signal::<HashSet<String>>(HashSet::new());
    let (loading_ids, set_loading_ids) = signal::<HashSet<String>>(HashSet::new());
    let (reauth_ids, set_reauth_ids) = signal::<HashSet<String>>(HashSet::new());
    let (usage_source_mode, set_usage_source_mode) = signal(CodexUsageSourceMode::Auto);
    let (theme_mode, set_theme_mode) = signal(initial_theme_mode);
    let (cost_usage_enabled, set_cost_usage_enabled) = signal(false);
    let (notifications_enabled, set_notifications_enabled) = signal(true);
    let (auto_account_switching_enabled, set_auto_account_switching_enabled) = signal(false);
    let (hide_account_credentials, set_hide_account_credentials) = signal(true);
    let (auto_switch_threshold, set_auto_switch_threshold) = signal::<f64>(90.0);
    let (weekly_penalty_threshold, set_weekly_penalty_threshold) = signal::<f64>(20.0);
    let (revealed_credential, set_revealed_credential) = signal::<Option<String>>(None);
    let (is_menu_open, set_is_menu_open) = signal(false);
    let (cost_usage, set_cost_usage) = signal::<Option<CostUsageSnapshot>>(None);
    let (cost_error, set_cost_error) = signal::<Option<String>>(None);
    let (snapshot_generated_at, set_snapshot_generated_at) = signal::<Option<i64>>(None);
    let (snapshot_stale, set_snapshot_stale) = signal(false);
    let (is_settings_loading, set_is_settings_loading) = signal(true);
    let (is_listing, set_is_listing) = signal(true);
    let (is_account_action_loading, set_is_account_action_loading) = signal(false);
    let (global_error, set_global_error) = signal::<Option<String>>(None);

    let apply_settings = move |settings: CodexSettings| {
        set_usage_source_mode.set(settings.usage_source_mode);
        set_cost_usage_enabled.set(settings.cost_usage_enabled);
        set_notifications_enabled.set(settings.notifications_enabled);
        set_auto_account_switching_enabled.set(settings.auto_account_switching_enabled);
        set_hide_account_credentials.set(settings.hide_account_credentials);
        set_auto_switch_threshold.set(settings.auto_switch_threshold_percent);
        set_weekly_penalty_threshold.set(settings.weekly_penalty_threshold);
    };

    let apply_snapshot = move |snapshot: CodexOverviewSnapshot| {
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
            .filter(|(_, message)| is_auth_failure_message(message))
            .map(|(id, _)| id.clone())
            .collect();

        set_accounts.set(snapshot.accounts);
        set_usage_by_id.set(snapshot.usage_by_account_id);
        set_errors_by_id.set(snapshot.errors_by_account_id);
        set_quota_events.set(snapshot.quota_events);
        set_dismissed_quota_event_ids.update(|set| {
            set.retain(|id| quota_event_ids.contains(id));
        });
        set_loading_ids.update(|set| set.retain(|id| next_ids.contains(id)));
        set_reauth_ids.set(reauth_ids);
        set_cost_usage.set(snapshot.cost_usage);
        set_cost_error.set(snapshot.cost_error);
        set_snapshot_generated_at.set(Some(snapshot.generated_at));
        set_snapshot_stale.set(snapshot.stale);
    };

    let refresh_overview_snapshot = move |force: bool| {
        spawn_local(async move {
            set_is_listing.set(true);
            set_global_error.set(None);
            set_loading_ids.set(
                accounts
                    .get_untracked()
                    .into_iter()
                    .map(|account| account.id)
                    .collect(),
            );

            match refresh_snapshot(force).await {
                Ok(snapshot) => apply_snapshot(snapshot),
                Err(error) => {
                    set_global_error.set(Some(error.message));
                }
            }

            set_loading_ids.set(HashSet::new());
            set_is_listing.set(false);
        });
    };

    let load_cached_snapshot = move || {
        spawn_local(async move {
            match invoke_tauri::<Option<CodexOverviewSnapshot>>(
                "get_cached_codex_snapshot",
                JsValue::UNDEFINED,
            )
            .await
            {
                Ok(Some(snapshot)) => apply_snapshot(snapshot),
                Ok(None) => {}
                Err(error) => set_global_error.set(Some(error.message)),
            }
        });
    };

    let load_settings = move || {
        spawn_local(async move {
            set_is_settings_loading.set(true);
            match invoke_tauri::<CodexSettings>("get_codex_settings", JsValue::UNDEFINED).await {
                Ok(settings) => {
                    apply_settings(settings);
                }
                Err(error) => set_global_error.set(Some(error.message)),
            }
            set_is_settings_loading.set(false);
        });
    };

    let change_theme_mode = move |mode: ThemeMode| {
        if mode == theme_mode.get_untracked() {
            return;
        }

        set_theme_mode.set(mode);
        if let Err(error) = set_theme_preference(mode.storage_value()) {
            set_global_error.set(Some(js_command_error(&error).message));
        }
    };

    let change_usage_source_mode = move |mode: CodexUsageSourceMode| {
        if mode == usage_source_mode.get_untracked() {
            return;
        }
        let previous = usage_source_mode.get_untracked();
        set_usage_source_mode.set(mode);
        spawn_local(async move {
            set_is_settings_loading.set(true);
            set_global_error.set(None);
            let args = serde_wasm_bindgen::to_value(&SetUsageSourceModeArgs {
                usage_source_mode: mode,
            })
            .map_err(|error| CommandError::from_message(error.to_string()));

            match args {
                Ok(args) => {
                    match invoke_tauri::<CodexSettings>("set_codex_usage_source_mode", args).await {
                        Ok(settings) => {
                            apply_settings(settings);
                            refresh_overview_snapshot(true);
                        }
                        Err(error) => {
                            set_usage_source_mode.set(previous);
                            set_global_error.set(Some(error.message));
                        }
                    }
                }
                Err(error) => {
                    set_usage_source_mode.set(previous);
                    set_global_error.set(Some(error.message));
                }
            }
            set_is_settings_loading.set(false);
        });
    };

    let change_cost_usage_enabled = move |enabled: bool| {
        if enabled == cost_usage_enabled.get_untracked() {
            return;
        }
        let previous = cost_usage_enabled.get_untracked();
        set_cost_usage_enabled.set(enabled);
        if !enabled {
            set_cost_usage.set(None);
            set_cost_error.set(None);
        }

        spawn_local(async move {
            set_is_settings_loading.set(true);
            set_global_error.set(None);
            let args = serde_wasm_bindgen::to_value(&SetCostUsageEnabledArgs { enabled })
                .map_err(|error| CommandError::from_message(error.to_string()));

            match args {
                Ok(args) => {
                    match invoke_tauri::<CodexSettings>("set_codex_cost_usage_enabled", args).await
                    {
                        Ok(settings) => {
                            apply_settings(settings);
                            refresh_overview_snapshot(true);
                        }
                        Err(error) => {
                            set_cost_usage_enabled.set(previous);
                            set_global_error.set(Some(error.message));
                        }
                    }
                }
                Err(error) => {
                    set_cost_usage_enabled.set(previous);
                    set_global_error.set(Some(error.message));
                }
            }
            set_is_settings_loading.set(false);
        });
    };

    let change_notifications_enabled = move |enabled: bool| {
        if enabled == notifications_enabled.get_untracked() {
            return;
        }
        let previous = notifications_enabled.get_untracked();
        set_notifications_enabled.set(enabled);

        spawn_local(async move {
            set_is_settings_loading.set(true);
            set_global_error.set(None);
            let args = serde_wasm_bindgen::to_value(&SetNotificationsEnabledArgs { enabled })
                .map_err(|error| CommandError::from_message(error.to_string()));

            match args {
                Ok(args) => {
                    match invoke_tauri::<CodexSettings>("set_codex_notifications_enabled", args)
                        .await
                    {
                        Ok(settings) => {
                            apply_settings(settings);
                        }
                        Err(error) => {
                            set_notifications_enabled.set(previous);
                            set_global_error.set(Some(error.message));
                        }
                    }
                }
                Err(error) => {
                    set_notifications_enabled.set(previous);
                    set_global_error.set(Some(error.message));
                }
            }
            set_is_settings_loading.set(false);
        });
    };

    let change_auto_account_switching_enabled = move |enabled: bool| {
        if enabled == auto_account_switching_enabled.get_untracked() {
            return;
        }
        let previous = auto_account_switching_enabled.get_untracked();
        set_auto_account_switching_enabled.set(enabled);

        spawn_local(async move {
            set_is_settings_loading.set(true);
            set_global_error.set(None);
            let args =
                serde_wasm_bindgen::to_value(&SetAutoAccountSwitchingEnabledArgs { enabled })
                    .map_err(|error| CommandError::from_message(error.to_string()));

            match args {
                Ok(args) => {
                    match invoke_tauri::<CodexSettings>(
                        "set_codex_auto_account_switching_enabled",
                        args,
                    )
                    .await
                    {
                        Ok(settings) => {
                            apply_settings(settings);
                            if enabled {
                                refresh_overview_snapshot(true);
                            }
                        }
                        Err(error) => {
                            set_auto_account_switching_enabled.set(previous);
                            set_global_error.set(Some(error.message));
                        }
                    }
                }
                Err(error) => {
                    set_auto_account_switching_enabled.set(previous);
                    set_global_error.set(Some(error.message));
                }
            }
            set_is_settings_loading.set(false);
        });
    };

    let change_hide_account_credentials = move |enabled: bool| {
        if enabled == hide_account_credentials.get_untracked() {
            return;
        }
        let previous = hide_account_credentials.get_untracked();
        set_hide_account_credentials.set(enabled);
        if enabled {
            set_revealed_credential.set(None);
        }

        spawn_local(async move {
            set_is_settings_loading.set(true);
            set_global_error.set(None);
            let args = serde_wasm_bindgen::to_value(&SetHideAccountCredentialsArgs { enabled })
                .map_err(|error| CommandError::from_message(error.to_string()));

            match args {
                Ok(args) => {
                    match invoke_tauri::<CodexSettings>("set_codex_hide_account_credentials", args)
                        .await
                    {
                        Ok(settings) => {
                            let hide_enabled = settings.hide_account_credentials;
                            apply_settings(settings);
                            if hide_enabled {
                                set_revealed_credential.set(None);
                            }
                        }
                        Err(error) => {
                            set_hide_account_credentials.set(previous);
                            set_global_error.set(Some(error.message));
                        }
                    }
                }
                Err(error) => {
                    set_hide_account_credentials.set(previous);
                    set_global_error.set(Some(error.message));
                }
            }
            set_is_settings_loading.set(false);
        });
    };

    let change_auto_switch_threshold = move |value: f64| {
        let clamped = if value.is_finite() {
            value.clamp(50.0, 100.0)
        } else {
            90.0
        };
        if clamped == auto_switch_threshold.get_untracked() {
            return;
        }
        let previous = auto_switch_threshold.get_untracked();
        set_auto_switch_threshold.set(clamped);
        spawn_local(async move {
            set_is_settings_loading.set(true);
            set_global_error.set(None);
            let args = serde_wasm_bindgen::to_value(&SetAutoSwitchThresholdArgs { value: clamped })
                .map_err(|error| CommandError::from_message(error.to_string()));
            match args {
                Ok(args) => {
                    match invoke_tauri::<CodexSettings>(
                        "set_codex_auto_switch_threshold_percent",
                        args,
                    )
                    .await
                    {
                        Ok(settings) => apply_settings(settings),
                        Err(error) => {
                            set_auto_switch_threshold.set(previous);
                            set_global_error.set(Some(error.message));
                        }
                    }
                }
                Err(error) => {
                    set_auto_switch_threshold.set(previous);
                    set_global_error.set(Some(error.message));
                }
            }
            set_is_settings_loading.set(false);
        });
    };

    let change_weekly_penalty_threshold = move |value: f64| {
        let clamped = if value.is_finite() {
            value.clamp(0.0, 50.0)
        } else {
            20.0
        };
        if clamped == weekly_penalty_threshold.get_untracked() {
            return;
        }
        let previous = weekly_penalty_threshold.get_untracked();
        set_weekly_penalty_threshold.set(clamped);
        spawn_local(async move {
            set_is_settings_loading.set(true);
            set_global_error.set(None);
            let args =
                serde_wasm_bindgen::to_value(&SetWeeklyPenaltyThresholdArgs { value: clamped })
                    .map_err(|error| CommandError::from_message(error.to_string()));
            match args {
                Ok(args) => {
                    match invoke_tauri::<CodexSettings>("set_codex_weekly_penalty_threshold", args)
                        .await
                    {
                        Ok(settings) => apply_settings(settings),
                        Err(error) => {
                            set_weekly_penalty_threshold.set(previous);
                            set_global_error.set(Some(error.message));
                        }
                    }
                }
                Err(error) => {
                    set_weekly_penalty_threshold.set(previous);
                    set_global_error.set(Some(error.message));
                }
            }
            set_is_settings_loading.set(false);
        });
    };

    let listen_for_snapshots = move || {
        let handler = Closure::<dyn FnMut(JsValue)>::new(move |event| {
            let payload = js_sys::Reflect::get(&event, &JsValue::from_str("payload"))
                .unwrap_or(JsValue::UNDEFINED);
            if let Ok(snapshot) = serde_wasm_bindgen::from_value::<CodexOverviewSnapshot>(payload) {
                apply_snapshot(snapshot);
                set_loading_ids.set(HashSet::new());
                set_is_listing.set(false);
            }
        });
        let function = handler.as_ref().unchecked_ref::<js_sys::Function>().clone();
        spawn_local(async move {
            match listen_tauri("codex:snapshot-updated", &function).await {
                Ok(_) => handler.forget(),
                Err(error) => set_global_error.set(Some(js_command_error(&error).message)),
            }
        });
    };

    load_settings();
    load_cached_snapshot();
    listen_for_snapshots();
    refresh_overview_snapshot(false);

    let refresh_all = move |_| refresh_overview_snapshot(true);

    let add_account = move || {
        spawn_local(async move {
            set_is_account_action_loading.set(true);
            set_global_error.set(None);

            match invoke_tauri::<AccountSummary>("add_codex_account", JsValue::UNDEFINED).await {
                Ok(_) => refresh_overview_snapshot(true),
                Err(error) => set_global_error.set(Some(error.message)),
            }

            set_is_account_action_loading.set(false);
        });
    };

    let cancel_account_login = move || {
        spawn_local(async move {
            match invoke_tauri::<bool>("cancel_codex_account_login", JsValue::UNDEFINED).await {
                Ok(true) => set_global_error.set(Some("Codex login cancelled.".to_string())),
                Ok(false) => set_is_account_action_loading.set(false),
                Err(error) => set_global_error.set(Some(error.message)),
            }
        });
    };

    let reauthenticate_account = move |account_id: String| {
        spawn_local(async move {
            set_is_account_action_loading.set(true);
            set_global_error.set(None);

            match account_action::<AccountSummary>("reauthenticate_codex_account", &account_id)
                .await
            {
                Ok(account) => {
                    set_reauth_ids.update(|set| {
                        set.remove(&account.id);
                    });
                    refresh_overview_snapshot(true);
                }
                Err(error) => set_global_error.set(Some(error.message)),
            }

            set_is_account_action_loading.set(false);
        });
    };

    let remove_account = move |account_id: String| {
        spawn_local(async move {
            set_is_account_action_loading.set(true);
            set_global_error.set(None);

            match account_action::<()>("remove_codex_account", &account_id).await {
                Ok(()) => {
                    set_usage_by_id.update(|map| {
                        map.remove(&account_id);
                    });
                    set_errors_by_id.update(|map| {
                        map.remove(&account_id);
                    });
                    set_loading_ids.update(|set| {
                        set.remove(&account_id);
                    });
                    set_reauth_ids.update(|set| {
                        set.remove(&account_id);
                    });
                    set_quota_events.update(|events| {
                        events.retain(|event| event.account_id != account_id);
                    });
                    let remaining_quota_event_ids: HashSet<String> = quota_events
                        .with(|events| events.iter().map(|event| event.id.clone()).collect());
                    set_dismissed_quota_event_ids.update(|set| {
                        set.retain(|id| remaining_quota_event_ids.contains(id));
                    });
                    refresh_overview_snapshot(true);
                }
                Err(error) => set_global_error.set(Some(error.message)),
            }

            set_is_account_action_loading.set(false);
        });
    };

    let set_system_account = move |account_id: String| {
        spawn_local(async move {
            set_is_account_action_loading.set(true);
            set_global_error.set(None);

            match account_action::<AccountSummary>("set_system_codex_account", &account_id).await {
                Ok(_) => refresh_overview_snapshot(true),
                Err(error) => set_global_error.set(Some(error.message)),
            }

            set_is_account_action_loading.set(false);
        });
    };

    let any_action_in_flight = move || is_account_action_loading.get();
    let any_loading = Memo::new(move |_| !loading_ids.get().is_empty());
    let latest_updated_at = Memo::new(move |_| {
        let usage_updated_at = usage_by_id.with(|map| map.values().map(|s| s.updated_at).max());
        let cost_updated_at = cost_usage.with(|usage| usage.as_ref().map(|usage| usage.updated_at));
        [
            usage_updated_at,
            cost_updated_at,
            snapshot_generated_at.get(),
        ]
        .into_iter()
        .flatten()
        .max()
    });
    let visible_quota_events = Memo::new(move |_| {
        let dismissed = dismissed_quota_event_ids.get();
        quota_events
            .get()
            .into_iter()
            .filter(|event| !dismissed.contains(&event.id))
            .collect::<Vec<_>>()
    });
    let auto_switch_runway = Memo::new(move |_| {
        let current_accounts = accounts.get();
        let current_usage = usage_by_id.get();
        let current_errors = errors_by_id.get();
        auto_switch_runway_estimate(&current_accounts, &current_usage, &current_errors)
    });

    view! {
        <div
            class="min-h-screen overflow-hidden bg-background text-foreground"
            on:click=move |_| {
                if hide_account_credentials.get_untracked() {
                    set_revealed_credential.set(None);
                }
            }
        >
            <div class="app-shell mx-auto flex h-screen w-[min(960px,calc(100vw-1rem))] min-w-0 flex-col gap-3 px-3 py-3">
                <nav class="flex h-12 shrink-0 items-center justify-between gap-3 border-b border-border">
                    <ProviderNav
                        active=move || active_provider.get()
                        on_select=Box::new(move |page| set_active_provider.set(page))
                    />
                    <div class="flex min-w-0 items-center gap-2">
                        <AutoSwitchStatus
                            enabled=move || auto_account_switching_enabled.get()
                            disabled=move || is_settings_loading.get() || any_action_in_flight()
                            estimate=move || auto_switch_runway.get()
                            on_change=Box::new(move |enabled| change_auto_account_switching_enabled(enabled))
                        />
                    <Tooltip>
                        <button
                            class=ButtonClass { variant: ButtonVariant::Outline, size: ButtonSize::Icon }.with_class("")
                            type="button"
                            aria-label=move || {
                                if is_account_action_loading.get() {
                                    "Cancel login"
                                } else {
                                    "Add Codex account"
                                }
                            }
                            on:click=move |_| {
                                if is_account_action_loading.get_untracked() {
                                    cancel_account_login();
                                } else {
                                    add_account();
                                }
                            }
                            disabled=move || is_listing.get()
                        >
                            {move || if is_account_action_loading.get() {
                                view! { <LoaderCircle class="size-4 animate-spin"/> }.into_any()
                            } else {
                                view! { <Plus class="size-4"/> }.into_any()
                            }}
                        </button>
                        <TooltipContent position=TooltipPosition::Bottom>
                            {move || if is_account_action_loading.get() { "Cancel login" } else { "Add Codex account" }}
                        </TooltipContent>
                    </Tooltip>
                    <Tooltip>
                        <button
                            class=ButtonClass { variant: ButtonVariant::Outline, size: ButtonSize::Icon }.with_class("")
                            type="button"
                            aria-label="Refresh all accounts"
                            on:click=refresh_all
                            disabled=move || is_listing.get() || any_action_in_flight()
                        >
                            {move || {
                                if is_listing.get() || any_loading.get() {
                                    view! { <LoaderCircle class="size-4 animate-spin"/> }.into_any()
                                } else {
                                    view! { <RefreshCw class="size-4"/> }.into_any()
                                }
                            }}
                        </button>
                        <TooltipContent position=TooltipPosition::Bottom>
                            "Refresh all accounts"
                        </TooltipContent>
                    </Tooltip>
                    <div class="relative">
                        <Tooltip>
                        <button
                            class=ButtonClass { variant: ButtonVariant::Outline, size: ButtonSize::Icon }.with_class("")
                            type="button"
                            aria-label="Settings"
                            aria-expanded=move || is_menu_open.get().to_string()
                            on:click=move |_| set_is_menu_open.update(|o| *o = !*o)
                        >
                            <EllipsisVertical class="size-4"/>
                        </button>
                            <TooltipContent position=TooltipPosition::Bottom>
                                "Settings"
                            </TooltipContent>
                        </Tooltip>
                        {move || is_menu_open.get().then(|| view! {
                            <div
                                class="fixed inset-0 z-40"
                                on:click=move |_| set_is_menu_open.set(false)
                            />
                            <div class="absolute right-0 top-full z-50 mt-1 w-60 overflow-hidden rounded-md border border-border bg-background p-1.5 shadow-md">
                                <div class="px-2 py-1.5">
                                    <p class="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                                        "Theme"
                                    </p>
                                    <div
                                        class="grid gap-1"
                                        role="group"
                                        aria-label="Theme"
                                    >
                                        {[ThemeMode::Light, ThemeMode::Dark, ThemeMode::Auto]
                                            .into_iter()
                                            .map(|mode| {
                                                let selected = move || theme_mode.get() == mode;
                                                view! {
                                                    <button
                                                        class=move || theme_menu_item_class(selected())
                                                        type="button"
                                                        aria-pressed=move || selected().to_string()
                                                        on:click=move |_| change_theme_mode(mode)
                                                    >
                                                        <span class="flex min-w-0 items-center gap-2">
                                                            {match mode {
                                                                ThemeMode::Light => view! { <Sun class="size-3.5 shrink-0"/> }.into_any(),
                                                                ThemeMode::Dark => view! { <Moon class="size-3.5 shrink-0"/> }.into_any(),
                                                                ThemeMode::Auto => view! { <Monitor class="size-3.5 shrink-0"/> }.into_any(),
                                                            }}
                                                            <span class="truncate">{mode.label()}</span>
                                                        </span>
                                                    </button>
                                                }
                                            })
                                            .collect_view()
                                        }
                                    </div>
                                </div>
                                <div class="my-1 h-px bg-border"/>
                                <div class="px-2 py-1.5">
                                    <p class="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                                        "Usage Source"
                                    </p>
                                    <div
                                        class="inline-grid h-8 w-full grid-cols-3 rounded-md border border-border bg-secondary p-0.5"
                                        role="group"
                                        aria-label="Usage source"
                                    >
                                        {[CodexUsageSourceMode::Auto, CodexUsageSourceMode::Oauth, CodexUsageSourceMode::Cli]
                                            .into_iter()
                                            .map(|mode| {
                                                let selected = move || usage_source_mode.get() == mode;
                                                view! {
                                                    <button
                                                        class=move || if selected() {
                                                            "inline-flex min-w-12 items-center justify-center rounded-sm bg-background px-2 text-[11px] font-semibold text-foreground shadow-xs transition-colors"
                                                        } else {
                                                            "inline-flex min-w-12 items-center justify-center rounded-sm px-2 text-[11px] font-medium text-muted-foreground transition-colors hover:cursor-pointer hover:text-foreground"
                                                        }
                                                        type="button"
                                                        aria-pressed=move || selected().to_string()
                                                        disabled=move || is_listing.get() || is_settings_loading.get() || any_action_in_flight()
                                                        on:click=move |_| change_usage_source_mode(mode)
                                                    >
                                                        {mode.label()}
                                                    </button>
                                                }
                                            })
                                            .collect_view()
                                        }
                                    </div>
                                </div>
                                <div class="my-1 h-px bg-border"/>
                                <label class="flex w-full cursor-pointer items-center gap-3 rounded-sm px-2 py-1.5 hover:bg-accent">
                                    <Checkbox
                                        checked=move || cost_usage_enabled.get()
                                        disabled=move || is_listing.get() || is_settings_loading.get() || any_action_in_flight()
                                        on_change=move |enabled| change_cost_usage_enabled(enabled)
                                    />
                                    <div>
                                        <p class="text-sm font-medium leading-none">"Cost tracker"</p>
                                        <p class="mt-0.5 text-[11px] text-muted-foreground">"Track local token cost"</p>
                                    </div>
                                </label>
                                <label class="flex w-full cursor-pointer items-center gap-3 rounded-sm px-2 py-1.5 hover:bg-accent">
                                    <Checkbox
                                        checked=move || notifications_enabled.get()
                                        disabled=move || is_settings_loading.get()
                                        on_change=move |enabled| change_notifications_enabled(enabled)
                                    />
                                    <div>
                                        <p class="text-sm font-medium leading-none">"Notifications"</p>
                                        <p class="mt-0.5 text-[11px] text-muted-foreground">"Quota and auto-switch alerts"</p>
                                    </div>
                                </label>
                                <label class="flex w-full cursor-pointer items-center gap-3 rounded-sm px-2 py-1.5 hover:bg-accent">
                                    <Checkbox
                                        checked=move || hide_account_credentials.get()
                                        disabled=move || is_settings_loading.get()
                                        on_change=move |enabled| change_hide_account_credentials(enabled)
                                    />
                                    <div>
                                        <p class="text-sm font-medium leading-none">"Hide credentials"</p>
                                        <p class="mt-0.5 text-[11px] text-muted-foreground">"Blur account labels"</p>
                                    </div>
                                </label>
                                {move || auto_account_switching_enabled.get().then(|| view! {
                                    <div class="my-1 h-px bg-border"/>
                                    <div class="px-2 py-1.5">
                                        <p class="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                                            "Auto Switch"
                                        </p>
                                        <div class="flex items-center justify-between gap-2 py-1">
                                            <span class="text-sm font-medium">"Switch when 5h used ≥"</span>
                                            <div class="flex items-center gap-1">
                                                <input
                                                    type="number"
                                                    min="50"
                                                    max="100"
                                                    step="1"
                                                    class="w-14 rounded border border-border bg-background px-1.5 py-0.5 text-xs text-right"
                                                    disabled=move || is_settings_loading.get()
                                                    prop:value=move || auto_switch_threshold.get().to_string()
                                                    on:change=move |ev| {
                                                        if let Ok(v) = event_target_value(&ev).parse::<f64>() {
                                                            change_auto_switch_threshold(v);
                                                        }
                                                    }
                                                />
                                                <span class="text-xs text-muted-foreground">"%"</span>
                                            </div>
                                        </div>
                                        <div class="flex items-center justify-between gap-2 py-1">
                                            <span class="text-sm font-medium">"Penalise if weekly remaining <"</span>
                                            <div class="flex items-center gap-1">
                                                <input
                                                    type="number"
                                                    min="0"
                                                    max="50"
                                                    step="1"
                                                    class="w-14 rounded border border-border bg-background px-1.5 py-0.5 text-xs text-right"
                                                    disabled=move || is_settings_loading.get()
                                                    prop:value=move || weekly_penalty_threshold.get().to_string()
                                                    on:change=move |ev| {
                                                        if let Ok(v) = event_target_value(&ev).parse::<f64>() {
                                                            change_weekly_penalty_threshold(v);
                                                        }
                                                    }
                                                />
                                                <span class="text-xs text-muted-foreground">"% (0=off)"</span>
                                            </div>
                                        </div>
                                    </div>
                                })}
                            </div>
                        })}
                    </div>
                </div>
                </nav>
                {move || match active_provider.get() {
                    ProviderPage::Codex => view! {
                        <main class="codex-page flex min-h-0 flex-1 flex-col overflow-visible">

            <div class="min-h-0 flex-1 overflow-y-auto overflow-x-hidden pr-1 pb-3">
            {move || {
                cost_usage.get().map(|usage| view! {
                    <CostSummary usage=usage/>
                })
            }}

            {move || {
                cost_error.get().map(|message| view! {
                    <p class="mb-3 text-xs font-medium text-[var(--critical)]">{message}</p>
                })
            }}

            {move || {
                global_error.get().map(|message| view! {
                    <Alert class="mb-4 border-[var(--warning)] bg-[var(--warning-muted)] text-[var(--warning-foreground)]">
                        <AlertDescription class="flex items-center justify-between gap-3">
                            <span>{message}</span>
                            <button
                                class="shrink-0 opacity-70 hover:opacity-100 hover:cursor-pointer"
                                type="button"
                                aria-label="Dismiss error"
                                title="Dismiss error"
                                on:click=move |_| set_global_error.set(None)
                            >
                                <X class="size-4"/>
                            </button>
                        </AlertDescription>
                    </Alert>
                })
            }}

            {move || {
                if visible_quota_events.get().is_empty() {
                    view! { <span></span> }.into_any()
                } else {
                    view! {
                        <div class="mb-4 grid gap-2">
                            <For
                                each=move || visible_quota_events.get()
                                key=|event| event.id.clone()
                                children=move |event| {
                                    let dismiss = Box::new(move |event_id: String| {
                                        set_dismissed_quota_event_ids.update(|set| {
                                            set.insert(event_id);
                                        });
                                    });
                                    view! {
                                        <QuotaEventCard
                                            event=event
                                            hide_credentials=move || hide_account_credentials.get()
                                            is_credential_revealed=move |value| {
                                                revealed_credential.with(|current| current.as_deref() == Some(value))
                                            }
                                            on_reveal_credential=Box::new(move |value| {
                                                set_revealed_credential.update(|current| {
                                                    let already_revealed = current.as_deref() == Some(value.as_str());
                                                    if already_revealed {
                                                        *current = None;
                                                    } else {
                                                        *current = Some(value);
                                                    }
                                                });
                                            })
                                            on_dismiss=dismiss
                                        />
                                    }
                                }
                            />
                        </div>
                    }
                    .into_any()
                }
            }}

            {move || snapshot_stale.get().then(|| view! {
                <div class="mb-3">
                    <Badge variant=BadgeVariant::Warning size=BadgeSize::Sm>
                        "Cached snapshot"
                    </Badge>
                </div>
            })}

            {move || {
                let current = accounts.get();
                if current.is_empty() {
                    if is_listing.get() {
                        view! {
                            <div class="flex flex-col items-center justify-center gap-2 py-12 text-center">
                                <LoaderCircle class="size-4 animate-spin text-muted-foreground"/>
                                <p class="text-xs font-medium text-muted-foreground">"Checking Codex"</p>
                            </div>
                        }
                        .into_any()
                    } else {
                        view! {
                            <div class="flex flex-col items-center justify-center gap-2 py-12 text-center">
                                <h2 class="text-sm font-semibold leading-none">"No Codex account found"</h2>
                                <p class="text-xs text-muted-foreground">
                                    "Use the + button above to add an account, or run `codex login`."
                                </p>
                            </div>
                        }
                        .into_any()
                    }
                } else {
                    view! {
                        <div class="flex flex-col gap-3">
                            <For
                                each=move || accounts.get()
                                key=|account| account.id.clone()
                                children=move |account| {
                                    let id_for_usage = account.id.clone();
                                    let id_for_error = account.id.clone();
                                    let id_for_loading = account.id.clone();
                                    let id_for_reauth = account.id.clone();
                                    let id_for_remove = account.id.clone();
                                    let id_for_set_system = account.id.clone();
                                    let id_for_reauth_action = account.id.clone();
                                    let id_for_label = account.id.clone();
                                    let id_for_source = account.id.clone();
                                    let id_for_live_system = account.id.clone();
                                    let id_for_can_set_system = account.id.clone();
                                    let id_for_can_remove = account.id.clone();
                                    let fallback_label = account.label.clone();
                                    let fallback_source = account.source.clone();
                                    let fallback_is_live_system = account.is_live_system;
                                    let fallback_can_set_system = account.can_set_system;
                                    let fallback_can_remove = account.can_remove;

                                    let usage_signal = move || usage_by_id.with(|map| map.get(&id_for_usage).cloned());
                                    let error_signal = move || errors_by_id.with(|map| map.get(&id_for_error).cloned());
                                    let loading_signal = move || loading_ids.with(|set| set.contains(&id_for_loading));
                                    let reauth_signal = move || reauth_ids.with(|set| set.contains(&id_for_reauth));
                                    let label_signal = move || accounts.with(|items| {
                                        items
                                            .iter()
                                            .find(|item| item.id == id_for_label)
                                            .map(|item| item.label.clone())
                                            .unwrap_or_else(|| fallback_label.clone())
                                    });
                                    let managed_signal = move || accounts.with(|items| {
                                        items
                                            .iter()
                                            .find(|item| item.id == id_for_source)
                                            .map(|item| item.source == AccountSourceKind::Managed)
                                            .unwrap_or(fallback_source == AccountSourceKind::Managed)
                                    });
                                    let live_system_signal = move || accounts.with(|items| {
                                        items
                                            .iter()
                                            .find(|item| item.id == id_for_live_system)
                                            .map(|item| item.is_live_system)
                                            .unwrap_or(fallback_is_live_system)
                                    });
                                    let can_set_system_signal = move || accounts.with(|items| {
                                        items
                                            .iter()
                                            .find(|item| item.id == id_for_can_set_system)
                                            .map(|item| item.can_set_system)
                                            .unwrap_or(fallback_can_set_system)
                                    });
                                    let can_remove_signal = move || accounts.with(|items| {
                                        items
                                            .iter()
                                            .find(|item| item.id == id_for_can_remove)
                                            .map(|item| item.can_remove)
                                            .unwrap_or(fallback_can_remove)
                                    });

                                    view! {
                                        <AccountRow
                                            label=label_signal
                                            is_managed=managed_signal
                                            is_live_system=live_system_signal
                                            can_set_system=can_set_system_signal
                                            can_remove=can_remove_signal
                                            usage=usage_signal
                                            error=error_signal
                                            is_loading=loading_signal
                                            reauth_required=reauth_signal
                                            disabled=any_action_in_flight
                                            hide_credentials=move || hide_account_credentials.get()
                                            is_credential_revealed=move |value| {
                                                revealed_credential.with(|current| current.as_deref() == Some(value))
                                            }
                                            on_reveal_credential=Box::new(move |value| {
                                                set_revealed_credential.update(|current| {
                                                    let already_revealed = current.as_deref() == Some(value.as_str());
                                                    if already_revealed {
                                                        *current = None;
                                                    } else {
                                                        *current = Some(value);
                                                    }
                                                });
                                            })
                                            on_set_system=Box::new(move || set_system_account(id_for_set_system.clone()))
                                            on_remove=Box::new(move || remove_account(id_for_remove.clone()))
                                            on_reauth=Box::new(move || reauthenticate_account(id_for_reauth_action.clone()))
                                        />
                                    }
                                }
                            />
                        </div>
                    }
                    .into_any()
                }
            }}

            <p class="mt-5 text-[11px] text-muted-foreground">
                {move || {
                    if any_loading.get() || is_listing.get() {
                        "Refreshing...".to_string()
                    } else if snapshot_stale.get() {
                        "Cached data".to_string()
                    } else {
                        match latest_updated_at.get() {
                            Some(ts) => format!("Last refreshed {}", format_time_ago(ts)),
                            None => "Not refreshed".to_string(),
                        }
                    }
                }}
            </p>
            </div>
        </main>
                    }
                    .into_any(),
                    ProviderPage::Anthropic => view! { <ComingSoonPage/> }.into_any(),
                }}
            </div>
        </div>
    }
}

#[component]
fn ProviderNav<A>(active: A, on_select: Box<dyn Fn(ProviderPage) + Send + Sync>) -> impl IntoView
where
    A: Fn() -> ProviderPage + Send + Sync + 'static,
{
    let active = StoredValue::new(active);
    let on_select = StoredValue::new(on_select);

    view! {
        <div class="flex shrink-0 items-center gap-1.5">
            <div class="flex min-w-0 items-center gap-1.5" role="tablist" aria-label="Provider">
                <ProviderNavButton
                    page=ProviderPage::Codex
                    active=move || active.with_value(|f| f()) == ProviderPage::Codex
                    on_select=Box::new(move |page| on_select.with_value(|f| f(page)))
                />
                <ProviderNavButton
                    page=ProviderPage::Anthropic
                    active=move || active.with_value(|f| f()) == ProviderPage::Anthropic
                    on_select=Box::new(move |page| on_select.with_value(|f| f(page)))
                />
            </div>
        </div>
    }
}

#[component]
fn ProviderNavButton<A>(
    page: ProviderPage,
    active: A,
    on_select: Box<dyn Fn(ProviderPage) + Send + Sync>,
) -> impl IntoView
where
    A: Fn() -> bool + Send + Sync + 'static,
{
    let active = StoredValue::new(active);
    let on_select = StoredValue::new(on_select);
    let label = page.label();

    view! {
        <Tooltip>
            <button
                class=move || if active.with_value(|f| f()) {
                    "inline-flex size-10 items-center justify-center rounded-md border border-primary bg-accent text-foreground shadow-xs transition-colors"
                } else {
                    "inline-flex size-10 items-center justify-center rounded-md border border-border bg-background text-muted-foreground shadow-xs transition-colors hover:cursor-pointer hover:bg-accent hover:text-foreground"
                }
                type="button"
                role="tab"
                aria-selected=move || active.with_value(|f| f()).to_string()
                aria-label=label
                on:click=move |_| on_select.with_value(|f| f(page))
            >
                {match page {
                    ProviderPage::Codex => view! {
                        <>
                            <img src="/public/openai-black.svg" class="size-7 shrink-0 dark:hidden" alt=""/>
                            <img src="/public/openai-white.svg" class="hidden size-7 shrink-0 dark:block" alt=""/>
                        </>
                    }
                    .into_any(),
                    ProviderPage::Anthropic => view! {
                        <>
                            <img src="/public/anthropic-black.svg" class="size-7 shrink-0 dark:hidden" alt=""/>
                            <img src="/public/anthropic-white.svg" class="hidden size-7 shrink-0 dark:block" alt=""/>
                        </>
                    }
                    .into_any(),
                }}
            </button>
            <TooltipContent position=TooltipPosition::Bottom align=TooltipAlign::Start>
                {label}
            </TooltipContent>
        </Tooltip>
    }
}

#[component]
fn ComingSoonPage() -> impl IntoView {
    view! {
        <main class="flex min-h-0 flex-1 items-center justify-center overflow-hidden">
            <div class="grid justify-items-center gap-3 text-center">
                <img src="/public/anthropic-black.svg" class="size-12 dark:hidden" alt="Anthropic"/>
                <img src="/public/anthropic-white.svg" class="hidden size-12 dark:block" alt="Anthropic"/>
                <div class="grid gap-1">
                    <h1 class="text-sm font-semibold leading-none">"Claude Code"</h1>
                    <p class="text-xs text-muted-foreground">"Coming Soon"</p>
                </div>
            </div>
        </main>
    }
}

#[component]
fn AutoSwitchStatus<E, D, T>(
    enabled: E,
    disabled: D,
    estimate: T,
    on_change: Box<dyn Fn(bool) + Send + Sync>,
) -> impl IntoView
where
    E: Fn() -> bool + Send + Sync + 'static,
    D: Fn() -> bool + Send + Sync + 'static,
    T: Fn() -> Option<AutoSwitchRunwayEstimate> + Send + Sync + 'static,
{
    let enabled = StoredValue::new(enabled);
    let disabled = StoredValue::new(disabled);
    let estimate = StoredValue::new(estimate);
    let on_change = StoredValue::new(on_change);

    view! {
        <Tooltip>
            <label class="inline-flex h-9 max-w-full shrink-0 cursor-pointer items-center gap-2 rounded-md border border-border bg-background px-2.5 text-xs font-medium text-foreground shadow-xs transition-colors hover:bg-accent">
                <Checkbox
                    checked=move || enabled.with_value(|f| f())
                    disabled=move || disabled.with_value(|f| f())
                    on_change=move |checked| on_change.with_value(|f| f(checked))
                />
                <span>"Auto switch"</span>
                <span class="rounded-sm bg-secondary px-1.5 py-0.5 text-[11px] text-muted-foreground">
                    {move || estimate.with_value(|f| f())
                        .map(|estimate| format!("Pool {}", format_usage_days(estimate.days_until_limit)))
                        .unwrap_or_else(|| "Pool n/a".to_string())
                    }
                </span>
            </label>
            <TooltipContent
                class="w-60 whitespace-normal text-left leading-5"
                position=TooltipPosition::Bottom
                align=TooltipAlign::End
            >
                {move || estimate.with_value(|f| f())
                    .map(|estimate| {
                        format!(
                            "Auto-switch pool: {} across {} managed accounts.",
                            format_usage_days(estimate.days_until_limit),
                            estimate.account_count,
                        )
                    })
                    .unwrap_or_else(|| {
                        "Auto-switch pool estimate needs weekly usage on a managed account.".to_string()
                    })
                }
            </TooltipContent>
        </Tooltip>
    }
}

#[component]
fn UsageRunway(estimate: UsageRunwayEstimate) -> impl IntoView {
    view! {
        <div class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 rounded-sm bg-secondary px-2 py-1 text-[11px] text-muted-foreground">
            <span class="font-medium text-foreground">
                {format!("Estimated {} usage left", format_usage_days(estimate.days_until_limit))}
            </span>
        </div>
    }
}

#[component]
fn QuotaEventCard<H, R>(
    event: QuotaEvent,
    hide_credentials: H,
    is_credential_revealed: R,
    on_reveal_credential: Box<dyn Fn(String) + Send + Sync>,
    on_dismiss: Box<dyn Fn(String) + Send + Sync>,
) -> impl IntoView
where
    H: Fn() -> bool + Send + Sync + 'static,
    R: Fn(&str) -> bool + Send + Sync + 'static,
{
    let event_id = event.id.clone();
    let event_kind = quota_event_kind_label(&event.kind);
    let event_class = quota_event_class(&event.severity);
    let body_suffix = quota_event_body_suffix(&event);
    let meta_suffix = quota_event_meta_suffix(&event);
    let account_label_for_body = event.account_label.clone();
    let account_label_for_meta = event.account_label.clone();
    let full_detail_title = format!(
        "{} - {} - {}",
        event.account_id, event.window_key, event.window_label
    );
    let redacted_detail_title = format!("{} - {}", event.window_key, event.window_label);
    let hide_credentials = StoredValue::new(hide_credentials);
    let is_credential_revealed = StoredValue::new(is_credential_revealed);
    let on_reveal_credential = StoredValue::new(on_reveal_credential);

    view! {
        <div
            class=event_class
            title=move || if hide_credentials.with_value(|f| f()) {
                redacted_detail_title.clone()
            } else {
                full_detail_title.clone()
            }
        >
            <div class="min-w-0">
                <div class="flex flex-wrap items-center gap-2">
                    <Badge variant=BadgeVariant::Outline size=BadgeSize::Sm class="rounded-full border-current/30 uppercase tracking-wide">
                        {event_kind}
                    </Badge>
                    <strong class="text-sm font-semibold leading-5">{event.title}</strong>
                </div>
                <p class="mt-1 text-xs leading-5">
                    <CredentialText
                        value=move || account_label_for_body.clone()
                        hide_credentials=move || hide_credentials.with_value(|f| f())
                        is_revealed=move |value| is_credential_revealed.with_value(|f| f(value))
                        on_reveal=Box::new(move |value| on_reveal_credential.with_value(|f| f(value)))
                    />
                    <span>{body_suffix}</span>
                </p>
                <p class="mt-1 text-[11px] opacity-75">
                    <CredentialText
                        value=move || account_label_for_meta.clone()
                        hide_credentials=move || hide_credentials.with_value(|f| f())
                        is_revealed=move |value| is_credential_revealed.with_value(|f| f(value))
                        on_reveal=Box::new(move |value| on_reveal_credential.with_value(|f| f(value)))
                    />
                    <span>{meta_suffix}</span>
                </p>
            </div>
            <button
                class="ml-auto inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-transparent text-sm leading-none opacity-70 hover:cursor-pointer hover:border-current/20 hover:opacity-100"
                type="button"
                aria-label="Dismiss quota notification"
                title="Dismiss quota notification"
                on:click=move |_| on_dismiss(event_id.clone())
            >
                <X class="size-3.5"/>
            </button>
        </div>
    }
}

#[component]
fn CredentialText<T, H, R>(
    value: T,
    hide_credentials: H,
    is_revealed: R,
    on_reveal: Box<dyn Fn(String) + Send + Sync>,
) -> impl IntoView
where
    T: Fn() -> String + Send + Sync + 'static,
    H: Fn() -> bool + Send + Sync + 'static,
    R: Fn(&str) -> bool + Send + Sync + 'static,
{
    let value = StoredValue::new(value);
    let hide_credentials = StoredValue::new(hide_credentials);
    let is_revealed = StoredValue::new(is_revealed);
    let on_reveal = StoredValue::new(on_reveal);

    view! {
        {move || {
            let text = value.with_value(|f| f());
            let privacy_enabled = hide_credentials.with_value(|f| f());
            let revealed = is_revealed.with_value(|f| f(&text));

            if privacy_enabled {
                let toggle_value = text.clone();
                let class = if revealed {
                    "inline-flex max-w-full items-center truncate rounded-sm text-left align-middle leading-none transition hover:cursor-pointer focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
                } else {
                    "inline-flex max-w-full items-center truncate rounded-sm text-left align-middle leading-none blur-[3px] transition hover:cursor-pointer focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
                };
                let aria_label = if revealed {
                    "Hide credential"
                } else {
                    "Reveal hidden credential"
                };
                view! {
                    <button
                        class=class
                        type="button"
                        aria-label=aria_label
                        title=aria_label
                        on:click=move |event| {
                            event.stop_propagation();
                            on_reveal.with_value(|f| f(toggle_value.clone()));
                        }
                    >
                        {text}
                    </button>
                }
                .into_any()
            } else {
                view! {
                    <span class="inline-flex max-w-full items-center truncate align-middle leading-none">{text}</span>
                }
                .into_any()
            }
        }}
    }
}

#[component]
fn CostSummary(usage: CostUsageSnapshot) -> impl IntoView {
    let today_key = utc_day_key(usage.updated_at);
    let today_detail = usage
        .daily
        .iter()
        .find(|point| point.day_key == today_key)
        .map(CostUsageBreakdown::from_daily_point)
        .unwrap_or_else(CostUsageBreakdown::empty);
    let last_30_days_detail = CostUsageBreakdown::from_daily_points(&usage.daily);

    view! {
        <div class="mb-3 flex items-start gap-2 rounded-md border border-border bg-secondary p-2.5">
            <div class="grid min-w-0 flex-1 grid-cols-2 gap-3">
                <CostMetric
                    label="Today"
                    tokens=usage.today_tokens
                    cost=usage.today_cost_usd
                    detail=today_detail
                />
                <CostMetric
                    label="Last 30 days"
                    tokens=usage.last_30_days_tokens
                    cost=usage.last_30_days_cost_usd
                    detail=last_30_days_detail
                />
            </div>
        </div>
    }
}

#[component]
fn CostMetric(
    label: &'static str,
    tokens: i64,
    cost: Option<f64>,
    detail: CostUsageBreakdown,
) -> impl IntoView {
    let input_detail = format!(
        "{} input ({} cached)",
        format_tokens(detail.input_tokens),
        format_tokens(detail.cached_input_tokens),
    );
    let output_detail = format!("{} output", format_tokens(detail.output_tokens));
    let total_detail = format!("{} total", format_tokens(detail.total_tokens));
    let tooltip_label = format!("{label} pricing details");

    view! {
        <div class="flex min-w-0 items-center gap-2 whitespace-nowrap">
            <p class="shrink-0 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">{label}</p>
            <div class="flex min-w-0 items-baseline gap-2">
                <strong class="shrink-0 text-sm font-semibold leading-none">{format_cost(cost)}</strong>
                <span class="min-w-0 truncate text-xs text-muted-foreground">{format_tokens(tokens)}</span>
            </div>
            <Tooltip class="shrink-0">
                <button
                    class="inline-flex size-5 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:cursor-pointer hover:bg-accent hover:text-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
                    type="button"
                    aria-label=tooltip_label
                >
                    <Info class="size-3"/>
                </button>
                <TooltipContent
                    class="w-52 whitespace-normal text-left leading-5"
                    position=TooltipPosition::Bottom
                    align=TooltipAlign::Start
                >
                    <div class="grid gap-1">
                        <p class="font-medium">"Pricing details"</p>
                        <p>{input_detail.clone()}</p>
                        <p>{output_detail.clone()}</p>
                        <p>{total_detail.clone()}</p>
                    </div>
                </TooltipContent>
            </Tooltip>
        </div>
    }
}

#[component]
fn AccountRow<T, M, S, C, X, U, E, L, R, D, H, V>(
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

    view! {
        <Card size=CardSize::Sm class="border-0 shadow-none">
            <div class="flex items-center justify-between gap-3">
                <div class="flex min-w-0 flex-wrap items-center gap-2">
                    <h2 class="flex h-5 min-w-0 items-center truncate font-mono text-sm font-medium leading-none tracking-normal">
                        <CredentialText
                            value=label_call
                            hide_credentials=move || hide_credentials.with_value(|f| f())
                            is_revealed=move |value| is_credential_revealed.with_value(|f| f(value))
                            on_reveal=Box::new(move |value| on_reveal_credential.with_value(|f| f(value)))
                        />
                    </h2>
                    {move || is_live_system_call().then(|| view! {
                        <Badge variant=BadgeVariant::Outline size=BadgeSize::Sm class="h-5 shrink-0 leading-none uppercase tracking-wide">
                            "System"
                        </Badge>
                    })}
                    {move || plan_label().map(|plan| view! {
                        <Badge variant=BadgeVariant::Outline size=BadgeSize::Sm class="h-5 shrink-0 leading-none uppercase tracking-wide">
                            {plan}
                        </Badge>
                    })}
                    {move || usage_source().map(|source| view! {
                        <Badge variant=BadgeVariant::Outline size=BadgeSize::Sm class="h-5 shrink-0 leading-none uppercase tracking-wide">
                            {source}
                        </Badge>
                    })}
                </div>
                <div class="flex shrink-0 items-center gap-2">
                    {move || can_set_system_call().then(|| view! {
                        <Tooltip>
                        <button
                            class=ButtonClass { variant: ButtonVariant::Outline, size: ButtonSize::Sm }.with_class("")
                            type="button"
                            aria-label="Set as system account"
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

            <div class="grid gap-3">
                {move || primary().map(|window| view! { <UsageMeter window=window/> })}
                {move || secondary().map(|window| view! { <UsageMeter window=window/> })}
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
                            <p class="text-xs text-muted-foreground">
                                "No usage yet."
                            </p>
                        }.into_any()
                    }
                }}
            </div>

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

            {move || credits().and_then(render_credits)}

            {move || error.with_value(|f| f()).map(|message| view! {
                <p class="mt-2 text-xs font-medium text-[var(--critical)]">{message}</p>
            })}
        </Card>
    }
}

#[component]
fn UsageMeter(window: UsageWindow) -> impl IntoView {
    let used = window.used_percent.clamp(0.0, 100.0);
    let width = format!("width: {:.1}%;", used);
    let fill_class = usage_meter_fill_class(used);
    let label = window.label.clone();
    let reset = window
        .reset_at
        .map(format_remaining_time)
        .unwrap_or_else(|| "Reset unavailable".to_string());

    view! {
        <div class="grid gap-1.5">
            <div class="flex justify-between gap-3 text-xs text-foreground">
                <span>{label.clone()}</span>
                <strong>{format!("{:.0}% used", used)}</strong>
            </div>
            <div
                class="relative h-3 w-full overflow-hidden rounded-full bg-secondary"
                role="progressbar"
                aria-label={format!("{} usage", label)}
                aria-valuemin="0"
                aria-valuemax="100"
                aria-valuenow={format!("{:.0}", used)}
            >
                <div class=fill_class style=width></div>
            </div>
            <div class="flex justify-between gap-3 text-[11px] text-muted-foreground">
                <span>{format!("{:.0}% remaining", window.remaining_percent.clamp(0.0, 100.0))}</span>
                <span>{reset}</span>
            </div>
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

fn weekly_runway_estimate(usage: &UsageSnapshot) -> Option<UsageRunwayEstimate> {
    usage.secondary.as_ref().and_then(weekly_window_estimate)
}

fn weekly_window_estimate(window: &UsageWindow) -> Option<UsageRunwayEstimate> {
    let reset_at = window.reset_at?;
    let window_seconds = window.window_seconds.unwrap_or(7 * 86_400);
    if window_seconds <= 0 {
        return None;
    }

    let used_percent = finite_percent(window.used_percent)?;
    if used_percent <= 0.0 {
        return None;
    }

    let remaining_percent = finite_percent(window.remaining_percent)?;
    let now = js_sys::Date::now() / 1000.0;
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

fn auto_switch_runway_estimate(
    accounts: &[AccountSummary],
    usage_by_id: &HashMap<String, UsageSnapshot>,
    errors_by_id: &HashMap<String, String>,
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

fn finite_percent(value: f64) -> Option<f64> {
    value.is_finite().then(|| value.clamp(0.0, 100.0))
}

fn format_usage_days(days: f64) -> String {
    if !days.is_finite() {
        return "n/a".to_string();
    }

    if days < 0.1 {
        "<0.1 day".to_string()
    } else {
        let rounded = if days < 10.0 {
            (days * 10.0).round() / 10.0
        } else {
            days.round()
        };
        let amount = if rounded < 10.0 && rounded.fract() != 0.0 {
            format!("{rounded:.1}")
        } else {
            format!("{rounded:.0}")
        };
        let unit = if amount == "1" { "day" } else { "days" };
        format!("{amount} {unit}")
    }
}
