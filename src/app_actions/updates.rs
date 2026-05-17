use crate::codex_api::{invoke_tauri, AppUpdateInfo, AppUpdateProgress};
use crate::request_epoch::RequestEpoch;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;

#[derive(Clone, Copy)]
pub(crate) struct UpdateActions {
    pub(crate) update_epoch: RequestEpoch,
    pub(crate) set_app_update: WriteSignal<Option<AppUpdateInfo>>,
    pub(crate) set_is_update_installing: WriteSignal<bool>,
    pub(crate) set_update_progress: WriteSignal<Option<AppUpdateProgress>>,
    pub(crate) set_global_error: WriteSignal<Option<String>>,
}

impl UpdateActions {
    pub(crate) fn check(&self) {
        let actions = *self;
        let ticket = actions.update_epoch.next();
        spawn_local(async move {
            let result =
                invoke_tauri::<Option<AppUpdateInfo>>("check_app_update", JsValue::UNDEFINED).await;
            if !actions.update_epoch.is_current(ticket) {
                return;
            }

            if let Ok(update) = result {
                actions.set_app_update.set(update);
            }
        });
    }

    pub(crate) fn install(&self) {
        let actions = *self;
        spawn_local(async move {
            actions.set_is_update_installing.set(true);
            actions.set_update_progress.set(None);
            actions.set_global_error.set(None);

            match invoke_tauri::<()>("install_app_update", JsValue::UNDEFINED).await {
                Ok(()) => {}
                Err(error) => {
                    actions.set_is_update_installing.set(false);
                    actions.set_global_error.set(Some(error.user_message));
                }
            }
        });
    }

    pub(crate) fn apply_progress(&self, progress: AppUpdateProgress) {
        let installed = progress.phase == "installed";
        self.set_update_progress.set(Some(progress));
        if installed {
            self.set_is_update_installing.set(false);
        }
    }
}
