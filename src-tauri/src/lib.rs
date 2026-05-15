mod account_commands;
mod app_runtime;
mod app_updates;
mod auto_switch;
mod codex;
mod domain;
mod error;
mod notifications;
mod settings_commands;
mod snapshot;
mod tray;
mod usage_commands;

pub fn run() {
    app_runtime::run();
}
