use crate::codex_api::{
    invoke_tauri, CodexSettings, CodexUsageSourceMode, CommandError,
    SetAutoAccountSwitchingEnabledArgs, SetCostUsageEnabledArgs, SetHideAccountCredentialsArgs,
    SetLaunchOnLoginArgs, SetNotificationsEnabledArgs, SetUsageSourceModeArgs,
};
use crate::request_epoch::RequestEpoch;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;
use wasm_bindgen::JsValue;

use super::{NotificationActions, SnapshotActions};

#[derive(Clone, Copy)]
pub(crate) struct SettingsActions {
    pub(crate) get_settings_command: &'static str,
    pub(crate) set_usage_source_command: &'static str,
    pub(crate) set_cost_usage_command: &'static str,
    pub(crate) set_notifications_command: &'static str,
    pub(crate) set_auto_switching_command: &'static str,
    pub(crate) set_hide_credentials_command: &'static str,
    pub(crate) include_launch_on_login: bool,
    pub(crate) usage_source_mode: ReadSignal<CodexUsageSourceMode>,
    pub(crate) set_usage_source_mode: WriteSignal<CodexUsageSourceMode>,
    pub(crate) cost_usage_enabled: ReadSignal<bool>,
    pub(crate) set_cost_usage_enabled: WriteSignal<bool>,
    pub(crate) notifications_enabled: ReadSignal<bool>,
    pub(crate) set_notifications_enabled: WriteSignal<bool>,
    pub(crate) auto_account_switching_enabled: ReadSignal<bool>,
    pub(crate) set_auto_account_switching_enabled: WriteSignal<bool>,
    pub(crate) hide_account_credentials: ReadSignal<bool>,
    pub(crate) set_hide_account_credentials: WriteSignal<bool>,
    pub(crate) launch_on_login: ReadSignal<bool>,
    pub(crate) set_launch_on_login: WriteSignal<bool>,
    pub(crate) set_revealed_credential: WriteSignal<Option<String>>,
    pub(crate) set_is_settings_loading: WriteSignal<bool>,
    pub(crate) set_global_error: WriteSignal<Option<String>>,
    pub(crate) settings_epoch: RequestEpoch,
    pub(crate) snapshot_actions: SnapshotActions,
    pub(crate) notification_actions: NotificationActions,
}

impl SettingsActions {
    pub(crate) fn apply(&self, settings: CodexSettings) {
        self.set_usage_source_mode.set(settings.usage_source_mode);
        self.set_cost_usage_enabled.set(settings.cost_usage_enabled);
        self.set_notifications_enabled
            .set(settings.notifications_enabled);
        self.set_auto_account_switching_enabled
            .set(settings.auto_account_switching_enabled);
        self.set_hide_account_credentials
            .set(settings.hide_account_credentials);
        if self.include_launch_on_login {
            self.set_launch_on_login.set(settings.launch_on_login);
        }
    }

    pub(crate) fn load(&self) {
        let actions = *self;
        let ticket = actions.settings_epoch.next();
        spawn_local(async move {
            actions.set_is_settings_loading.set(true);
            let result =
                invoke_tauri::<CodexSettings>(actions.get_settings_command, JsValue::UNDEFINED)
                    .await;
            if !actions.settings_epoch.is_current(ticket) {
                return;
            }

            match result {
                Ok(settings) => actions.apply(settings),
                Err(error) => actions.set_global_error.set(Some(error.user_message)),
            }
            actions.set_is_settings_loading.set(false);
        });
    }

    pub(crate) fn change_usage_source_mode(&self, mode: CodexUsageSourceMode) {
        if mode == self.usage_source_mode.get_untracked() {
            return;
        }
        let previous = self.usage_source_mode.get_untracked();
        self.set_usage_source_mode.set(mode);
        let actions = *self;
        let ticket = actions.settings_epoch.next();
        spawn_local(async move {
            actions.set_is_settings_loading.set(true);
            actions.set_global_error.set(None);
            match command_args(&SetUsageSourceModeArgs {
                usage_source_mode: mode,
            }) {
                Ok(args) => {
                    match invoke_tauri::<CodexSettings>(actions.set_usage_source_command, args)
                        .await
                    {
                        Ok(settings) => {
                            if !actions.settings_epoch.is_current(ticket) {
                                return;
                            }
                            actions.apply(settings);
                            actions.snapshot_actions.refresh(true);
                        }
                        Err(error) => {
                            if !actions.settings_epoch.is_current(ticket) {
                                return;
                            }
                            actions.set_usage_source_mode.set(previous);
                            actions.set_global_error.set(Some(error.user_message));
                        }
                    }
                }
                Err(error) => {
                    if !actions.settings_epoch.is_current(ticket) {
                        return;
                    }
                    actions.set_usage_source_mode.set(previous);
                    actions.set_global_error.set(Some(error.user_message));
                }
            }
            if actions.settings_epoch.is_current(ticket) {
                actions.set_is_settings_loading.set(false);
            }
        });
    }

