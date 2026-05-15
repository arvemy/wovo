use crate::codex_api::QuotaEvent;
use crate::components::credential_text::CredentialText;
use crate::formatting::{
    quota_event_body_suffix, quota_event_class, quota_event_kind_label, quota_event_meta_suffix,
};
use crate::ui::badge::{Badge, BadgeSize, BadgeVariant};
use icons::X;
use leptos::prelude::*;

#[component]
pub fn QuotaEventCard<H, R>(
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
