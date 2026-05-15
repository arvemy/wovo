use leptos::prelude::*;

#[component]
pub fn CredentialText<T, H, R>(
    value: T,
    hide_credentials: H,
    is_revealed: R,
    on_reveal: Box<dyn Fn(String) + Send + Sync>,
) -> impl IntoView
where
    T: Fn() -> String + Send + Sync + 'static,
    H: Fn() -> bool + Send + Sync + 'static,
    R: Fn(&str) -> bool + Send + Sync + 'static,
{
    let value = StoredValue::new(value);
    let hide_credentials = StoredValue::new(hide_credentials);
    let is_revealed = StoredValue::new(is_revealed);
    let on_reveal = StoredValue::new(on_reveal);

    view! {
        {move || {
            let text = value.with_value(|f| f());
            let privacy_enabled = hide_credentials.with_value(|f| f());
            let revealed = is_revealed.with_value(|f| f(&text));

            if privacy_enabled {
                let toggle_value = text.clone();
                let class = if revealed {
                    "inline-flex max-w-full items-center truncate rounded-sm text-left align-middle leading-none transition hover:cursor-pointer focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
                } else {
                    "inline-flex max-w-full select-none items-center truncate rounded-sm text-left text-foreground blur-[5px] align-middle leading-none transition hover:cursor-pointer focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
                };
                let aria_label = if revealed {
                    "Hide credential"
                } else {
                    "Reveal hidden credential"
                };
                view! {
                    <button
                        class=class
                        type="button"
                        aria-label=aria_label
                        title=aria_label
                        on:click=move |event| {
                            event.stop_propagation();
                            on_reveal.with_value(|f| f(toggle_value.clone()));
                        }
                    >
                        {text}
                    </button>
                }
                .into_any()
            } else {
                view! {
                    <span class="inline-flex max-w-full items-center truncate align-middle leading-none">{text}</span>
                }
                .into_any()
            }
        }}
    }
}
