#![allow(dead_code)]
use leptos::prelude::*;
use tw_merge::tw_merge;

#[component]
pub fn Progress(
    #[prop(into, optional)] class: String,
    #[prop(into, optional)] value: Signal<f64>,
    #[prop(default = 100.0)] max: f64,
) -> impl IntoView {
    let style = move || {
        let pct = (value.get() / max * 100.0).clamp(0.0, 100.0);
        format!("transform: translateX(-{}%)", 100.0 - pct)
    };
    let merged_class = tw_merge!(
        "relative h-2 w-full overflow-hidden rounded-full bg-secondary",
        class
    );
    view! {
        <div role="progressbar" class=merged_class>
            <div
                class="flex-1 w-full h-full transition-all duration-300 ease-in-out bg-primary"
                style=style
            />
        </div>
    }
}
