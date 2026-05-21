use crate::app_context::{AppUiState, CodexOverviewState, SettingsState};
use crate::codex_api::{AccountSourceKind, QuotaEvent};
use crate::components::account_card::AccountCard;
use crate::components::cost_summary::CostSummary;
use crate::components::quota_event_card::QuotaEventCard;
use crate::formatting::format_time_ago;
use crate::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::ui::skeleton::Skeleton;
use icons::LoaderCircle;
use leptos::prelude::*;

#[derive(Clone, Copy)]
pub(crate) struct CodexPageData {
    pub(crate) provider_label: &'static str,
    pub(crate) login_hint: &'static str,
    pub(crate) visible_quota_events: Memo<Vec<QuotaEvent>>,
    pub(crate) any_loading: Memo<bool>,
    pub(crate) latest_updated_at: Memo<Option<i64>>,
    pub(crate) account_action_in_flight: ReadSignal<bool>,
}

pub(crate) struct CodexPageActions {
    pub(crate) on_dismiss_quota_event: Box<dyn Fn(String) + Send + Sync>,
    pub(crate) on_reveal_credential: Box<dyn Fn(String) + Send + Sync>,
    pub(crate) on_set_system: Box<dyn Fn(String) + Send + Sync>,
    pub(crate) on_remove_account: Box<dyn Fn(String) + Send + Sync>,
    pub(crate) on_reauth: Box<dyn Fn(String) + Send + Sync>,
}

