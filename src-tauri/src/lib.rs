mod account_commands;
mod app_runtime;
mod app_updates;
mod auto_switch;
mod claude;
mod codex;
mod config_validation;
mod domain;
mod error;
mod notifications;
mod provider;
mod provider_state;
mod settings_commands;
mod snapshot;
mod tray;
mod usage_commands;

pub fn run() {
    app_runtime::run();
}
