use std::collections::{HashMap, HashSet};

use crate::app_actions::{
    AccountActions, NotificationActions, SettingsActions, SnapshotActions, UpdateActions,
};
use crate::app_context::{AppUiState, CodexOverviewState, SettingsState};
use crate::auto_switch_runway::auto_switch_runway_estimate;
use crate::codex_api::{
    js_command_error, listen_tauri, AccountIssue, AccountSummary, AppUpdateInfo, AppUpdateProgress,
    CodexOverviewSnapshot, CodexSettings, CodexUsageSourceMode, CostUsageSnapshot,
    NotificationStatus, QuotaEvent, UsageSnapshot,
};
use crate::components::nav::AppNav;
use crate::components::settings_panel::{SettingsPanel, SettingsPanelActions, SettingsPanelState};
use crate::components::update_banner::UpdateBanner;
use crate::request_epoch::RequestEpoch;
use crate::resize_guard::install_resize_transition_guard;
use crate::theme::{current_theme_preference, set_theme_preference, ThemeMode};
use crate::ui::alert::{Alert, AlertDescription};
use crate::views::claude_page::ClaudePage;
use crate::views::codex_page::{CodexPage, CodexPageActions, CodexPageData};
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

#[component]
pub fn App() -> impl IntoView {
    install_resize_transition_guard();

    let initial_theme_mode = current_theme_preference();
    let (active_provider, set_active_provider) = signal(ProviderPage::Codex);
    let (accounts, set_accounts) = signal::<Vec<AccountSummary>>(Vec::new());
    let (usage_by_id, set_usage_by_id) = signal::<HashMap<String, UsageSnapshot>>(HashMap::new());
    let (errors_by_id, set_errors_by_id) = signal::<HashMap<String, AccountIssue>>(HashMap::new());
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
    let (claude_accounts, set_claude_accounts) = signal::<Vec<AccountSummary>>(Vec::new());
    let (claude_usage_by_id, set_claude_usage_by_id) =
        signal::<HashMap<String, UsageSnapshot>>(HashMap::new());
    let (claude_errors_by_id, set_claude_errors_by_id) =
        signal::<HashMap<String, AccountIssue>>(HashMap::new());
    let (claude_quota_events, set_claude_quota_events) = signal::<Vec<QuotaEvent>>(Vec::new());
    let (claude_dismissed_quota_event_ids, set_claude_dismissed_quota_event_ids) =
        signal::<HashSet<String>>(HashSet::new());
    let (claude_loading_ids, set_claude_loading_ids) = signal::<HashSet<String>>(HashSet::new());
    let (claude_reauth_ids, set_claude_reauth_ids) = signal::<HashSet<String>>(HashSet::new());
    let (claude_usage_source_mode, set_claude_usage_source_mode) =
        signal(CodexUsageSourceMode::Auto);
    let (claude_cost_usage_enabled, set_claude_cost_usage_enabled) = signal(false);
    let (claude_notifications_enabled, set_claude_notifications_enabled) = signal(true);
    let (claude_auto_account_switching_enabled, set_claude_auto_account_switching_enabled) =
        signal(false);
    let (claude_hide_account_credentials, set_claude_hide_account_credentials) = signal(true);
    let (claude_cost_usage, set_claude_cost_usage) = signal::<Option<CostUsageSnapshot>>(None);
    let (claude_cost_error, set_claude_cost_error) = signal::<Option<String>>(None);
    let (claude_snapshot_generated_at, set_claude_snapshot_generated_at) =
        signal::<Option<i64>>(None);
    let (claude_snapshot_stale, set_claude_snapshot_stale) = signal(false);
    let (claude_initial_refresh_started, set_claude_initial_refresh_started) = signal(false);
    let (claude_is_listing, set_claude_is_listing) = signal(true);
    let (claude_is_account_action_loading, set_claude_is_account_action_loading) = signal(false);
    let (claude_is_account_login_loading, set_claude_is_account_login_loading) = signal(false);
    let (is_settings_loading, set_is_settings_loading) = signal(true);
    let (is_listing, set_is_listing) = signal(true);
    let (is_account_action_loading, set_is_account_action_loading) = signal(false);
    let (is_account_login_loading, set_is_account_login_loading) = signal(false);
    let (app_update, set_app_update) = signal::<Option<AppUpdateInfo>>(None);
    let (is_update_installing, set_is_update_installing) = signal(false);
    let (update_progress, set_update_progress) = signal::<Option<AppUpdateProgress>>(None);
    let (notification_status, set_notification_status) = signal::<Option<NotificationStatus>>(None);
    let (is_notification_test_sending, set_is_notification_test_sending) = signal(false);
    let (global_error, set_global_error) = signal::<Option<String>>(None);
    let snapshot_epoch = RequestEpoch::new();
    let settings_epoch = RequestEpoch::new();
    let notification_epoch = RequestEpoch::new();
    let update_epoch = RequestEpoch::new();
    let account_epoch = RequestEpoch::new();
    let claude_snapshot_epoch = RequestEpoch::new();
    let claude_settings_epoch = RequestEpoch::new();
    let claude_account_epoch = RequestEpoch::new();

    let active_is_listing = Signal::derive(move || match active_provider.get() {
        ProviderPage::Codex => is_listing.get(),
        ProviderPage::Anthropic => claude_is_listing.get(),
    });
    let active_accounts = Signal::derive(move || match active_provider.get() {
        ProviderPage::Codex => accounts.get(),
        ProviderPage::Anthropic => claude_accounts.get(),
    });
    let active_usage_by_id = Signal::derive(move || match active_provider.get() {
        ProviderPage::Codex => usage_by_id.get(),
        ProviderPage::Anthropic => claude_usage_by_id.get(),
    });
    let active_errors_by_id = Signal::derive(move || match active_provider.get() {
        ProviderPage::Codex => errors_by_id.get(),
        ProviderPage::Anthropic => claude_errors_by_id.get(),
    });
    let active_loading_ids = Signal::derive(move || match active_provider.get() {
        ProviderPage::Codex => loading_ids.get(),
        ProviderPage::Anthropic => claude_loading_ids.get(),
    });
    let active_reauth_ids = Signal::derive(move || match active_provider.get() {
        ProviderPage::Codex => reauth_ids.get(),
        ProviderPage::Anthropic => claude_reauth_ids.get(),
    });
    let active_cost_usage = Signal::derive(move || match active_provider.get() {
        ProviderPage::Codex => cost_usage.get(),
        ProviderPage::Anthropic => claude_cost_usage.get(),
    });
    let active_cost_error = Signal::derive(move || match active_provider.get() {
        ProviderPage::Codex => cost_error.get(),
        ProviderPage::Anthropic => claude_cost_error.get(),
    });
    let active_snapshot_stale = Signal::derive(move || match active_provider.get() {
        ProviderPage::Codex => snapshot_stale.get(),
        ProviderPage::Anthropic => claude_snapshot_stale.get(),
    });
    let active_hide_account_credentials = Signal::derive(move || match active_provider.get() {
        ProviderPage::Codex => hide_account_credentials.get(),
        ProviderPage::Anthropic => claude_hide_account_credentials.get(),
    });

    provide_context(AppUiState {
        is_listing: active_is_listing,
    });
    provide_context(CodexOverviewState {
        accounts: active_accounts,
        usage_by_id: active_usage_by_id,
        errors_by_id: active_errors_by_id,
        loading_ids: active_loading_ids,
        reauth_ids: active_reauth_ids,
        cost_usage: active_cost_usage,
        cost_error: active_cost_error,
        snapshot_stale: active_snapshot_stale,
        revealed_credential: revealed_credential.into(),
    });
    provide_context(SettingsState {
        hide_account_credentials: active_hide_account_credentials,
    });

    let snapshot_actions = SnapshotActions {
        cached_snapshot_command: "get_cached_codex_snapshot",
        refresh_snapshot_command: "refresh_codex_snapshot",
        accounts,
        set_accounts,
        set_usage_by_id,
        set_errors_by_id,
        set_quota_events,
        set_dismissed_quota_event_ids,
        set_loading_ids,
        set_reauth_ids,
        set_cost_usage,
        set_cost_error,
        snapshot_generated_at,
        set_snapshot_generated_at,
        set_snapshot_stale,
        set_is_listing,
        set_global_error,
        snapshot_epoch,
    };
    let claude_snapshot_actions = SnapshotActions {
        cached_snapshot_command: "get_cached_claude_snapshot",
        refresh_snapshot_command: "refresh_claude_snapshot",
        accounts: claude_accounts,
        set_accounts: set_claude_accounts,
        set_usage_by_id: set_claude_usage_by_id,
        set_errors_by_id: set_claude_errors_by_id,
        set_quota_events: set_claude_quota_events,
        set_dismissed_quota_event_ids: set_claude_dismissed_quota_event_ids,
        set_loading_ids: set_claude_loading_ids,
        set_reauth_ids: set_claude_reauth_ids,
        set_cost_usage: set_claude_cost_usage,
        set_cost_error: set_claude_cost_error,
        snapshot_generated_at: claude_snapshot_generated_at,
        set_snapshot_generated_at: set_claude_snapshot_generated_at,
        set_snapshot_stale: set_claude_snapshot_stale,
        set_is_listing: set_claude_is_listing,
        set_global_error,
        snapshot_epoch: claude_snapshot_epoch,
    };
    let notification_actions = NotificationActions {
        notification_epoch,
        set_notification_status,
        is_notification_test_sending,
        set_is_notification_test_sending,
        set_global_error,
    };
    let settings_actions = SettingsActions {
        get_settings_command: "get_codex_settings",
        set_usage_source_command: "set_codex_usage_source_mode",
        set_cost_usage_command: "set_codex_cost_usage_enabled",
        set_notifications_command: "set_codex_notifications_enabled",
        set_auto_switching_command: "set_codex_auto_account_switching_enabled",
        set_hide_credentials_command: "set_codex_hide_account_credentials",
        include_launch_on_login: true,
        usage_source_mode,
        set_usage_source_mode,
        cost_usage_enabled,
        set_cost_usage_enabled,
        notifications_enabled,
        set_notifications_enabled,
        auto_account_switching_enabled,
        set_auto_account_switching_enabled,
        hide_account_credentials,
        set_hide_account_credentials,
        launch_on_login,
        set_launch_on_login,
        set_revealed_credential,
        set_is_settings_loading,
        set_global_error,
        settings_epoch,
        snapshot_actions,
        notification_actions,
    };
    let claude_settings_actions = SettingsActions {
        get_settings_command: "get_claude_settings",
        set_usage_source_command: "set_claude_usage_source_mode",
        set_cost_usage_command: "set_claude_cost_usage_enabled",
        set_notifications_command: "set_claude_notifications_enabled",
        set_auto_switching_command: "set_claude_auto_account_switching_enabled",
        set_hide_credentials_command: "set_claude_hide_account_credentials",
        include_launch_on_login: false,
        usage_source_mode: claude_usage_source_mode,
        set_usage_source_mode: set_claude_usage_source_mode,
        cost_usage_enabled: claude_cost_usage_enabled,
        set_cost_usage_enabled: set_claude_cost_usage_enabled,
        notifications_enabled: claude_notifications_enabled,
        set_notifications_enabled: set_claude_notifications_enabled,
        auto_account_switching_enabled: claude_auto_account_switching_enabled,
        set_auto_account_switching_enabled: set_claude_auto_account_switching_enabled,
        hide_account_credentials: claude_hide_account_credentials,
        set_hide_account_credentials: set_claude_hide_account_credentials,
        launch_on_login,
        set_launch_on_login,
        set_revealed_credential,
        set_is_settings_loading,
        set_global_error,
        settings_epoch: claude_settings_epoch,
        snapshot_actions: claude_snapshot_actions,
        notification_actions,
    };
    let update_actions = UpdateActions {
        update_epoch,
        set_app_update,
        set_is_update_installing,
        set_update_progress,
        set_global_error,
    };
    let account_actions = AccountActions {
        add_command: "add_codex_account",
        cancel_login_command: "cancel_codex_account_login",
        reauthenticate_command: "reauthenticate_codex_account",
        remove_command: "remove_codex_account",
        set_system_command: "set_system_codex_account",
        provider_label: "Codex",
        account_epoch,
        is_account_login_loading,
        set_is_account_action_loading,
        set_is_account_login_loading,
        set_global_error,
        set_usage_by_id,
        set_errors_by_id,
        set_loading_ids,
        set_reauth_ids,
        quota_events,
        set_quota_events,
        set_dismissed_quota_event_ids,
        snapshot_actions,
    };
    let claude_account_actions = AccountActions {
        add_command: "add_claude_account",
        cancel_login_command: "cancel_claude_account_login",
        reauthenticate_command: "reauthenticate_claude_account",
        remove_command: "remove_claude_account",
        set_system_command: "set_system_claude_account",
        provider_label: "Claude Code",
        account_epoch: claude_account_epoch,
        is_account_login_loading: claude_is_account_login_loading,
        set_is_account_action_loading: set_claude_is_account_action_loading,
        set_is_account_login_loading: set_claude_is_account_login_loading,
        set_global_error,
        set_usage_by_id: set_claude_usage_by_id,
        set_errors_by_id: set_claude_errors_by_id,
        set_loading_ids: set_claude_loading_ids,
        set_reauth_ids: set_claude_reauth_ids,
        quota_events: claude_quota_events,
        set_quota_events: set_claude_quota_events,
        set_dismissed_quota_event_ids: set_claude_dismissed_quota_event_ids,
        snapshot_actions: claude_snapshot_actions,
    };

    let change_theme_mode = move |mode: ThemeMode| {
        if mode == theme_mode.get_untracked() {
            return;
        }

        set_theme_mode.set(mode);
        if let Err(error) = set_theme_preference(mode.storage_value()) {
            set_global_error.set(Some(js_command_error(&error).user_message));
        }
    };

    let listen_for_snapshots = move || {
        let handler = Closure::<dyn FnMut(JsValue)>::new(move |event| {
            let payload = js_sys::Reflect::get(&event, &JsValue::from_str("payload"))
                .unwrap_or(JsValue::UNDEFINED);
            if let Ok(snapshot) = serde_wasm_bindgen::from_value::<CodexOverviewSnapshot>(payload) {
                snapshot_actions.apply_snapshot(snapshot);
                snapshot_actions.finish_listing();
                notification_actions.refresh_status();
            }
        });
        let function = handler.as_ref().unchecked_ref::<js_sys::Function>().clone();
        spawn_local(async move {
            match listen_tauri("codex:snapshot-updated", &function).await {
                Ok(_) => handler.forget(),
                Err(error) => set_global_error.set(Some(js_command_error(&error).user_message)),
            }
        });
    };

    let listen_for_claude_snapshots = move || {
        let handler = Closure::<dyn FnMut(JsValue)>::new(move |event| {
            let payload = js_sys::Reflect::get(&event, &JsValue::from_str("payload"))
                .unwrap_or(JsValue::UNDEFINED);
            if let Ok(snapshot) = serde_wasm_bindgen::from_value::<CodexOverviewSnapshot>(payload) {
                claude_snapshot_actions.apply_snapshot(snapshot);
                claude_snapshot_actions.finish_listing();
                notification_actions.refresh_status();
            }
        });
        let function = handler.as_ref().unchecked_ref::<js_sys::Function>().clone();
        spawn_local(async move {
            match listen_tauri("claude:snapshot-updated", &function).await {
                Ok(_) => handler.forget(),
                Err(error) => set_global_error.set(Some(js_command_error(&error).user_message)),
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
                settings_actions.apply(settings);
                if hides_credentials {
                    set_revealed_credential.set(None);
                }
            }
        });
        let function = handler.as_ref().unchecked_ref::<js_sys::Function>().clone();
        spawn_local(async move {
            match listen_tauri("codex:settings-updated", &function).await {
                Ok(_) => handler.forget(),
                Err(error) => set_global_error.set(Some(js_command_error(&error).user_message)),
            }
        });
    };

    let listen_for_claude_settings = move || {
        let handler = Closure::<dyn FnMut(JsValue)>::new(move |event| {
            let payload = js_sys::Reflect::get(&event, &JsValue::from_str("payload"))
                .unwrap_or(JsValue::UNDEFINED);
            if let Ok(settings) = serde_wasm_bindgen::from_value::<CodexSettings>(payload) {
                let hides_credentials = settings.hide_account_credentials
                    && !claude_hide_account_credentials.get_untracked();
                claude_settings_actions.apply(settings);
                if hides_credentials {
                    set_revealed_credential.set(None);
                }
            }
        });
        let function = handler.as_ref().unchecked_ref::<js_sys::Function>().clone();
        spawn_local(async move {
            match listen_tauri("claude:settings-updated", &function).await {
                Ok(_) => handler.forget(),
                Err(error) => set_global_error.set(Some(js_command_error(&error).user_message)),
            }
        });
    };

    let listen_for_update_progress = move || {
        let handler = Closure::<dyn FnMut(JsValue)>::new(move |event| {
            let payload = js_sys::Reflect::get(&event, &JsValue::from_str("payload"))
                .unwrap_or(JsValue::UNDEFINED);
            if let Ok(progress) = serde_wasm_bindgen::from_value::<AppUpdateProgress>(payload) {
                update_actions.apply_progress(progress);
            }
        });
        let function = handler.as_ref().unchecked_ref::<js_sys::Function>().clone();
        spawn_local(async move {
            match listen_tauri("app:update-progress", &function).await {
                Ok(_) => handler.forget(),
                Err(error) => set_global_error.set(Some(js_command_error(&error).user_message)),
            }
        });
    };

    settings_actions.load();
    claude_settings_actions.load();
    notification_actions.refresh_status();
    snapshot_actions.load_cached();
    claude_snapshot_actions.load_cached();
    listen_for_snapshots();
    listen_for_claude_snapshots();
    listen_for_settings();
    listen_for_claude_settings();
    listen_for_update_progress();
    snapshot_actions.refresh(false);
    update_actions.check();

    let select_provider = move |page| {
        if page != active_provider.get_untracked() {
            set_revealed_credential.set(None);
        }
        set_active_provider.set(page);
        if page == ProviderPage::Anthropic && !claude_initial_refresh_started.get_untracked() {
            set_claude_initial_refresh_started.set(true);
            claude_snapshot_actions.refresh(false);
        }
    };

    let any_loading = Memo::new(move |_| match active_provider.get() {
        ProviderPage::Codex => !loading_ids.get().is_empty(),
        ProviderPage::Anthropic => !claude_loading_ids.get().is_empty(),
    });
    let latest_updated_at = Memo::new(move |_| match active_provider.get() {
        ProviderPage::Codex => {
            let usage_updated_at = usage_by_id.with(|map| map.values().map(|s| s.updated_at).max());
            let cost_updated_at =
                cost_usage.with(|usage| usage.as_ref().map(|usage| usage.updated_at));
            [
                usage_updated_at,
                cost_updated_at,
                snapshot_generated_at.get(),
            ]
            .into_iter()
            .flatten()
            .max()
        }
        ProviderPage::Anthropic => {
            let usage_updated_at =
                claude_usage_by_id.with(|map| map.values().map(|s| s.updated_at).max());
            let cost_updated_at =
                claude_cost_usage.with(|usage| usage.as_ref().map(|usage| usage.updated_at));
            [
                usage_updated_at,
                cost_updated_at,
                claude_snapshot_generated_at.get(),
            ]
            .into_iter()
            .flatten()
            .max()
        }
    });
    let visible_quota_events = Memo::new(move |_| {
        let (events, dismissed) = match active_provider.get() {
            ProviderPage::Codex => (quota_events.get(), dismissed_quota_event_ids.get()),
            ProviderPage::Anthropic => (
                claude_quota_events.get(),
                claude_dismissed_quota_event_ids.get(),
            ),
        };
        events
            .into_iter()
            .filter(|event| !dismissed.contains(&event.id))
            .collect::<Vec<_>>()
    });
    let auto_switch_runway = Memo::new(move |_| {
        let (current_accounts, current_usage, current_errors) = match active_provider.get() {
            ProviderPage::Codex => (accounts.get(), usage_by_id.get(), errors_by_id.get()),
            ProviderPage::Anthropic => (
                claude_accounts.get(),
                claude_usage_by_id.get(),
                claude_errors_by_id.get(),
            ),
        };
        auto_switch_runway_estimate(&current_accounts, &current_usage, &current_errors)
    });
    let active_usage_source_mode = Signal::derive(move || match active_provider.get() {
        ProviderPage::Codex => usage_source_mode.get(),
        ProviderPage::Anthropic => claude_usage_source_mode.get(),
    });
    let active_cost_usage_enabled = Signal::derive(move || match active_provider.get() {
        ProviderPage::Codex => cost_usage_enabled.get(),
        ProviderPage::Anthropic => claude_cost_usage_enabled.get(),
    });
    let active_notifications_enabled = Signal::derive(move || match active_provider.get() {
        ProviderPage::Codex => notifications_enabled.get(),
        ProviderPage::Anthropic => claude_notifications_enabled.get(),
    });
    let active_auto_account_switching_enabled =
        Signal::derive(move || match active_provider.get() {
            ProviderPage::Codex => auto_account_switching_enabled.get(),
            ProviderPage::Anthropic => claude_auto_account_switching_enabled.get(),
        });

    view! {
        <div
            class="min-h-screen overflow-hidden bg-background text-foreground"
            on:click=move |_| {
                let should_hide = match active_provider.get_untracked() {
                    ProviderPage::Codex => hide_account_credentials.get_untracked(),
                    ProviderPage::Anthropic => claude_hide_account_credentials.get_untracked(),
                };
                if should_hide {
                    set_revealed_credential.set(None);
                }
            }
        >
            <div class="app-shell mx-auto flex h-screen w-full max-w-[960px] min-w-0 flex-col gap-3 px-3 py-3">
                <AppNav
                    active_provider=move || active_provider.get()
                    on_select_provider=Box::new(select_provider)
                    is_listing=move || active_is_listing.get()
                    is_account_login_loading=move || match active_provider.get() {
                        ProviderPage::Codex => is_account_login_loading.get(),
                        ProviderPage::Anthropic => claude_is_account_login_loading.get(),
                    }
                    any_action_in_flight=move || match active_provider.get() {
                        ProviderPage::Codex => is_account_action_loading.get(),
                        ProviderPage::Anthropic => claude_is_account_action_loading.get(),
                    }
                    any_loading=any_loading
                    is_settings_open=move || is_settings_open.get()
                    on_open_settings=Box::new(move || set_is_settings_open.set(true))
                    on_add_account=Box::new(move || match active_provider.get() {
                        ProviderPage::Codex => account_actions.add(),
                        ProviderPage::Anthropic => claude_account_actions.add(),
                    })
                    on_cancel_login=Box::new(move || match active_provider.get() {
                        ProviderPage::Codex => account_actions.cancel_login(),
                        ProviderPage::Anthropic => claude_account_actions.cancel_login(),
                    })
                    on_refresh=Box::new(move || match active_provider.get() {
                        ProviderPage::Codex => snapshot_actions.refresh(true),
                        ProviderPage::Anthropic => claude_snapshot_actions.refresh(true),
                    })
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
                        on_install=Box::new(move || update_actions.install())
                        on_dismiss=Box::new(move || set_app_update.set(None))
                    />
                })}
                {move || match active_provider.get() {
                    ProviderPage::Codex => view! {
                        <CodexPage
                            data={CodexPageData {
                                provider_label: "Codex",
                                login_hint: "Use the + button above to add an account, or run `codex login`.",
                                visible_quota_events,
                                any_loading,
                                latest_updated_at,
                                account_action_in_flight: is_account_action_loading,
                            }}
                            actions={CodexPageActions {
                                on_dismiss_quota_event: Box::new(move |event_id: String| {
                                    set_dismissed_quota_event_ids.update(|set| { set.insert(event_id); });
                                }),
                                on_reveal_credential: Box::new(move |value: String| {
                                    set_revealed_credential.update(|current| {
                                        let already_revealed = current.as_deref() == Some(value.as_str());
                                        if already_revealed {
                                            *current = None;
                                        } else {
                                            *current = Some(value);
                                        }
                                    });
                                }),
                                on_set_system: Box::new(move |account_id| account_actions.set_system(account_id)),
                                on_remove_account: Box::new(move |account_id| account_actions.remove(account_id)),
                                on_reauth: Box::new(move |account_id| account_actions.reauthenticate(account_id)),
                            }}
                        />
                    }.into_any(),
                    ProviderPage::Anthropic => view! {
                        <ClaudePage
                            data={CodexPageData {
                                provider_label: "Claude Code",
                                login_hint: "Use the + button above to add an account, or run `claude /login`.",
                                visible_quota_events,
                                any_loading,
                                latest_updated_at,
                                account_action_in_flight: claude_is_account_action_loading,
                            }}
                            actions={CodexPageActions {
                                on_dismiss_quota_event: Box::new(move |event_id: String| {
                                    set_claude_dismissed_quota_event_ids.update(|set| { set.insert(event_id); });
                                }),
                                on_reveal_credential: Box::new(move |value: String| {
                                    set_revealed_credential.update(|current| {
                                        let already_revealed = current.as_deref() == Some(value.as_str());
                                        if already_revealed {
                                            *current = None;
                                        } else {
                                            *current = Some(value);
                                        }
                                    });
                                }),
                                on_set_system: Box::new(move |account_id| claude_account_actions.set_system(account_id)),
                                on_remove_account: Box::new(move |account_id| claude_account_actions.remove(account_id)),
                                on_reauth: Box::new(move |account_id| claude_account_actions.reauthenticate(account_id)),
                            }}
                        />
                    }.into_any(),
                }}
            </div>
            <SettingsPanel
                state={SettingsPanelState {
                    is_open: is_settings_open,
                    theme_mode,
                    active_provider_label: Signal::derive(move || active_provider.get().label()),
                    usage_source_mode: active_usage_source_mode,
                    cost_usage_enabled: active_cost_usage_enabled,
                    notifications_enabled: active_notifications_enabled,
                    notification_status,
                    is_notification_test_sending,
                    hide_account_credentials: active_hide_account_credentials,
                    launch_on_login,
                    auto_account_switching_enabled: active_auto_account_switching_enabled,
                    auto_switch_runway,
                    is_settings_loading,
                    is_listing: active_is_listing,
                }}
                actions={SettingsPanelActions {
                    on_close: Box::new(move || set_is_settings_open.set(false)),
                    on_change_theme: Box::new(change_theme_mode),
                    on_change_usage_source: Box::new(move |mode| match active_provider.get() {
                        ProviderPage::Codex => settings_actions.change_usage_source_mode(mode),
                        ProviderPage::Anthropic => claude_settings_actions.change_usage_source_mode(mode),
                    }),
                    on_change_cost_usage: Box::new(move |enabled| match active_provider.get() {
                        ProviderPage::Codex => settings_actions.change_cost_usage_enabled(enabled),
                        ProviderPage::Anthropic => claude_settings_actions.change_cost_usage_enabled(enabled),
                    }),
                    on_change_notifications: Box::new(move |enabled| match active_provider.get() {
                        ProviderPage::Codex => settings_actions.change_notifications_enabled(enabled),
                        ProviderPage::Anthropic => claude_settings_actions.change_notifications_enabled(enabled),
                    }),
                    on_send_test_notification: Box::new(move || notification_actions.send_test()),
                    on_refresh_notification_status: Box::new(move || notification_actions.refresh_status()),
                    on_change_hide_credentials: Box::new(move |enabled| match active_provider.get() {
                        ProviderPage::Codex => settings_actions.change_hide_account_credentials(enabled),
                        ProviderPage::Anthropic => claude_settings_actions.change_hide_account_credentials(enabled),
                    }),
                    on_change_launch_on_login: Box::new(move |enabled| settings_actions.change_launch_on_login(enabled)),
                    on_change_auto_switching: Box::new(move |enabled| match active_provider.get() {
                        ProviderPage::Codex => settings_actions.change_auto_account_switching_enabled(enabled),
                        ProviderPage::Anthropic => claude_settings_actions.change_auto_account_switching_enabled(enabled),
                    }),
                }}
            />
        </div>
    }
}
