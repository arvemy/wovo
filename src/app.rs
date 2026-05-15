use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::codex_api::{
    account_action, invoke_tauri, js_command_error, listen_tauri, refresh_snapshot,
    AccountSourceKind, AccountSummary, AppUpdateInfo, AppUpdateProgress, CodexOverviewSnapshot,
    CodexSettings, CodexUsageSourceMode, CommandError, CostUsageSnapshot, NotificationStatus,
    QuotaEvent, SetAutoAccountSwitchingEnabledArgs, SetCostUsageEnabledArgs,
    SetHideAccountCredentialsArgs, SetLaunchOnLoginArgs, SetNotificationsEnabledArgs,
    SetUsageSourceModeArgs, UsageSnapshot,
};
use crate::components::account_card::weekly_runway_estimate;
use crate::components::nav::AppNav;
use crate::components::settings_panel::SettingsPanel;
use crate::components::update_banner::UpdateBanner;
use crate::formatting::{finite_percent, is_auth_failure_message};
use crate::theme::{current_theme_preference, set_theme_preference, ThemeMode};
use crate::ui::alert::{Alert, AlertDescription};
use crate::views::codex_page::CodexPage;
use crate::views::coming_soon::ComingSoonPage;
use icons::X;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderPage {
    Codex,
    Anthropic,
}

impl ProviderPage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Anthropic => "Claude Code",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AutoSwitchRunwayEstimate {
    pub days_until_limit: f64,
    pub account_count: usize,
}

