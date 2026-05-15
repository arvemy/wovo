use crate::codex_api::{AppUpdateInfo, AppUpdateProgress};
use crate::ui::alert::{Alert, AlertDescription};
use crate::ui::badge::{Badge, BadgeSize, BadgeVariant};
use crate::ui::button::{ButtonClass, ButtonSize, ButtonVariant};
use icons::{LoaderCircle, RefreshCw, X};
use leptos::prelude::*;
use tw_merge::IntoTailwindClass;

#[component]
pub fn UpdateBanner(
    update: AppUpdateInfo,
    #[prop(into)] update_progress: Signal<Option<AppUpdateProgress>>,
    #[prop(into)] is_update_installing: Signal<bool>,
    on_install: Box<dyn Fn() + Send + Sync>,
    on_dismiss: Box<dyn Fn() + Send + Sync>,
) -> impl IntoView {
    let on_install = StoredValue::new(on_install);
    let on_dismiss = StoredValue::new(on_dismiss);

    let version = update.version;
    let current_version = update.current_version;
    let body = update.body.filter(|body| !body.trim().is_empty());
    let date = update.date;
    let can_install = update.can_install;

    view! {
        <Alert class="border-primary bg-secondary text-foreground">
            <AlertDescription class="flex flex-wrap items-center justify-between gap-3">
                <div class="min-w-0">
                    <div class="flex flex-wrap items-center gap-2">
                        <Badge variant=BadgeVariant::Default size=BadgeSize::Sm class="rounded-full uppercase tracking-wide">
                            "Update"
                        </Badge>
                        <strong class="text-sm font-semibold leading-5">
                            {format!("WoVo {version} is available")}
                        </strong>
                    </div>
                    <p class="mt-1 text-xs text-muted-foreground">
                        {move || update_progress.get().map(update_install_label).unwrap_or_else(|| {
                            if can_install {
                                format!("Installed version: {current_version}")
                            } else {
                                format!("Installed version: {current_version}. Update with your package manager.")
                            }
                        })}
                    </p>
                    {body.map(|body| view! {
                        <p class="mt-1 max-w-full truncate text-xs text-muted-foreground">{body}</p>
                    })}
                    {date.map(|date| view! {
                        <p class="mt-1 text-[11px] text-muted-foreground">{format!("Published {date}")}</p>
                    })}
                </div>
                <div class="flex shrink-0 items-center gap-2">
                    {if can_install {
                        view! {
                            <button
                                class=ButtonClass { variant: ButtonVariant::Default, size: ButtonSize::Sm }.with_class("")
                                type="button"
                                disabled=move || is_update_installing.get()
                                on:click=move |_| on_install.with_value(|f| f())
                            >
                                {move || if is_update_installing.get() {
                                    view! { <LoaderCircle class="size-3.5 animate-spin"/> }.into_any()
                                } else {
                                    view! { <RefreshCw class="size-3.5"/> }.into_any()
                                }}
                                <span>{move || if is_update_installing.get() { "Installing" } else { "Install" }}</span>
                            </button>
                        }.into_any()
                    } else {
                        view! {
                            <Badge variant=BadgeVariant::Muted size=BadgeSize::Default>
                                "Manual update"
                            </Badge>
                        }.into_any()
                    }}
                    <button
                        class=ButtonClass { variant: ButtonVariant::Ghost, size: ButtonSize::Icon }.with_class("size-8 text-muted-foreground")
                        type="button"
                        aria-label="Dismiss update"
                        title="Dismiss update"
                        disabled=move || is_update_installing.get()
                        on:click=move |_| on_dismiss.with_value(|f| f())
                    >
                        <X class="size-4"/>
                    </button>
                </div>
            </AlertDescription>
        </Alert>
    }
}

fn update_install_label(progress: AppUpdateProgress) -> String {
    match progress.phase.as_str() {
        "started" => "Starting update download...".to_string(),
        "progress" => {
            if let Some(content_length) = progress.content_length.filter(|value| *value > 0) {
                let percent = ((progress.downloaded as f64 / content_length as f64) * 100.0)
                    .clamp(0.0, 100.0);
                format!("Downloading update {:.0}%", percent)
            } else if progress.downloaded > 0 {
                format!("Downloaded {}", format_bytes(progress.downloaded))
            } else if let Some(chunk_length) = progress.chunk_length {
                format!("Downloading update ({})", format_bytes(chunk_length as u64))
            } else {
                "Downloading update...".to_string()
            }
        }
        "downloaded" => "Download complete. Installing...".to_string(),
        "installed" => "Update installed. Restarting...".to_string(),
        _ => "Preparing update...".to_string(),
    }
}

fn format_bytes(value: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;

    if value as f64 >= MB {
        format!("{:.1} MB", value as f64 / MB)
    } else if value as f64 >= KB {
        format!("{:.1} KB", value as f64 / KB)
    } else {
        format!("{value} B")
    }
}