    pub(crate) fn change_cost_usage_enabled(&self, enabled: bool) {
        if enabled == self.cost_usage_enabled.get_untracked() {
            return;
        }
        let previous = self.cost_usage_enabled.get_untracked();
        self.set_cost_usage_enabled.set(enabled);
        if !enabled {
            self.snapshot_actions.set_cost_usage.set(None);
            self.snapshot_actions.set_cost_error.set(None);
        }
        let actions = *self;
        let ticket = actions.settings_epoch.next();

        spawn_local(async move {
            actions.set_is_settings_loading.set(true);
            actions.set_global_error.set(None);
            match command_args(&SetCostUsageEnabledArgs { enabled }) {
                Ok(args) => {
                    match invoke_tauri::<CodexSettings>(actions.set_cost_usage_command, args).await
                    {
                        Ok(settings) => {
                            if !actions.settings_epoch.is_current(ticket) {
                                return;
                            }
                            actions.apply(settings);
                            actions.snapshot_actions.refresh(true);
                        }
                        Err(error) => {
                            if !actions.settings_epoch.is_current(ticket) {
                                return;
                            }
                            actions.set_cost_usage_enabled.set(previous);
                            actions.set_global_error.set(Some(error.user_message));
                        }
                    }
                }
                Err(error) => {
                    if !actions.settings_epoch.is_current(ticket) {
                        return;
                    }
                    actions.set_cost_usage_enabled.set(previous);
                    actions.set_global_error.set(Some(error.user_message));
                }
            }
            if actions.settings_epoch.is_current(ticket) {
                actions.set_is_settings_loading.set(false);
            }
        });
    }

    pub(crate) fn change_notifications_enabled(&self, enabled: bool) {
        if enabled == self.notifications_enabled.get_untracked() {
            return;
        }
        let previous = self.notifications_enabled.get_untracked();
        self.set_notifications_enabled.set(enabled);
        let actions = *self;
        let ticket = actions.settings_epoch.next();

        spawn_local(async move {
            actions.set_is_settings_loading.set(true);
            actions.set_global_error.set(None);
            match command_args(&SetNotificationsEnabledArgs { enabled }) {
                Ok(args) => {
                    match invoke_tauri::<CodexSettings>(actions.set_notifications_command, args)
                        .await
                    {
                        Ok(settings) => {
                            if !actions.settings_epoch.is_current(ticket) {
                                return;
                            }
                            actions.apply(settings);
                            actions.notification_actions.refresh_status();
                        }
                        Err(error) => {
                            if !actions.settings_epoch.is_current(ticket) {
                                return;
                            }
                            actions.set_notifications_enabled.set(previous);
                            actions.set_global_error.set(Some(error.user_message));
                        }
                    }
                }
                Err(error) => {
                    if !actions.settings_epoch.is_current(ticket) {
                        return;
                    }
                    actions.set_notifications_enabled.set(previous);
                    actions.set_global_error.set(Some(error.user_message));
                }
            }
            if actions.settings_epoch.is_current(ticket) {
                actions.set_is_settings_loading.set(false);
            }
        });
    }