#[component]
pub fn CodexPage(data: CodexPageData, actions: CodexPageActions) -> impl IntoView {
    let visible_quota_events = data.visible_quota_events;
    let provider_label = data.provider_label;
    let login_hint = data.login_hint;
    let any_loading = data.any_loading;
    let latest_updated_at = data.latest_updated_at;
    let account_action_in_flight = data.account_action_in_flight;
    let actions = StoredValue::new(actions);
    let overview = expect_context::<CodexOverviewState>();
    let settings = expect_context::<SettingsState>();
    let ui = expect_context::<AppUiState>();
    let accounts = overview.accounts;
    let usage_by_id = overview.usage_by_id;
    let errors_by_id = overview.errors_by_id;
    let loading_ids = overview.loading_ids;
    let reauth_ids = overview.reauth_ids;
    let cost_usage = overview.cost_usage;
    let cost_error = overview.cost_error;
    let snapshot_stale = overview.snapshot_stale;
    let revealed_credential = overview.revealed_credential;
    let hide_account_credentials = settings.hide_account_credentials;
    let is_listing = ui.is_listing;

    view! {
        <main class="codex-page flex min-h-0 flex-1 flex-col overflow-visible">
            <div class="min-h-0 flex-1 overflow-y-auto overflow-x-hidden pr-1 pb-3">

                {move || cost_usage.get().map(|usage| view! { <CostSummary usage=usage/> })}

                {move || cost_error.get().map(|message| view! {
                    <p class="mb-3 text-xs font-medium text-[var(--critical)]">{message}</p>
                })}

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
                                        let event_id = event.id.clone();
                                        view! {
                                            <QuotaEventCard
                                                event=event
                                                hide_credentials=move || hide_account_credentials.get()
                                                is_credential_revealed=move |value| {
                                                    revealed_credential.with(|current| current.as_deref() == Some(value))
                                                }
                                                on_reveal_credential=Box::new(move |value| {
                                                    actions.with_value(|actions| (actions.on_reveal_credential)(value))
                                                })
                                                on_dismiss=Box::new(move |_| {
                                                    actions.with_value(|actions| (actions.on_dismiss_quota_event)(event_id.clone()))
                                                })
                                            />
                                        }
                                    }
                                />
                            </div>
                        }.into_any()
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
                                <div class="grid gap-3 py-2" aria-busy="true">
                                    <Skeleton class="h-24 w-full"/>
                                    <Skeleton class="h-24 w-full"/>
                                    <div class="flex items-center justify-center gap-2 py-4 text-center">
                                        <LoaderCircle class="size-4 animate-spin text-muted-foreground"/>
                                        <p class="text-xs font-medium text-muted-foreground">{format!("Checking {provider_label}")}</p>
                                    </div>
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <div class="flex flex-col items-center justify-center gap-2 py-12 text-center">
                                    <h2 class="text-sm font-semibold leading-none">{format!("No {provider_label} account found")}</h2>
                                    <p class="text-xs text-muted-foreground">
                                        {login_hint}
                                    </p>
                                </div>
                            }.into_any()
                        }
                    } else {
                        view! {
                            <div class="flex flex-col">
                                <For
                                    each=move || accounts.get()
                                    key=|account| account.id.clone()
                                    children=move |account| {
                                        let id_usage = account.id.clone();
                                        let id_error = account.id.clone();
                                        let id_loading = account.id.clone();
                                        let id_reauth = account.id.clone();
                                        let id_remove = account.id.clone();
                                        let id_set_system = account.id.clone();
                                        let id_reauth_action = account.id.clone();
                                        let id_label = account.id.clone();
                                        let id_source = account.id.clone();
                                        let id_live_system = account.id.clone();
                                        let id_can_set_system = account.id.clone();
                                        let id_can_remove = account.id.clone();
                                        let fallback_label = account.label.clone();
                                        let fallback_source = account.source.clone();
                                        let fallback_is_live_system = account.is_live_system;
                                        let fallback_can_set_system = account.can_set_system;
                                        let fallback_can_remove = account.can_remove;

                                        view! {
                                            <AccountCard
                                                label=move || accounts.with(|items| {
                                                    items.iter()
                                                        .find(|item| item.id == id_label)
                                                        .map(|item| item.label.clone())
                                                        .unwrap_or_else(|| fallback_label.clone())
                                                })
                                                is_managed=move || accounts.with(|items| {
                                                    items.iter()
                                                        .find(|item| item.id == id_source)
                                                        .map(|item| item.source == AccountSourceKind::Managed)
                                                        .unwrap_or(fallback_source == AccountSourceKind::Managed)
                                                })
                                                is_live_system=move || accounts.with(|items| {
                                                    items.iter()
                                                        .find(|item| item.id == id_live_system)
                                                        .map(|item| item.is_live_system)
                                                        .unwrap_or(fallback_is_live_system)
                                                })
                                                can_set_system=move || accounts.with(|items| {
                                                    items.iter()
                                                        .find(|item| item.id == id_can_set_system)
                                                        .map(|item| item.can_set_system)
                                                        .unwrap_or(fallback_can_set_system)
                                                })
                                                can_remove=move || accounts.with(|items| {
                                                    items.iter()
                                                        .find(|item| item.id == id_can_remove)
                                                        .map(|item| item.can_remove)
                                                        .unwrap_or(fallback_can_remove)
                                                })
                                                usage=move || usage_by_id.with(|map| map.get(&id_usage).cloned())
                                                error=move || errors_by_id.with(|map| {
                                                    map.get(&id_error).map(|issue| issue.user_message.clone())
                                                })
                                                is_loading=move || loading_ids.with(|set| set.contains(&id_loading))
                                                reauth_required=move || reauth_ids.with(|set| set.contains(&id_reauth))
                                                disabled=move || account_action_in_flight.get()
                                                hide_credentials=move || hide_account_credentials.get()
                                                is_credential_revealed=move |value| {
                                                    revealed_credential.with(|current| current.as_deref() == Some(value))
                                                }
                                                on_reveal_credential=Box::new(move |value| {
                                                    actions.with_value(|actions| (actions.on_reveal_credential)(value))
                                                })
                                                on_set_system=Box::new(move || {
                                                    actions.with_value(|actions| (actions.on_set_system)(id_set_system.clone()))
                                                })
                                                on_remove=Box::new(move || {
                                                    actions.with_value(|actions| (actions.on_remove_account)(id_remove.clone()))
                                                })
                                                on_reauth=Box::new(move || {
                                                    actions.with_value(|actions| (actions.on_reauth)(id_reauth_action.clone()))
                                                })
                                            />
                                        }
                                    }
                                />
                            </div>
                        }.into_any()
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
}
