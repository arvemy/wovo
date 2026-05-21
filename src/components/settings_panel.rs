use crate::auto_switch_runway::AutoSwitchRunwayEstimate;
use crate::codex_api::{
    invoke_tauri, CodexUsageSourceMode, NotificationSettingsOpenResult, NotificationStatus,
};
use crate::formatting::{format_time_ago, format_usage_days};
use crate::theme::ThemeMode;
use crate::ui::button::{ButtonClass, ButtonSize, ButtonVariant};
use crate::ui::switch::Switch;
use icons::{Monitor, Moon, Sun, X};
use leptos::prelude::*;
use leptos::task::spawn_local;
use tw_merge::IntoTailwindClass;
use wasm_bindgen::{closure::Closure, JsCast, JsValue};

#[derive(Clone, Copy)]
pub(crate) struct SettingsPanelState {
    pub(crate) is_open: ReadSignal<bool>,
    pub(crate) theme_mode: ReadSignal<ThemeMode>,
    pub(crate) active_provider_label: Signal<&'static str>,
    pub(crate) usage_source_mode: Signal<CodexUsageSourceMode>,
    pub(crate) cost_usage_enabled: Signal<bool>,
    pub(crate) notifications_enabled: Signal<bool>,
    pub(crate) notification_status: ReadSignal<Option<NotificationStatus>>,
    pub(crate) is_notification_test_sending: ReadSignal<bool>,
    pub(crate) hide_account_credentials: Signal<bool>,
    pub(crate) launch_on_login: ReadSignal<bool>,
    pub(crate) auto_account_switching_enabled: Signal<bool>,
    pub(crate) auto_switch_runway: Memo<Option<AutoSwitchRunwayEstimate>>,
    pub(crate) is_settings_loading: ReadSignal<bool>,
    pub(crate) is_listing: Signal<bool>,
}

pub(crate) struct SettingsPanelActions {
    pub(crate) on_close: Box<dyn Fn() + Send + Sync>,
    pub(crate) on_change_theme: Box<dyn Fn(ThemeMode) + Send + Sync>,
    pub(crate) on_change_usage_source: Box<dyn Fn(CodexUsageSourceMode) + Send + Sync>,
    pub(crate) on_change_cost_usage: Box<dyn Fn(bool) + Send + Sync>,
    pub(crate) on_change_notifications: Box<dyn Fn(bool) + Send + Sync>,
    pub(crate) on_send_test_notification: Box<dyn Fn() + Send + Sync>,
    pub(crate) on_refresh_notification_status: Box<dyn Fn() + Send + Sync>,
    pub(crate) on_change_hide_credentials: Box<dyn Fn(bool) + Send + Sync>,
    pub(crate) on_change_launch_on_login: Box<dyn Fn(bool) + Send + Sync>,
    pub(crate) on_change_auto_switching: Box<dyn Fn(bool) + Send + Sync>,
}

