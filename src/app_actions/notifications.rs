use crate::codex_api::{invoke_tauri, NotificationStatus};
use crate::request_epoch::RequestEpoch;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;

#[derive(Clone, Copy)]
pub(crate) struct NotificationActions {
    pub(crate) notification_epoch: RequestEpoch,
    pub(crate) set_notification_status: WriteSignal<Option<NotificationStatus>>,
    pub(crate) is_notification_test_sending: ReadSignal<bool>,
    pub(crate) set_is_notification_test_sending: WriteSignal<bool>,
    pub(crate) set_global_error: WriteSignal<Option<String>>,
}

impl NotificationActions {
    pub(crate) fn refresh_status(&self) {
        let actions = *self;
        let ticket = actions.notification_epoch.next();
        spawn_local(async move {
            let result = invoke_tauri::<NotificationStatus>(
                "get_codex_notification_status",
                JsValue::UNDEFINED,
            )
            .await;
            if !actions.notification_epoch.is_current(ticket) {
                return;
            }

            if let Ok(status) = result {
                actions.set_notification_status.set(Some(status));
            }
        });
    }

    pub(crate) fn send_test(&self) {
        if self.is_notification_test_sending.get_untracked() {
            return;
        }

        let actions = *self;
        actions.set_is_notification_test_sending.set(true);
        let ticket = actions.notification_epoch.next();
        spawn_local(async move {
            actions.set_global_error.set(None);
            match invoke_tauri::<NotificationStatus>(
                "send_codex_test_notification",
                JsValue::UNDEFINED,
            )
            .await
            {
                Ok(status) => {
                    if actions.notification_epoch.is_current(ticket) {
                        actions.set_notification_status.set(Some(status));
                    }
                }
                Err(error) => {
                    if actions.notification_epoch.is_current(ticket) {
                        actions.set_global_error.set(Some(error.user_message));
                    }
                }
            }
            actions.set_is_notification_test_sending.set(false);
        });
    }
}