#[component]
pub fn App() -> impl IntoView {
    install_resize_transition_guard();

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
    let (launch_on_login, set_launch_on_login) = signal(false);
    let (revealed_credential, set_revealed_credential) = signal::<Option<String>>(None);
    let (is_settings_open, set_is_settings_open) = signal(false);
    let (cost_usage, set_cost_usage) = signal::<Option<CostUsageSnapshot>>(None);
    let (cost_error, set_cost_error) = signal::<Option<String>>(None);
    let (snapshot_generated_at, set_snapshot_generated_at) = signal::<Option<i64>>(None);
    let (snapshot_stale, set_snapshot_stale) = signal(false);
    let (is_settings_loading, set_is_settings_loading) = signal(true);
    let (is_listing, set_is_listing) = signal(true);
    let (is_account_action_loading, set_is_account_action_loading) = signal(false);
    let (app_update, set_app_update) = signal::<Option<AppUpdateInfo>>(None);
    let (is_update_installing, set_is_update_installing) = signal(false);
    let (update_progress, set_update_progress) = signal::<Option<AppUpdateProgress>>(None);
    let (notification_status, set_notification_status) = signal::<Option<NotificationStatus>>(None);
    let (is_notification_test_sending, set_is_notification_test_sending) = signal(false);
    let (global_error, set_global_error) = signal::<Option<String>>(None);

    let apply_settings = move |settings: CodexSettings| {
        set_usage_source_mode.set(settings.usage_source_mode);
        set_cost_usage_enabled.set(settings.cost_usage_enabled);
        set_notifications_enabled.set(settings.notifications_enabled);
        set_auto_account_switching_enabled.set(settings.auto_account_switching_enabled);
        set_hide_account_credentials.set(settings.hide_account_credentials);
        set_launch_on_login.set(settings.launch_on_login);
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

    let refresh_notification_status = move || {
        spawn_local(async move {
            if let Ok(status) = invoke_tauri::<NotificationStatus>(
                "get_codex_notification_status",
                JsValue::UNDEFINED,
            )
            .await
            {
                set_notification_status.set(Some(status));
            }
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

    let send_test_notification = move || {
        if is_notification_test_sending.get_untracked() {
            return;
        }

        spawn_local(async move {
            set_is_notification_test_sending.set(true);
            set_global_error.set(None);
            match invoke_tauri::<NotificationStatus>(
                "send_codex_test_notification",
                JsValue::UNDEFINED,
            )
            .await
            {
                Ok(status) => set_notification_status.set(Some(status)),
                Err(error) => set_global_error.set(Some(error.message)),
            }
            set_is_notification_test_sending.set(false);
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

    let change_launch_on_login = move |enabled: bool| {
        if enabled == launch_on_login.get_untracked() {
            return;
        }
        let previous = launch_on_login.get_untracked();
        set_launch_on_login.set(enabled);
        spawn_local(async move {
            set_is_settings_loading.set(true);
            set_global_error.set(None);
            let args = serde_wasm_bindgen::to_value(&SetLaunchOnLoginArgs { enabled })
                .map_err(|error| CommandError::from_message(error.to_string()));
            match args {
                Ok(args) => {
                    match invoke_tauri::<CodexSettings>("set_codex_launch_on_login", args).await {
                        Ok(settings) => apply_settings(settings),
                        Err(error) => {
                            set_launch_on_login.set(previous);
                            set_global_error.set(Some(error.message));
                        }
                    }
                }
                Err(error) => {
                    set_launch_on_login.set(previous);
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
                refresh_notification_status();
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

    let listen_for_settings = move || {
        let handler = Closure::<dyn FnMut(JsValue)>::new(move |event| {
            let payload = js_sys::Reflect::get(&event, &JsValue::from_str("payload"))
                .unwrap_or(JsValue::UNDEFINED);
            if let Ok(settings) = serde_wasm_bindgen::from_value::<CodexSettings>(payload) {
                let hides_credentials =
                    settings.hide_account_credentials && !hide_account_credentials.get_untracked();
                apply_settings(settings);
                if hides_credentials {
                    set_revealed_credential.set(None);
                }
            }
        });
        let function = handler.as_ref().unchecked_ref::<js_sys::Function>().clone();
        spawn_local(async move {
            match listen_tauri("codex:settings-updated", &function).await {
                Ok(_) => handler.forget(),
                Err(error) => set_global_error.set(Some(js_command_error(&error).message)),
            }
        });
    };

    let listen_for_update_progress = move || {
        let handler = Closure::<dyn FnMut(JsValue)>::new(move |event| {
            let payload = js_sys::Reflect::get(&event, &JsValue::from_str("payload"))
                .unwrap_or(JsValue::UNDEFINED);
            if let Ok(progress) = serde_wasm_bindgen::from_value::<AppUpdateProgress>(payload) {
                let installed = progress.phase == "installed";
                set_update_progress.set(Some(progress));
                if installed {
                    set_is_update_installing.set(false);
                }
            }
        });
        let function = handler.as_ref().unchecked_ref::<js_sys::Function>().clone();
        spawn_local(async move {
            match listen_tauri("app:update-progress", &function).await {
                Ok(_) => handler.forget(),
                Err(error) => set_global_error.set(Some(js_command_error(&error).message)),
            }
        });
    };

    let check_for_app_update = move || {
        spawn_local(async move {
            if let Ok(update) =
                invoke_tauri::<Option<AppUpdateInfo>>("check_app_update", JsValue::UNDEFINED).await
            {
                set_app_update.set(update);
            }
        });
    };

    load_settings();
    refresh_notification_status();
    load_cached_snapshot();
    listen_for_snapshots();
    listen_for_settings();
    listen_for_update_progress();
    refresh_overview_snapshot(false);
    check_for_app_update();

    let install_app_update = move || {
        spawn_local(async move {
            set_is_update_installing.set(true);
            set_update_progress.set(None);
            set_global_error.set(None);

            match invoke_tauri::<()>("install_app_update", JsValue::UNDEFINED).await {
                Ok(()) => {}
                Err(error) => {
                    set_is_update_installing.set(false);
                    set_global_error.set(Some(error.message));
                }
            }
        });
    };
    let install_app_update = StoredValue::new(install_app_update);

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
            <div class="app-shell mx-auto flex h-screen w-full max-w-[960px] min-w-0 flex-col gap-3 px-3 py-3">
                <AppNav
                    active_provider=move || active_provider.get()
                    on_select_provider=Box::new(move |page| set_active_provider.set(page))
                    is_listing=move || is_listing.get()
                    is_account_action_loading=move || is_account_action_loading.get()
                    any_action_in_flight=move || is_account_action_loading.get()
                    any_loading=any_loading
                    is_settings_open=move || is_settings_open.get()
                    on_open_settings=Box::new(move || set_is_settings_open.set(true))
                    on_add_account=Box::new(add_account)
                    on_cancel_login=Box::new(cancel_account_login)
                    on_refresh=Box::new(move || refresh_overview_snapshot(true))
                />
                {move || global_error.get().map(|message| view! {
                    <Alert class="border-[var(--warning)] bg-[var(--warning-muted)] text-[var(--warning-foreground)]">
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
                })}
                {move || app_update.get().map(|update| view! {
                    <UpdateBanner
                        update=update
                        update_progress=update_progress
                        is_update_installing=is_update_installing
                        on_install=Box::new(move || install_app_update.with_value(|f| f()))
                        on_dismiss=Box::new(move || set_app_update.set(None))
                    />
                })}
                {move || match active_provider.get() {
                    ProviderPage::Codex => view! {
                        <CodexPage
                            accounts=accounts
                            usage_by_id=usage_by_id
                            errors_by_id=errors_by_id
                            loading_ids=loading_ids
                            reauth_ids=reauth_ids
                            visible_quota_events=visible_quota_events
                            cost_usage=cost_usage
                            cost_error=cost_error
                            hide_account_credentials=hide_account_credentials
                            revealed_credential=revealed_credential
                            snapshot_stale=snapshot_stale
                            any_loading=any_loading
                            is_listing=is_listing
                            latest_updated_at=latest_updated_at
                            any_action_in_flight=move || is_account_action_loading.get()
                            on_dismiss_quota_event=Box::new(move |event_id: String| {
                                set_dismissed_quota_event_ids.update(|set| { set.insert(event_id); });
                            })
                            on_reveal_credential=Box::new(move |value: String| {
                                set_revealed_credential.update(|current| {
                                    let already_revealed = current.as_deref() == Some(value.as_str());
                                    if already_revealed {
                                        *current = None;
                                    } else {
                                        *current = Some(value);
                                    }
                                });
                            })
                            on_set_system=Box::new(set_system_account)
                            on_remove_account=Box::new(remove_account)
                            on_reauth=Box::new(reauthenticate_account)
                        />
                    }.into_any(),
                    ProviderPage::Anthropic => view! { <ComingSoonPage/> }.into_any(),
                }}
            </div>
            <SettingsPanel
                is_open=is_settings_open
                on_close=Box::new(move || set_is_settings_open.set(false))
                theme_mode=theme_mode
                on_change_theme=Box::new(change_theme_mode)
                usage_source_mode=usage_source_mode
                on_change_usage_source=Box::new(change_usage_source_mode)
                cost_usage_enabled=cost_usage_enabled
                on_change_cost_usage=Box::new(change_cost_usage_enabled)
                notifications_enabled=notifications_enabled
                on_change_notifications=Box::new(change_notifications_enabled)
                notification_status=notification_status
                is_notification_test_sending=is_notification_test_sending
                on_send_test_notification=Box::new(send_test_notification)
                hide_account_credentials=hide_account_credentials
                on_change_hide_credentials=Box::new(change_hide_account_credentials)
                launch_on_login=launch_on_login
                on_change_launch_on_login=Box::new(change_launch_on_login)
                auto_account_switching_enabled=auto_account_switching_enabled
                on_change_auto_switching=Box::new(change_auto_account_switching_enabled)
                auto_switch_runway=auto_switch_runway
                is_settings_loading=is_settings_loading
                is_listing=is_listing
            />
        </div>
    }
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

fn install_resize_transition_guard() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let Some(root) = document.document_element() else {
        return;
    };

    let resize_timeout = Rc::new(RefCell::new(None::<(i32, Closure<dyn FnMut()>)>));
    let window_for_handler = window.clone();
    let root_for_handler = root.clone();
    let timeout_for_handler = Rc::clone(&resize_timeout);

    let handler = Closure::<dyn FnMut()>::new(move || {
        let _ = root_for_handler.class_list().add_1("is-window-resizing");

        if let Some((timeout_id, _callback)) = timeout_for_handler.borrow_mut().take() {
            window_for_handler.clear_timeout_with_handle(timeout_id);
        }

        let root_for_timeout = root_for_handler.clone();
        let timeout_for_timeout = Rc::clone(&timeout_for_handler);
        let timeout_callback = Closure::<dyn FnMut()>::new(move || {
            let _ = root_for_timeout.class_list().remove_1("is-window-resizing");
            timeout_for_timeout.borrow_mut().take();
        });

        if let Ok(timeout_id) = window_for_handler
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                timeout_callback.as_ref().unchecked_ref(),
                120,
            )
        {
            timeout_for_handler
                .borrow_mut()
                .replace((timeout_id, timeout_callback));
        }
    });
    let callback = handler.as_ref().unchecked_ref::<js_sys::Function>();

    if window
        .add_event_listener_with_callback("resize", callback)
        .is_ok()
    {
        handler.forget();
    }
}
