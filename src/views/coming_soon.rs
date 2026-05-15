use leptos::prelude::*;

#[component]
pub fn ComingSoonPage() -> impl IntoView {
    view! {
        <main class="flex min-h-0 flex-1 items-center justify-center overflow-hidden">
            <div class="grid justify-items-center gap-3 text-center">
                <img src="/public/anthropic-black.svg" class="size-12 dark:hidden" alt="Anthropic"/>
                <img src="/public/anthropic-white.svg" class="hidden size-12 dark:block" alt="Anthropic"/>
                <div class="grid gap-1">
                    <h1 class="text-sm font-semibold leading-none">"Claude Code"</h1>
                    <p class="text-xs text-muted-foreground">"Coming Soon"</p>
                </div>
            </div>
        </main>
    }
}
