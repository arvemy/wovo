use icons::{LoaderCircle, RefreshCw};
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

#[component]
pub fn App() -> impl IntoView {
    let (account, set_account) = signal::<Option<AccountSummary>>(None);
    let (usage, set_usage) = signal::<Option<UsageSnapshot>>(None);
    let (is_loading, set_is_loading) = signal(true);
    let (error, set_error) = signal::<Option<String>>(None);

    let load_account_and_usage = move || {
        spawn_local(async move {
            set_is_loading.set(true);
            set_error.set(None);

            match invoke_tauri::<Option<AccountSummary>>(
                "get_detected_codex_account",
                JsValue::UNDEFINED,
            )
            .await
            {
                Ok(Some(next_account)) => {
                    set_account.set(Some(next_account.clone()));
                    match refresh_usage(&next_account.id).await {
                        Ok(snapshot) => set_usage.set(Some(snapshot)),
                        Err(message) => set_error.set(Some(message)),
                    }
                }
                Ok(None) => {
                    set_account.set(None);
                    set_usage.set(None);
                }
                Err(message) => {
                    set_account.set(None);
                    set_usage.set(None);
                    set_error.set(Some(message));
                }
            }

            set_is_loading.set(false);
        });
    };

    load_account_and_usage();

    let refresh = move |_| {
        let Some(current_account) = account.get_untracked() else {
            load_account_and_usage();
            return;
        };

        spawn_local(async move {
            set_is_loading.set(true);
            set_error.set(None);
            match refresh_usage(&current_account.id).await {
                Ok(snapshot) => set_usage.set(Some(snapshot)),
                Err(message) => set_error.set(Some(message)),
            }
            set_is_loading.set(false);
        });
    };

    view! {
        <main class="mx-auto min-h-screen w-[min(560px,calc(100vw-2rem))] bg-[var(--background)] py-10 text-[var(--foreground)] max-sm:w-[min(100vw-1.5rem,560px)] max-sm:py-6">
            <header class="mb-9 flex items-center justify-between gap-4 max-sm:items-start">
                <div>
                    <h1 class="text-2xl font-semibold leading-none tracking-normal">"Wovo"</h1>
                </div>
                <button
                    class="inline-flex h-9 min-w-20 items-center justify-center gap-2 whitespace-nowrap rounded-md border border-[var(--border)] bg-[var(--background)] px-4 py-2 text-sm font-medium text-[var(--foreground)] transition-all hover:cursor-pointer hover:bg-[var(--accent)] active:scale-[0.98] disabled:pointer-events-none disabled:opacity-50 max-sm:w-full"
                    data-name="Button"
                    type="button"
                    on:click=refresh
                    disabled=move || is_loading.get()
                >
                    {move || {
                        if is_loading.get() {
                            view! {
                                <LoaderCircle class="size-4 animate-spin" />
                                <span class="sr-only">"Refreshing"</span>
                            }.into_any()
                        } else {
                            view! {
                                <RefreshCw class="size-4" />
                                <span class="sr-only">"Refresh"</span>
                            }.into_any()
                        }
                    }}
                </button>
            </header>

            <section class="w-full">
                {move || render_content(account.get(), usage.get(), is_loading.get(), error.get())}
            </section>
        </main>
    }
}

fn render_content(
    account: Option<AccountSummary>,
    usage: Option<UsageSnapshot>,
    is_loading: bool,
    error: Option<String>,
) -> impl IntoView {
    match (account, usage) {
        (Some(account), Some(usage)) => view! {
            <div class="w-full rounded-lg border border-[var(--border)] bg-[var(--card)] p-6 text-[var(--card-foreground)] shadow-xs" data-name="Card">
                <AccountHeader account=account.clone() usage=Some(usage.clone())/>
                <div class="grid gap-6">
                    {usage.primary.clone().map(|window| view! { <UsageMeter window=window/> })}
                    {usage.secondary.clone().map(|window| view! { <UsageMeter window=window/> })}
                </div>
                <Credits credits=usage.credits.clone()/>
                <StatusLine
                    updated_at=Some(usage.updated_at)
                    is_loading=is_loading
                    error=error
                />
            </div>
        }
        .into_any(),
        (Some(account), None) => view! {
            <div class="w-full rounded-lg border border-[var(--border)] bg-[var(--card)] p-6 text-[var(--card-foreground)] shadow-xs" data-name="Card">
                <AccountHeader account=account usage=None/>
                <div class="flex flex-col items-center justify-center gap-2 rounded-lg border border-dashed border-[var(--border)] p-8 text-center" data-name="Empty">
                    <h2 class="text-lg font-semibold leading-none">"No usage yet"</h2>
                    <p class="text-sm text-[var(--muted-foreground)]">"Refresh to load Codex limits."</p>
                </div>
                <StatusLine updated_at=None is_loading=is_loading error=error/>
            </div>
        }
        .into_any(),
        (None, _) if is_loading => view! {
            <div class="flex flex-col items-center justify-center gap-2 rounded-lg border border-dashed border-[var(--border)] p-8 text-center" data-name="Empty">
                <h2 class="text-lg font-semibold leading-none">"Checking Codex"</h2>
            </div>
        }
        .into_any(),
        (None, _) => view! {
            <div class="flex flex-col items-center justify-center gap-2 rounded-lg border border-dashed border-[var(--border)] p-8 text-center" data-name="Empty">
                <h2 class="text-lg font-semibold leading-none">"No Codex account found"</h2>
                <p class="text-sm text-[var(--muted-foreground)]">"Run `codex login`, then refresh."</p>
                {error.map(|message| view! { <p class="text-sm font-medium text-[var(--foreground)]">{message}</p> })}
            </div>
        }
        .into_any(),
    }
}

