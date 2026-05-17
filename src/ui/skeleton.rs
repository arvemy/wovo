use leptos::prelude::*;

#[component]
pub fn Skeleton(#[prop(into, optional)] class: String) -> impl IntoView {
    let class = tw_merge::tw_merge!("animate-pulse rounded-md bg-muted", class);
    view! { <div class=class aria-hidden="true"></div> }
}