    pub(crate) fn change_auto_account_switching_enabled(&self, enabled: bool) {
        if enabled == self.auto_account_switching_enabled.get_untracked() {
            return;
        }
        let previous = self.auto_account_switching_enabled.get_untracked();
        self.set_auto_account_switching_enabled.set(enabled);
        let actions = *self;
        let ticket = actions.settings_epoch.next();

        spawn_local(async move {
            actions.set_is_settings_loading.set(true);
            actions.set_global_error.set(None);
            match command_args(&SetAutoAccountSwitchingEnabledArgs { enabled }) {
                Ok(args) => {
                    match invoke_tauri::<CodexSettings>(actions.set_auto_switching_command, args)
                        .await
                    {
                        Ok(settings) => {
                            if !actions.settings_epoch.is_current(ticket) {
                                return;
                            }
                            actions.apply(settings);
                            if enabled {
                                actions.snapshot_actions.refresh(true);
                            }
                        }
                        Err(error) => {
                            if !actions.settings_epoch.is_current(ticket) {
                                return;
                            }
                            actions.set_auto_account_switching_enabled.set(previous);
                            actions.set_global_error.set(Some(error.user_message));
                        }
                    }
                }
                Err(error) => {
                    if !actions.settings_epoch.is_current(ticket) {
                        return;
                    }
                    actions.set_auto_account_switching_enabled.set(previous);
                    actions.set_global_error.set(Some(error.user_message));
                }
            }
            if actions.settings_epoch.is_current(ticket) {
                actions.set_is_settings_loading.set(false);
            }
        });
    }

    pub(crate) fn change_hide_account_credentials(&self, enabled: bool) {
        if enabled == self.hide_account_credentials.get_untracked() {
            return;
        }
        let previous = self.hide_account_credentials.get_untracked();
        self.set_hide_account_credentials.set(enabled);
        if enabled {
            self.set_revealed_credential.set(None);
        }
        let actions = *self;
        let ticket = actions.settings_epoch.next();

        spawn_local(async move {
            actions.set_is_settings_loading.set(true);
            actions.set_global_error.set(None);
            match command_args(&SetHideAccountCredentialsArgs { enabled }) {
                Ok(args) => {
                    match invoke_tauri::<CodexSettings>(actions.set_hide_credentials_command, args)
                        .await
                    {
                        Ok(settings) => {
                            if !actions.settings_epoch.is_current(ticket) {
                                return;
                            }
                            let hide_enabled = settings.hide_account_credentials;
                            actions.apply(settings);
                            if hide_enabled {
                                actions.set_revealed_credential.set(None);
                            }
                        }
                        Err(error) => {
                            if !actions.settings_epoch.is_current(ticket) {
                                return;
                            }
                            actions.set_hide_account_credentials.set(previous);
                            actions.set_global_error.set(Some(error.user_message));
                        }
                    }
                }
                Err(error) => {
                    if !actions.settings_epoch.is_current(ticket) {
                        return;
                    }
                    actions.set_hide_account_credentials.set(previous);
                    actions.set_global_error.set(Some(error.user_message));
                }
            }
            if actions.settings_epoch.is_current(ticket) {
                actions.set_is_settings_loading.set(false);
            }
        });
    }

    pub(crate) fn change_launch_on_login(&self, enabled: bool) {
        if enabled == self.launch_on_login.get_untracked() {
            return;
        }
        let previous = self.launch_on_login.get_untracked();
        self.set_launch_on_login.set(enabled);
        let actions = *self;
        let ticket = actions.settings_epoch.next();

        spawn_local(async move {
            actions.set_is_settings_loading.set(true);
            actions.set_global_error.set(None);
            match command_args(&SetLaunchOnLoginArgs { enabled }) {
                Ok(args) => {
                    match invoke_tauri::<CodexSettings>("set_codex_launch_on_login", args).await {
                        Ok(settings) => {
                            if !actions.settings_epoch.is_current(ticket) {
                                return;
                            }
                            actions.apply(settings);
                        }
                        Err(error) => {
                            if !actions.settings_epoch.is_current(ticket) {
                                return;
                            }
                            actions.set_launch_on_login.set(previous);
                            actions.set_global_error.set(Some(error.user_message));
                        }
                    }
                }
                Err(error) => {
                    if !actions.settings_epoch.is_current(ticket) {
                        return;
                    }
                    actions.set_launch_on_login.set(previous);
                    actions.set_global_error.set(Some(error.user_message));
                }
            }
            if actions.settings_epoch.is_current(ticket) {
                actions.set_is_settings_loading.set(false);
            }
        });
    }
}

fn command_args(args: &impl Serialize) -> Result<JsValue, CommandError> {
    serde_wasm_bindgen::to_value(args)
        .map_err(|error| CommandError::from_message(error.to_string()))
}
