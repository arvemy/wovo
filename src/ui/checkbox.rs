use icons::Check;
use leptos::prelude::*;
use tw_merge::tw_merge;

#[component]
pub fn Checkbox(
    #[prop(into)] checked: Signal<bool>,
    #[prop(into, optional)] disabled: Signal<bool>,
    #[prop(into, optional)] class: String,
    #[prop(into)] on_change: Callback<bool>,
) -> impl IntoView {
    let indicator_class = move || {
        let state_class = if checked.get() {
            "border-primary bg-primary text-primary-foreground"
        } else {
            "border-border bg-background text-transparent"
        };

        tw_merge!(
            "pointer-events-none flex size-4 shrink-0 items-center justify-center rounded-sm border shadow-xs transition-colors peer-focus-visible:ring-[3px] peer-focus-visible:ring-ring/50 peer-disabled:opacity-50",
            state_class,
        )
    };

    let wrapper_class = tw_merge!("relative inline-flex size-4 shrink-0", class);

    view! {
        <span class=wrapper_class data-name="Checkbox">
            <input
                class="peer absolute inset-0 z-10 size-4 cursor-pointer opacity-0 disabled:cursor-not-allowed"
                type="checkbox"
                prop:checked=move || checked.get()
                disabled=move || disabled.get()
                on:change=move |event| on_change.run(event_target_checked(&event))
            />
            <span class=indicator_class aria-hidden="true">
                <Check class="size-3"/>
            </span>
        </span>
    }
}