#[component]
pub fn SettingsPanel(state: SettingsPanelState, actions: SettingsPanelActions) -> impl IntoView {
    let is_open = state.is_open;
    let theme_mode = state.theme_mode;
    let active_provider_label = state.active_provider_label;
    let usage_source_mode = state.usage_source_mode;
    let cost_usage_enabled = state.cost_usage_enabled;
    let notifications_enabled = state.notifications_enabled;
    let notification_status = state.notification_status;
    let is_notification_test_sending = state.is_notification_test_sending;
    let hide_account_credentials = state.hide_account_credentials;
    let launch_on_login = state.launch_on_login;
    let auto_account_switching_enabled = state.auto_account_switching_enabled;
    let auto_switch_runway = state.auto_switch_runway;
    let is_settings_loading = state.is_settings_loading;
    let is_listing = state.is_listing;
    let actions = StoredValue::new(actions);
    let (notification_settings_message, set_notification_settings_message) =
        signal::<Option<String>>(None);
    let (refresh_notification_status_on_focus, set_refresh_notification_status_on_focus) =
        signal(false);

    if let Some(window) = web_sys::window() {
        let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            if refresh_notification_status_on_focus.get_untracked() {
                set_refresh_notification_status_on_focus.set(false);
                actions.with_value(|actions| (actions.on_refresh_notification_status)());
            }
        });

        if window
            .add_event_listener_with_callback("focus", handler.as_ref().unchecked_ref())
            .is_ok()
        {
            handler.forget();
        }
    }

    let panel_class = move || {
        let base = "fixed top-0 right-0 z-50 flex h-full flex-col border-l border-border bg-background shadow-xl transition-transform duration-200 ease-in-out";
        if is_open.get() {
            format!("{base} translate-x-0")
        } else {
            format!("{base} translate-x-full")
        }
    };

    view! {
        // Backdrop — only rendered when open
        {move || is_open.get().then(|| view! {
            <div
                class="fixed inset-0 z-40 bg-background/60 backdrop-blur-[1px]"
                on:click=move |_| actions.with_value(|actions| (actions.on_close)())
            />
        })}

        // Slide-in panel — remains mounted for transition but is inert while hidden
        <div
            class=panel_class
            style="width: min(300px, 100vw);"
            aria-hidden=move || (!is_open.get()).to_string()
            aria-label="Settings"
            aria-modal=move || is_open.get().to_string()
            inert=move || (!is_open.get()).then_some("")
            role="dialog"
        >
            // Header
            <div class="flex h-12 shrink-0 items-center justify-between border-b border-border px-4">
                <span class="text-sm font-semibold">"Settings"</span>
                <button
                    class=ButtonClass { variant: ButtonVariant::Ghost, size: ButtonSize::Icon }.with_class("size-8 text-muted-foreground")
                    type="button"
                    aria-label="Close settings"
                    on:click=move |_| actions.with_value(|actions| (actions.on_close)())
                >
                    <X class="size-4"/>
                </button>
            </div>

            // Scrollable body
            <div class="min-h-0 flex-1 overflow-y-auto px-4 py-2">

                // ── Appearance ──
                <SettingsSection label="Appearance">
                    <div class="grid grid-cols-3 gap-1 rounded-md border border-border bg-secondary p-0.5">
                        {[ThemeMode::Light, ThemeMode::Dark, ThemeMode::Auto]
                            .into_iter()
                            .map(|mode| {
                                let selected = move || theme_mode.get() == mode;
                                view! {
                                    <button
                                        class=move || if selected() {
                                            "inline-flex items-center justify-center gap-1.5 rounded-sm bg-background px-2 py-1.5 text-[11px] font-semibold text-foreground shadow-xs transition-colors"
                                        } else {
                                            "inline-flex items-center justify-center gap-1.5 rounded-sm px-2 py-1.5 text-[11px] font-medium text-muted-foreground transition-colors hover:cursor-pointer hover:text-foreground"
                                        }
                                        type="button"
                                        aria-pressed=move || selected().to_string()
                                        on:click=move |_| {
                                            actions.with_value(|actions| (actions.on_change_theme)(mode))
                                        }
                                    >
                                        {match mode {
                                            ThemeMode::Light => view! { <Sun class="size-3"/> }.into_any(),
                                            ThemeMode::Dark => view! { <Moon class="size-3"/> }.into_any(),
                                            ThemeMode::Auto => view! { <Monitor class="size-3"/> }.into_any(),
                                        }}
                                        <span>{mode.label()}</span>
                                    </button>
                                }
                            })
                            .collect_view()
                        }
                    </div>
                </SettingsSection>

                <SettingsDivider/>

                // ── Data ──
                <SettingsSection label="Data">
                    <div class="mb-2">
                        <p class="mb-1.5 text-xs font-medium">{move || format!("{} usage source", active_provider_label.get())}</p>
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
                                            disabled=move || is_listing.get() || is_settings_loading.get()
                                            on:click=move |_| {
                                                actions.with_value(|actions| (actions.on_change_usage_source)(mode))
                                            }
                                        >
                                            {mode.label()}
                                        </button>
                                    }
                                })
                                .collect_view()
                            }
                        </div>
                    </div>
                    <SettingsToggleRow
                        label="Cost tracker"
                        description="Track local token cost"
                        checked=cost_usage_enabled
                        disabled=Signal::derive(move || is_listing.get() || is_settings_loading.get())
                        on_change=Callback::new(move |v| {
                            actions.with_value(|actions| (actions.on_change_cost_usage)(v))
                        })
                    />
                </SettingsSection>

                <SettingsDivider/>

                // ── Notifications ──
                <SettingsSection label="Notifications">
                    <SettingsToggleRow
                        label="Notifications"
                        description="Quota and auto-switch alerts"
                        checked=notifications_enabled
                        disabled=is_settings_loading
                        on_change=Callback::new(move |v| {
                            actions.with_value(|actions| (actions.on_change_notifications)(v))
                        })
                    />
                    {move || notification_status.get().map(|status| {
                        let status_text = notification_status_text(&status);
                        let test_available = status.test_available;
                        let rationale_required = status.rationale_required;
                        let show_settings_action =
                            status.permission_state.is_denied() && status.settings_action_available;
                        view! {
                            <div class="mt-2 rounded-md border border-border bg-secondary/40 p-2">
                                <div class="flex items-start justify-between gap-2">
                                    <p class="min-w-0 text-[11px] leading-4 text-muted-foreground">
                                        {status_text}
                                    </p>
                                    {move || test_available.then(|| view! {
                                        <button
                                            class=ButtonClass { variant: ButtonVariant::Outline, size: ButtonSize::Sm }.with_class("h-7 shrink-0 px-2 text-[11px]")
                                            type="button"
                                            aria-label="Send test notification"
                                            aria-busy=move || is_notification_test_sending.get().to_string()
                                            disabled=move || is_notification_test_sending.get()
                                            on:click=move |_| {
                                                actions.with_value(|actions| (actions.on_send_test_notification)())
                                            }
                                        >
                                            {move || if is_notification_test_sending.get() { "Sending..." } else { "Send test" }}
                                        </button>
                                    })}
                                </div>
                                {move || rationale_required.then(|| view! {
                                    <p class="mt-2 text-[11px] leading-4 text-muted-foreground">
                                        "Wovo uses notifications only for quota and auto-switch alerts. Your OS may ask for permission before banners can appear."
                                    </p>
                                })}
                                {move || show_settings_action.then(|| view! {
                                    <div class="mt-2 flex flex-wrap items-center gap-2">
                                        <button
                                            class=ButtonClass { variant: ButtonVariant::Outline, size: ButtonSize::Sm }.with_class("h-7 px-2 text-[11px]")
                                            type="button"
                                            on:click=move |_| {
                                                spawn_local(async move {
                                                    let result = invoke_tauri::<NotificationSettingsOpenResult>(
                                                        "open_notification_settings",
                                                        JsValue::UNDEFINED,
                                                    ).await;
                                                    let refresh_on_focus = result
                                                        .as_ref()
                                                        .map(|result| result.opened)
                                                        .unwrap_or(false);
                                                    let message = result
                                                        .map(|result| result.user_message)
                                                        .unwrap_or_else(|error| error.user_message);
                                                    set_notification_settings_message.set(Some(message));
                                                    if refresh_on_focus {
                                                        set_refresh_notification_status_on_focus.set(true);
                                                    }
                                                    actions.with_value(|actions| (actions.on_refresh_notification_status)());
                                                });
                                            }
                                        >
                                            "Go to Settings"
                                        </button>
                                        <p class="text-[11px] text-muted-foreground">
                                            "Enable notifications for WoVo in system settings."
                                        </p>
                                    </div>
                                })}
                                {move || notification_settings_message.get().map(|message| view! {
                                    <p class="mt-2 text-[11px] text-muted-foreground">{message}</p>
                                })}
                            </div>
                        }
                    })}
                </SettingsSection>

                <SettingsDivider/>

                // ── Privacy ──
                <SettingsSection label="Privacy">
                    <SettingsToggleRow
                        label="Hide credentials"
                        description="Mask account labels"
                        checked=hide_account_credentials
                        disabled=is_settings_loading
                        on_change=Callback::new(move |v| {
                            actions.with_value(|actions| (actions.on_change_hide_credentials)(v))
                        })
                    />
                </SettingsSection>

                <SettingsDivider/>

                // ── System ──
                <SettingsSection label="System">
                    <SettingsToggleRow
                        label="Launch at login"
                        description="Start minimized to tray"
                        checked=launch_on_login
                        disabled=is_settings_loading
                        on_change=Callback::new(move |v| {
                            actions.with_value(|actions| (actions.on_change_launch_on_login)(v))
                        })
                    />
                </SettingsSection>

                <SettingsDivider/>

                // ── Auto Switch ──
                <SettingsSection label="Auto Switch">
                    <SettingsToggleRow
                        label="Auto switch accounts"
                        description="Switch accounts automatically at 90% quota"
                        checked=auto_account_switching_enabled
                        disabled=is_settings_loading
                        on_change=Callback::new(move |v| {
                            actions.with_value(|actions| (actions.on_change_auto_switching)(v))
                        })
                    />

                    // Pool runway (always shown when available)
                    {move || auto_switch_runway.get().map(|est| view! {
                        <div class="mt-1 flex items-center justify-between border-t border-border py-2 text-[11px]">
                            <span class="text-muted-foreground">"Pool runway"</span>
                            <span class="font-semibold text-foreground">
                                {format_usage_days(est.days_until_limit)}
                                " across "
                                {est.account_count.to_string()}
                                " accounts"
                            </span>
                        </div>
                    })}
                </SettingsSection>

            </div>
        </div>
    }
}

