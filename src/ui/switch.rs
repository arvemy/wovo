use leptos::prelude::*;
use tw_merge::tw_merge;

#[component]
pub fn Switch(
    #[prop(optional, into)] id: String,
    #[prop(into)] checked: Signal<bool>,
    #[prop(into, optional)] disabled: Signal<bool>,
    #[prop(into, optional, default = "Toggle switch".to_string())] aria_label: String,
    #[prop(into, optional)] class: String,
    #[prop(into)] on_checked_change: Callback<bool>,
) -> impl IntoView {
    let state = move || {
        if checked.get() {
            "checked"
        } else {
            "unchecked"
        }
    };

    let track_class = tw_merge!(
        "inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full border-2 border-transparent transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:bg-primary data-[state=unchecked]:bg-input hover:cursor-pointer",
        class
    );

    view! {
        <button
            data-name="Switch"
            id=id
            type="button"
            role="switch"
            aria-checked=move || checked.get().to_string()
            aria-label=aria_label
            data-state=state
            class=track_class
            disabled=move || disabled.get()
            on:click=move |_| {
                if !disabled.get() {
                    on_checked_change.run(!checked.get_untracked());
                }
            }
        >
            <span
                data-state=state
                class="block size-5 rounded-full bg-background shadow-lg ring-0 transition-transform pointer-events-none data-[state=checked]:translate-x-5 data-[state=unchecked]:translate-x-0"
            />
        </button>
    }
}
