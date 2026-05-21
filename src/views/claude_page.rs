use crate::views::codex_page::{CodexPage, CodexPageActions, CodexPageData};
use leptos::prelude::*;

#[component]
pub fn ClaudePage(data: CodexPageData, actions: CodexPageActions) -> impl IntoView {
    view! {
        <CodexPage data=data actions=actions/>
    }
}