fn notification_status_text(status: &NotificationStatus) -> String {
    if status.permission_state.is_denied() {
        return "Notifications are denied by the operating system.".to_string();
    }
    if status.permission_state.needs_rationale() {
        return "Notifications need permission before Wovo can show quota alerts.".to_string();
    }

    let diagnostics = &status.diagnostics;
    let Some(attempted_at) = diagnostics.last_attempt_at else {
        return "No notification attempt recorded yet.".to_string();
    };

    let title = diagnostics.last_title.as_deref().unwrap_or("Notification");
    let state = diagnostics.last_status.as_deref().unwrap_or("unknown");
    let elapsed = format_time_ago(attempted_at);

    if let Some(error) = diagnostics.last_error.as_deref() {
        format!("{title}: {state} - {error} - {elapsed}")
    } else {
        format!("{title}: {state} - {elapsed}")
    }
}

#[component]
fn SettingsSection(label: &'static str, children: Children) -> impl IntoView {
    view! {
        <div class="py-2">
            <p class="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                {label}
            </p>
            {children()}
        </div>
    }
}

#[component]
fn SettingsDivider() -> impl IntoView {
    view! { <div class="h-px bg-border"/> }
}

#[component]
fn SettingsToggleRow(
    label: &'static str,
    description: &'static str,
    #[prop(into)] checked: Signal<bool>,
    #[prop(into, optional)] disabled: Signal<bool>,
    on_change: Callback<bool>,
) -> impl IntoView {
    view! {
        <div class="flex items-center justify-between gap-3 py-2">
            <div class="min-w-0">
                <p class="text-sm font-medium leading-none">{label}</p>
                <p class="mt-0.5 text-[11px] text-muted-foreground">{description}</p>
            </div>
            <Switch
                checked=checked
                disabled=disabled
                aria_label=label
                on_checked_change=on_change
            />
        </div>
    }
}
