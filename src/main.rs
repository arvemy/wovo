mod app;
mod app_actions;
mod app_context;
mod auto_switch_runway;
mod codex_api;
mod components;
mod cost_usage_view;
mod formatting;
mod request_epoch;
mod resize_guard;
mod theme;
mod ui;
mod views;

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
