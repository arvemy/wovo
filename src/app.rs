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
use crate::components::settings_panel::SettingsPanel;
use crate::components::update_banner::UpdateBanner;
use crate::request_epoch::RequestEpoch;
use crate::resize_guard::install_resize_transition_guard;
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

    provide_context(AppUiState { is_listing });
    provide_context(CodexOverviewState {
        accounts,
        usage_by_id,
        errors_by_id,
        loading_ids,
        reauth_ids,
        cost_usage,
        cost_error,
        snapshot_stale,
        revealed_credential,
    });
    provide_context(SettingsState {
        hide_account_credentials,
    });

    let snapshot_actions = SnapshotActions {
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
    let notification_actions = NotificationActions {
        notification_epoch,
        set_notification_status,
        is_notification_test_sending,
        set_is_notification_test_sending,
        set_global_error,
    };
    let settings_actions = SettingsActions {
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
    let update_actions = UpdateActions {
        update_epoch,
        set_app_update,
        set_is_update_installing,
        set_update_progress,
        set_global_error,
    };
    let account_actions = AccountActions {
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
    notification_actions.refresh_status();
    snapshot_actions.load_cached();
    listen_for_snapshots();
    listen_for_settings();
    listen_for_update_progress();
    snapshot_actions.refresh(false);
    update_actions.check();

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
                    is_account_login_loading=move || is_account_login_loading.get()
                    any_action_in_flight=move || is_account_action_loading.get()
                    any_loading=any_loading
                    is_settings_open=move || is_settings_open.get()
                    on_open_settings=Box::new(move || set_is_settings_open.set(true))
                    on_add_account=Box::new(move || account_actions.add())
                    on_cancel_login=Box::new(move || account_actions.cancel_login())
                    on_refresh=Box::new(move || snapshot_actions.refresh(true))
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
                            visible_quota_events=visible_quota_events
                            any_loading=any_loading
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
                            on_set_system=Box::new(move |account_id| account_actions.set_system(account_id))
                            on_remove_account=Box::new(move |account_id| account_actions.remove(account_id))
                            on_reauth=Box::new(move |account_id| account_actions.reauthenticate(account_id))
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
                on_change_usage_source=Box::new(move |mode| settings_actions.change_usage_source_mode(mode))
                cost_usage_enabled=cost_usage_enabled
                on_change_cost_usage=Box::new(move |enabled| settings_actions.change_cost_usage_enabled(enabled))
                notifications_enabled=notifications_enabled
                on_change_notifications=Box::new(move |enabled| settings_actions.change_notifications_enabled(enabled))
                notification_status=notification_status
                is_notification_test_sending=is_notification_test_sending
                on_send_test_notification=Box::new(move || notification_actions.send_test())
                on_refresh_notification_status=Box::new(move || notification_actions.refresh_status())
                hide_account_credentials=hide_account_credentials
                on_change_hide_credentials=Box::new(move |enabled| settings_actions.change_hide_account_credentials(enabled))
                launch_on_login=launch_on_login
                on_change_launch_on_login=Box::new(move |enabled| settings_actions.change_launch_on_login(enabled))
                auto_account_switching_enabled=auto_account_switching_enabled
                on_change_auto_switching=Box::new(move |enabled| settings_actions.change_auto_account_switching_enabled(enabled))
                auto_switch_runway=auto_switch_runway
                is_settings_loading=is_settings_loading
                is_listing=is_listing
            />
        </div>
    }
}
