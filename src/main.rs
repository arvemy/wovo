mod app;
mod codex_api;
mod cost_usage_view;
mod formatting;
mod theme;
mod ui;

use app::*;
use leptos::prelude::*;

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| {
        view! {
            <App/>
        }
    })
}
