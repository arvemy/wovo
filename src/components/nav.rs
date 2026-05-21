use crate::app::ProviderPage;
use crate::ui::button::{ButtonClass, ButtonSize, ButtonVariant};
use icons::{LoaderCircle, Plus, RefreshCw, SlidersHorizontal};
use leptos::prelude::*;
use tw_merge::IntoTailwindClass;

#[component]
pub fn AppNav<A, IL, ILL, ANFL, ISO>(
    active_provider: A,
    on_select_provider: Box<dyn Fn(ProviderPage) + Send + Sync>,
    is_listing: IL,
    is_account_login_loading: ILL,
    any_action_in_flight: ANFL,
    any_loading: Memo<bool>,
    is_settings_open: ISO,
    on_open_settings: Box<dyn Fn() + Send + Sync>,
    on_add_account: Box<dyn Fn() + Send + Sync>,
    on_cancel_login: Box<dyn Fn() + Send + Sync>,
    on_refresh: Box<dyn Fn() + Send + Sync>,
) -> impl IntoView
where
    A: Fn() -> ProviderPage + Send + Sync + 'static,
    IL: Fn() -> bool + Send + Sync + 'static,
    ILL: Fn() -> bool + Send + Sync + 'static,
    ANFL: Fn() -> bool + Send + Sync + 'static,
    ISO: Fn() -> bool + Send + Sync + 'static,
{
    let on_select_provider = StoredValue::new(on_select_provider);
    let active_provider = StoredValue::new(active_provider);
    let is_listing = StoredValue::new(is_listing);
    let is_account_login_loading = StoredValue::new(is_account_login_loading);
    let any_action_in_flight = StoredValue::new(any_action_in_flight);
    let is_settings_open = StoredValue::new(is_settings_open);
    let on_open_settings = StoredValue::new(on_open_settings);
    let on_add_account = StoredValue::new(on_add_account);
    let on_cancel_login = StoredValue::new(on_cancel_login);
    let on_refresh = StoredValue::new(on_refresh);

    view! {
        <nav class="flex h-12 shrink-0 items-center justify-between gap-3 border-b border-border">
            <ProviderNav
                active=move || active_provider.with_value(|f| f())
                on_select=Box::new(move |page| on_select_provider.with_value(|f| f(page)))
            />
            <div class="flex shrink-0 items-center gap-2">
                // Add account / cancel login
                <button
                    class=ButtonClass { variant: ButtonVariant::Outline, size: ButtonSize::Icon }.with_class("")
                    type="button"
                    aria-label=move || {
                        if is_account_login_loading.with_value(|f| f()) {
                            format!("Cancel {} login", active_provider.with_value(|f| f()).label())
                        } else {
                            format!("Add {} account", active_provider.with_value(|f| f()).label())
                        }
                    }
                    aria-busy=move || is_account_login_loading.with_value(|f| f()).to_string()
                    aria-disabled=move || {
                        let login_loading = is_account_login_loading.with_value(|f| f());
                        (!login_loading && (
                            is_listing.with_value(|f| f())
                                || any_action_in_flight.with_value(|f| f())
                        )).to_string()
                    }
                    disabled=move || {
                        let login_loading = is_account_login_loading.with_value(|f| f());
                        !login_loading && (
                            is_listing.with_value(|f| f())
                                || any_action_in_flight.with_value(|f| f())
                        )
                    }
                    on:click=move |_| {
                        if is_account_login_loading.with_value(|f| f()) {
                            on_cancel_login.with_value(|f| f());
                        } else if !is_listing.with_value(|f| f())
                            && !any_action_in_flight.with_value(|f| f())
                        {
                            on_add_account.with_value(|f| f());
                        }
                    }
                >
                    {move || if is_account_login_loading.with_value(|f| f()) {
                        view! { <LoaderCircle class="size-4 animate-spin"/> }.into_any()
                    } else {
                        view! { <Plus class="size-4"/> }.into_any()
                    }}
                </button>

                // Refresh all
                <button
                    class=ButtonClass { variant: ButtonVariant::Outline, size: ButtonSize::Icon }.with_class("")
                    type="button"
                    aria-label=move || format!("Refresh {} accounts", active_provider.with_value(|f| f()).label())
                    aria-busy=move || (is_listing.with_value(|f| f()) || any_loading.get()).to_string()
                    aria-disabled=move || (is_listing.with_value(|f| f()) || any_action_in_flight.with_value(|f| f())).to_string()
                    disabled=move || is_listing.with_value(|f| f()) || any_action_in_flight.with_value(|f| f())
                    on:click=move |_| on_refresh.with_value(|f| f())
                >
                    {move || if is_listing.with_value(|f| f()) || any_loading.get() {
                        view! { <LoaderCircle class="size-4 animate-spin"/> }.into_any()
                    } else {
                        view! { <RefreshCw class="size-4"/> }.into_any()
                    }}
                </button>

                // Settings — labeled button with sliders icon
                <button
                    class=move || {
                        let base = ButtonClass { variant: ButtonVariant::Outline, size: ButtonSize::Sm }.with_class("gap-1.5");
                        if is_settings_open.with_value(|f| f()) {
                            format!("{base} border-primary bg-accent")
                        } else {
                            base
                        }
                    }
                    type="button"
                    aria-label="Open settings"
                    aria-expanded=move || is_settings_open.with_value(|f| f()).to_string()
                    on:click=move |_| on_open_settings.with_value(|f| f())
                >
                    <SlidersHorizontal class="size-4 shrink-0"/>
                    <span class="text-xs">"Settings"</span>
                </button>
            </div>
        </nav>
    }
}

#[component]
pub fn ProviderNav<A>(
    active: A,
    on_select: Box<dyn Fn(ProviderPage) + Send + Sync>,
) -> impl IntoView
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
pub fn ProviderNavButton<A>(
    page: ProviderPage,
    active: A,
    on_select: Box<dyn Fn(ProviderPage) + Send + Sync>,
) -> impl IntoView
where
    A: Fn() -> bool + Send + Sync + 'static,
{
    use crate::ui::tooltip::{Tooltip, TooltipAlign, TooltipContent, TooltipPosition};

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