#[component]
fn AccountHeader(account: AccountSummary, usage: Option<UsageSnapshot>) -> impl IntoView {
    let plan = usage
        .as_ref()
        .and_then(|snapshot| snapshot.plan_type.clone());

    view! {
        <div class="mb-7 grid grid-cols-[minmax(0,1fr)_auto] items-baseline gap-3 max-sm:grid-cols-1">
            <div>
                <h2 class="text-base font-semibold leading-none">{account.label}</h2>
            </div>
            {plan.map(|plan| view! { <span class="text-sm font-medium text-[var(--muted-foreground)]">{plan}</span> })}
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
        <div class="grid gap-2">
            <div class="flex justify-between gap-3 text-sm text-[var(--foreground)]">
                <span>{label.clone()}</span>
                <strong>{format!("{:.0}% used", used)}</strong>
            </div>
            <div
                class="relative h-2 w-full overflow-hidden rounded-full border border-[var(--border)] bg-[var(--secondary)]"
                data-name="Progress"
                role="progressbar"
                aria-label={format!("{} usage", label)}
                aria-valuemin="0"
                aria-valuemax="100"
                aria-valuenow={format!("{:.0}", used)}
            >
                <div class="h-full min-w-0.5 bg-[var(--success)] transition-all duration-300 ease-in-out" style=width></div>
            </div>
            <div class="flex justify-between gap-3 text-xs text-[var(--muted-foreground)] max-sm:flex-col">
                <span>{format!("{:.0}% remaining", window.remaining_percent)}</span>
                <span>{reset}</span>
            </div>
        </div>
    }
}

#[component]
fn Credits(credits: Option<CreditsSnapshot>) -> impl IntoView {
    let label = match credits {
        Some(CreditsSnapshot {
            unlimited: true, ..
        }) => "Credits: unlimited".to_string(),
        Some(CreditsSnapshot {
            balance: Some(balance),
            has_credits: true,
            ..
        }) => format!("Credits: {:.2}", balance),
        Some(CreditsSnapshot {
            has_credits: false, ..
        }) => "Credits unavailable".to_string(),
        Some(_) => "Credits enabled".to_string(),
        None => "Credits unavailable".to_string(),
    };

    view! { <div class="mt-6 inline-flex text-sm font-medium text-[var(--muted-foreground)]">{label}</div> }
}

#[component]
fn StatusLine(
    updated_at: Option<i64>,
    is_loading: bool,
    error: Option<String>,
) -> impl IntoView {
    view! {
        <div class="mt-6 flex items-center justify-between gap-3 text-xs text-[var(--muted-foreground)] max-sm:flex-col max-sm:items-stretch">
            <span>
                {if is_loading {
                    "Refreshing...".to_string()
                } else {
                    updated_at.map(format_time_ago).unwrap_or_else(|| "Not refreshed".to_string())
                }}
            </span>
            {error.map(|message| view! { <span class="font-medium text-[var(--foreground)]">{message}</span> })}
        </div>
    }
}

async fn refresh_usage(account_id: &str) -> Result<UsageSnapshot, String> {
    let args = serde_wasm_bindgen::to_value(&RefreshArgs { account_id })
        .map_err(|error| error.to_string())?;
    invoke_tauri("refresh_codex_usage", args).await
}

async fn invoke_tauri<T>(cmd: &str, args: JsValue) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let value = invoke(cmd, args)
        .await
        .map_err(|error| js_error_message(&error))?;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

fn js_error_message(value: &JsValue) -> String {
    if let Some(message) = js_sys::Reflect::get(value, &JsValue::from_str("message"))
        .ok()
        .and_then(|value| value.as_string())
    {
        return message;
    }

    value
        .as_string()
        .unwrap_or_else(|| "Wovo could not complete the request.".to_string())
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
