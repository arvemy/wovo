mod account_commands;
mod app_runtime;
mod auto_switch;
mod codex;
mod domain;
mod error;
mod notifications;
mod settings_commands;
mod snapshot;
mod usage_commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    app_runtime::run();
}
