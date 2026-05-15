#![expect(
    dead_code,
    reason = "Shared UI primitives are exported for call sites even when unused by the current binary"
)]

use icons::Check;
use leptos::prelude::*;
use tw_merge::tw_merge;

const UNUSED_PRIMITIVE_EXPORT_SENTINEL: () = ();

#[component]
pub fn Checkbox(
    #[prop(into, optional)] id: String,
    #[prop(into, optional)] class: String,
    #[prop(into, optional)] checked: Signal<bool>,
    #[prop(into, optional)] disabled: Signal<bool>,
    #[prop(into, optional)] on_checked_change: Option<Callback<bool>>,
    #[prop(into, optional, default = "Checkbox".to_string())] aria_label: String,
) -> impl IntoView {
    let checked_state = move || {
        if checked.get() {
            "checked"
        } else {
            "unchecked"
        }
    };
    let checkbox_class = tw_merge!(
        "pointer-events-none flex size-4 shrink-0 items-center justify-center rounded-[4px] border border-input shadow-xs transition-shadow outline-none dark:bg-input/30 data-[state=checked]:border-primary data-[state=checked]:bg-primary data-[state=checked]:text-primary-foreground dark:data-[state=checked]:bg-primary peer-focus-visible:border-ring peer-focus-visible:ring-ring/50 peer-focus-visible:ring-[3px] peer-disabled:cursor-not-allowed peer-disabled:opacity-50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive",
        class
    );

    view! {
        <span
            data-name="Checkbox"
            class="relative inline-flex shrink-0"
        >
            <input
                data-name="CheckboxInput"
                id=id
                class="peer absolute inset-0 z-10 m-0 h-full w-full cursor-pointer opacity-0 disabled:cursor-not-allowed"
                type="checkbox"
                prop:checked=move || checked.get()
                aria-label=aria_label
                disabled=move || disabled.get()
                on:change=move |ev| {
                    if !disabled.get_untracked() {
                        let next = event_target_checked(&ev);
                        if let Some(callback) = on_checked_change.as_ref() {
                            callback.run(next);
                        }
                    }
                }
            />
            <span
                data-name="CheckboxControl"
                class=checkbox_class
                data-state=checked_state
                aria-hidden="true"
            >
                <span
                    data-name="CheckboxIndicator"
                    class="flex items-center justify-center text-current transition-none"
                >
                    {move || checked.get().then(|| view! { <Check class="size-3.5" /> })}
                </span>
            </span>
        </span>
    }
}
