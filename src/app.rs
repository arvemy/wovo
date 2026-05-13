use std::collections::{HashMap, HashSet};

use icons::{LoaderCircle, Plus, RefreshCw, Trash2};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = ["window", "__TAURI__", "core"])]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountSummary {
    id: String,
    label: String,
    source: AccountSourceKind,
    is_live_system: bool,
    can_set_system: bool,
    can_remove: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum AccountSourceKind {
    Ambient,
    Managed,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageSnapshot {
    plan_type: Option<String>,
    primary: Option<UsageWindow>,
    secondary: Option<UsageWindow>,
    credits: Option<CreditsSnapshot>,
    updated_at: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageWindow {
    label: String,
    used_percent: f64,
    remaining_percent: f64,
    reset_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreditsSnapshot {
    balance: Option<f64>,
    has_credits: bool,
    unlimited: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RefreshArgs<'a> {
    account_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountActionArgs<'a> {
    account_id: &'a str,
}

#[derive(Clone, Debug)]
struct CommandError {
    code: Option<String>,
    message: String,
}

impl CommandError {
    fn is_auth_failure(&self) -> bool {
        matches!(
            self.code.as_deref(),
            Some("auth_not_found" | "missing_tokens")
        ) || {
            let message = self.message.to_ascii_lowercase();
            message.contains("401")
                || message.contains("403")
                || message.contains("unauthorized")
                || message.contains("invalid_grant")
                || message.contains("auth.json was not found")
                || message.contains("does not contain oauth tokens")
        }
    }
}

#[component]
pub fn App() -> impl IntoView {
    let (accounts, set_accounts) = signal::<Vec<AccountSummary>>(Vec::new());
    let (usage_by_id, set_usage_by_id) = signal::<HashMap<String, UsageSnapshot>>(HashMap::new());
    let (errors_by_id, set_errors_by_id) = signal::<HashMap<String, String>>(HashMap::new());
    let (loading_ids, set_loading_ids) = signal::<HashSet<String>>(HashSet::new());
    let (reauth_ids, set_reauth_ids) = signal::<HashSet<String>>(HashSet::new());
    let (is_listing, set_is_listing) = signal(true);
    let (is_account_action_loading, set_is_account_action_loading) = signal(false);
    let (global_error, set_global_error) = signal::<Option<String>>(None);

    let mark_loading = move |id: String, loading: bool| {
        set_loading_ids.update(|set| {
            if loading {
                set.insert(id);
            } else {
                set.remove(&id);
            }
        });
    };

    let refresh_account_usage = move |account_id: String| {
        let id_for_clear = account_id.clone();
        set_errors_by_id.update(|map| {
            map.remove(&id_for_clear);
        });
        mark_loading(account_id.clone(), true);

        spawn_local(async move {
            match refresh_usage(&account_id).await {
                Ok(snapshot) => {
                    let id = account_id.clone();
                    set_reauth_ids.update(|set| {
                        set.remove(&id);
                    });
                    set_usage_by_id.update(|map| {
                        map.insert(account_id.clone(), snapshot);
                    });
                }
                Err(error) => {
                    let id = account_id.clone();
                    if error.is_auth_failure() {
                        set_reauth_ids.update(|set| {
                            set.insert(id.clone());
                        });
                    }
                    set_usage_by_id.update(|map| {
                        map.remove(&id);
                    });
                    set_errors_by_id.update(|map| {
                        map.insert(id, error.message);
                    });
                }
            }
            mark_loading(account_id, false);
        });
    };

    let load_accounts_and_usage = move || {
        spawn_local(async move {
            set_is_listing.set(true);
            set_global_error.set(None);

            match invoke_tauri::<Vec<AccountSummary>>("list_codex_accounts", JsValue::UNDEFINED)
                .await
            {
                Ok(next_accounts) => {
                    let next_ids: HashSet<String> = next_accounts
                        .iter()
                        .map(|account| account.id.clone())
                        .collect();
                    set_accounts.set(next_accounts.clone());
                    set_usage_by_id.update(|map| map.retain(|id, _| next_ids.contains(id)));
                    set_errors_by_id.update(|map| map.retain(|id, _| next_ids.contains(id)));
                    set_loading_ids.update(|set| set.retain(|id| next_ids.contains(id)));
                    set_reauth_ids.update(|set| set.retain(|id| next_ids.contains(id)));

                    for account in next_accounts {
                        refresh_account_usage(account.id);
                    }
                }
                Err(error) => {
                    set_accounts.set(Vec::new());
                    set_usage_by_id.set(HashMap::new());
                    set_errors_by_id.set(HashMap::new());
                    set_loading_ids.set(HashSet::new());
                    set_reauth_ids.set(HashSet::new());
                    set_global_error.set(Some(error.message));
                }
            }

            set_is_listing.set(false);
        });
    };

    load_accounts_and_usage();

    let refresh_all = move |_| {
        let current = accounts.get_untracked();
        if current.is_empty() {
            load_accounts_and_usage();
            return;
        }
        for account in current {
            refresh_account_usage(account.id);
        }
    };

    let add_account = move || {
        spawn_local(async move {
            set_is_account_action_loading.set(true);
            set_global_error.set(None);

            match invoke_tauri::<AccountSummary>("add_codex_account", JsValue::UNDEFINED).await {
                Ok(_) => load_accounts_and_usage(),
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
                    load_accounts_and_usage();
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
                    load_accounts_and_usage();
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
                Ok(_) => load_accounts_and_usage(),
                Err(error) => set_global_error.set(Some(error.message)),
            }

            set_is_account_action_loading.set(false);
        });
    };

    let any_action_in_flight = move || is_account_action_loading.get();
    let any_loading = Memo::new(move |_| !loading_ids.get().is_empty());
    let latest_updated_at =
        Memo::new(move |_| usage_by_id.with(|map| map.values().map(|s| s.updated_at).max()));

    view! {
        <main class="mx-auto min-h-screen w-[min(820px,calc(100vw-2rem))] bg-[var(--background)] py-6 text-[var(--foreground)] max-sm:w-[min(100vw-1.5rem,820px)] max-sm:py-4">
            <header class="mb-4 flex items-center justify-start">
                <img
                    class="h-20 w-auto drop-shadow-sm max-sm:h-16"
                    src="/public/wovo-logo.png"
                    alt="Wovo"
                />
            </header>

            <div class="mb-5 flex items-center justify-between gap-3">
                <div class="flex min-w-0 items-center gap-3">
                    <img src="/public/openai-black.svg" class="size-12 shrink-0 dark:hidden" alt="OpenAI"/>
                    <img src="/public/openai-white.svg" class="hidden size-12 shrink-0 dark:block" alt="OpenAI"/>
                    <h1 class="text-lg font-semibold leading-none tracking-tight">"Codex"</h1>
                </div>
                <div class="flex items-center gap-2">
                    <Tooltip text=move || {
                        if is_account_action_loading.get() {
                            "Cancel login".to_string()
                        } else {
                            "Add Codex account".to_string()
                        }
                    }>
                        <button
                            class="inline-flex h-9 w-9 items-center justify-center rounded-md border border-[var(--border)] bg-[var(--background)] text-[var(--foreground)] transition-all hover:cursor-pointer hover:bg-[var(--accent)] active:scale-[0.98] disabled:pointer-events-none disabled:opacity-50"
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
                    </Tooltip>
                    <Tooltip text=move || "Refresh all accounts".to_string()>
                        <button
                            class="inline-flex h-9 w-9 items-center justify-center rounded-md border border-[var(--border)] bg-[var(--background)] text-[var(--foreground)] transition-all hover:cursor-pointer hover:bg-[var(--accent)] active:scale-[0.98] disabled:pointer-events-none disabled:opacity-50"
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
                    </Tooltip>
                </div>
            </div>

            {move || {
                let global = global_error.get();
                global.map(|message| view! {
                    <div class="mb-4 text-sm font-medium text-[var(--foreground)]">{message}</div>
                })
            }}

            {move || {
                let current = accounts.get();
                if current.is_empty() {
                    if is_listing.get() {
                        view! {
                            <div class="flex flex-col items-center justify-center gap-2 py-12 text-center">
                                <LoaderCircle class="size-4 animate-spin text-[var(--muted-foreground)]"/>
                                <p class="text-xs font-medium text-[var(--muted-foreground)]">"Checking Codex"</p>
                            </div>
                        }
                        .into_any()
                    } else {
                        view! {
                            <div class="flex flex-col items-center justify-center gap-2 py-12 text-center">
                                <h2 class="text-sm font-semibold leading-none">"No Codex account found"</h2>
                                <p class="text-xs text-[var(--muted-foreground)]">
                                    "Use the + button above to add an account, or run `codex login`."
                                </p>
                            </div>
                        }
                        .into_any()
                    }
                } else {
                    view! {
                        <div class="divide-y divide-[var(--border)]">
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

            <p class="mt-5 text-[11px] text-[var(--muted-foreground)]">
                {move || {
                    if any_loading.get() || is_listing.get() {
                        "Refreshing...".to_string()
                    } else {
                        match latest_updated_at.get() {
                            Some(ts) => format!("Last refreshed {}", format_time_ago(ts)),
                            None => "Not refreshed".to_string(),
                        }
                    }
                }}
            </p>
        </main>
    }
}

#[component]
fn Tooltip<F, S>(text: F, children: Children) -> impl IntoView
where
    F: Fn() -> S + Send + Sync + 'static,
    S: Into<String> + 'static,
{
    let text = StoredValue::new(text);

    view! {
        <span class="group/tooltip relative inline-flex">
            {children()}
            <span
                role="tooltip"
                class="pointer-events-none absolute left-1/2 top-full z-10 mt-2 -translate-x-1/2 whitespace-nowrap rounded-md border border-[var(--border)] bg-[var(--popover)] px-2 py-1 text-xs font-medium text-[var(--popover-foreground)] opacity-0 shadow-xs transition-opacity duration-150 group-hover/tooltip:opacity-100 group-focus-within/tooltip:opacity-100"
            >
                {move || text.with_value(|f| f().into())}
            </span>
        </span>
    }
}

#[component]
fn AccountRow<T, M, S, C, X, U, E, L, R, D>(
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
    let primary = move || usage.with_value(|f| f().and_then(|s| s.primary));
    let secondary = move || usage.with_value(|f| f().and_then(|s| s.secondary));
    let credits = move || usage.with_value(|f| f().and_then(|s| s.credits));
    let has_usage = move || usage.with_value(|f| f().is_some());

    view! {
        <div class="py-4 first:pt-0 last:pb-0">
            <div class="mb-3 flex items-start justify-between gap-3 max-sm:flex-col max-sm:items-stretch">
                <div class="flex min-w-0 items-baseline gap-2">
                    <h2 class="truncate font-mono text-sm font-medium leading-5 tracking-normal">{label_call}</h2>
                    {move || {
                        if is_live_system_call() {
                            view! {
                                <span class="shrink-0 rounded-md border border-[var(--border)] px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-[var(--muted-foreground)]">"System"</span>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }
                    }}
                    {move || plan_label().map(|plan| view! {
                        <span class="shrink-0 rounded-md border border-[var(--border)] px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-[var(--muted-foreground)]">{plan}</span>
                    })}
                </div>
                <div class="flex items-center gap-2 max-sm:justify-end">
                    {move || {
                        if can_set_system_call() {
                            view! {
                                <button
                                    class="inline-flex h-8 items-center justify-center whitespace-nowrap rounded-md border border-[var(--border)] bg-[var(--background)] px-3 text-xs font-medium hover:cursor-pointer hover:bg-[var(--accent)] disabled:pointer-events-none disabled:opacity-50"
                                    type="button"
                                    disabled=move || disabled_for_set_system.with_value(|f| f())
                                    on:click=move |_| on_set_system.with_value(|f| f())
                                >
                                    "Set as System"
                                </button>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }
                    }}
                    {move || {
                        if reauth_required_call() {
                            let trigger = move |_| on_reauth.with_value(|f| f());
                            view! {
                                <button
                                    class="inline-flex h-8 items-center justify-center whitespace-nowrap rounded-md border border-[var(--border)] bg-[var(--background)] px-3 text-xs font-medium hover:cursor-pointer hover:bg-[var(--accent)] disabled:pointer-events-none disabled:opacity-50"
                                    type="button"
                                    disabled=move || disabled_for_reauth.with_value(|f| f())
                                    on:click=trigger
                                >
                                    "Re-auth"
                                </button>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }
                    }}
                    {move || {
                        if is_managed_call() && can_remove_call() {
                            view! {
                                <Tooltip text=move || "Remove account".to_string()>
                                    <button
                                        class="inline-flex h-8 w-8 items-center justify-center rounded-md border border-transparent text-[var(--muted-foreground)] hover:cursor-pointer hover:border-[var(--border)] hover:bg-[var(--destructive,var(--accent))] hover:text-[var(--destructive-foreground,var(--foreground))] active:scale-[0.98] disabled:pointer-events-none disabled:opacity-50"
                                        type="button"
                                        aria-label="Remove account"
                                        disabled=move || disabled_for_remove.with_value(|f| f())
                                        on:click=move |_| on_remove.with_value(|f| f())
                                    >
                                        <Trash2 class="size-4"/>
                                    </button>
                                </Tooltip>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }
                    }}
                </div>
            </div>

            <div class="grid gap-3">
                {move || primary().map(|window| view! { <UsageMeter window=window/> })}
                {move || secondary().map(|window| view! { <UsageMeter window=window/> })}
                {move || {
                    if has_usage() {
                        view! { <span></span> }.into_any()
                    } else if is_loading_call() {
                        view! {
                            <div class="flex items-center gap-2 text-xs text-[var(--muted-foreground)]">
                                <LoaderCircle class="size-3.5 animate-spin"/>
                                <span>"Loading usage..."</span>
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <p class="text-xs text-[var(--muted-foreground)]">
                                "No usage yet."
                            </p>
                        }.into_any()
                    }
                }}
            </div>

            {move || credits().and_then(render_credits)}

            {move || error.with_value(|f| f()).map(|message| view! {
                <p class="mt-2 text-xs font-medium text-[var(--foreground)]">{message}</p>
            })}
        </div>
    }
}

#[component]
fn UsageMeter(window: UsageWindow) -> impl IntoView {
    let used = window.used_percent.clamp(0.0, 100.0);
    let width = format!("width: {:.1}%;", used);
    let label = window.label.clone();
    let reset = window
        .reset_at
        .map(format_remaining_time)
        .unwrap_or_else(|| "Reset unavailable".to_string());

    view! {
        <div class="grid gap-1.5">
            <div class="flex justify-between gap-3 text-xs text-[var(--foreground)]">
                <span>{label.clone()}</span>
                <strong>{format!("{:.0}% used", used)}</strong>
            </div>
            <div
                class="relative h-3 w-full overflow-hidden rounded-full bg-[var(--secondary)]"
                role="progressbar"
                aria-label={format!("{} usage", label)}
                aria-valuemin="0"
                aria-valuemax="100"
                aria-valuenow={format!("{:.0}", used)}
            >
                <div class="h-full min-w-0.5 rounded-full bg-[var(--success)] transition-all duration-300 ease-in-out" style=width></div>
            </div>
            <div class="flex justify-between gap-3 text-[11px] text-[var(--muted-foreground)] max-sm:flex-col">
                <span>{format!("{:.0}% remaining", window.remaining_percent)}</span>
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
        <p class="mt-3 text-xs font-medium text-[var(--muted-foreground)]">{label}</p>
    })
}

async fn refresh_usage(account_id: &str) -> Result<UsageSnapshot, CommandError> {
    let args = serde_wasm_bindgen::to_value(&RefreshArgs { account_id })
        .map_err(|error| CommandError::from_message(error.to_string()))?;
    invoke_tauri("refresh_codex_usage", args).await
}

async fn account_action<T>(cmd: &str, account_id: &str) -> Result<T, CommandError>
where
    T: DeserializeOwned,
{
    let args = serde_wasm_bindgen::to_value(&AccountActionArgs { account_id })
        .map_err(|error| CommandError::from_message(error.to_string()))?;
    invoke_tauri(cmd, args).await
}

async fn invoke_tauri<T>(cmd: &str, args: JsValue) -> Result<T, CommandError>
where
    T: DeserializeOwned,
{
    let value = invoke(cmd, args)
        .await
        .map_err(|error| js_command_error(&error))?;
    serde_wasm_bindgen::from_value(value)
        .map_err(|error| CommandError::from_message(error.to_string()))
}

fn js_command_error(value: &JsValue) -> CommandError {
    let code = js_sys::Reflect::get(value, &JsValue::from_str("code"))
        .ok()
        .and_then(|value| value.as_string());
    let message = js_sys::Reflect::get(value, &JsValue::from_str("message"))
        .ok()
        .and_then(|value| value.as_string())
        .or_else(|| value.as_string())
        .unwrap_or_else(|| "Wovo could not complete the request.".to_string());

    CommandError { code, message }
}

impl CommandError {
    fn from_message(message: String) -> Self {
        Self {
            code: None,
            message,
        }
    }
}

fn format_time_ago(value: i64) -> String {
    let now_seconds = js_sys::Date::now() / 1000.0;
    let elapsed = (now_seconds - (value as f64)).max(0.0).round() as i64;

    if elapsed < 5 {
        return "just now".to_string();
    }

    let (amount, unit) = if elapsed < 60 {
        (elapsed, "second")
    } else if elapsed < 3_600 {
        (elapsed / 60, "minute")
    } else if elapsed < 86_400 {
        (elapsed / 3_600, "hour")
    } else if elapsed < 2_592_000 {
        (elapsed / 86_400, "day")
    } else if elapsed < 31_536_000 {
        (elapsed / 2_592_000, "month")
    } else {
        (elapsed / 31_536_000, "year")
    };

    if amount == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{amount} {unit}s ago")
    }
}

fn format_remaining_time(reset_at: i64) -> String {
    let now_seconds = js_sys::Date::now() / 1000.0;
    let remaining = ((reset_at as f64) - now_seconds).max(0.0).round() as i64;

    if remaining <= 0 {
        return "resets now".to_string();
    }

    let days = remaining / 86_400;
    let hours = (remaining % 86_400) / 3_600;
    let minutes = (remaining % 3_600) / 60;

    if days > 0 {
        format!("{days}d {hours}h left")
    } else if hours > 0 {
        format!("{hours}h {minutes}m left")
    } else {
        format!("{minutes}m left")
    }
}
