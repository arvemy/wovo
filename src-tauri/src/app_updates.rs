use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

use serde::Serialize;
use tauri::{
    utils::{config::BundleType, platform::bundle_type},
    AppHandle, Emitter, State,
};
use tauri_plugin_updater::{Update, UpdaterExt};
use time::format_description::well_known::Rfc3339;

use crate::error::AppError;

const UPDATE_PROGRESS_EVENT: &str = "app:update-progress";

#[derive(Default)]
pub(crate) struct PendingAppUpdate(Mutex<Option<Update>>);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppUpdateInfo {
    version: String,
    current_version: String,
    date: Option<String>,
    body: Option<String>,
    can_install: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppUpdateProgress {
    phase: &'static str,
    downloaded: u64,
    chunk_length: Option<usize>,
    content_length: Option<u64>,
}

#[tauri::command]
pub(crate) async fn check_app_update(
    app: AppHandle,
    pending_update: State<'_, PendingAppUpdate>,
) -> Result<Option<AppUpdateInfo>, AppError> {
    let update = app
        .updater()
        .map_err(update_error)?
        .check()
        .await
        .map_err(update_error)?;
    let info = update.as_ref().map(update_info);

    let mut pending = pending_update
        .0
        .lock()
        .map_err(|_| AppError::AppUpdate("pending update state is unavailable".to_string()))?;
    *pending = update;

    Ok(info)
}

#[tauri::command]
pub(crate) async fn install_app_update(
    app: AppHandle,
    pending_update: State<'_, PendingAppUpdate>,
) -> Result<(), AppError> {
    let update = {
        let pending = pending_update
            .0
            .lock()
            .map_err(|_| AppError::AppUpdate("pending update state is unavailable".to_string()))?;
        pending.clone()
    };
    let Some(update) = update else {
        return Err(AppError::AppUpdate(
            "No pending app update. Check for updates again.".to_string(),
        ));
    };
    if !can_install_app_update() {
        return Err(AppError::AppUpdate(
            "This app build cannot install updates in-app. Install a packaged release manually."
                .to_string(),
        ));
    }

    let progress_app = app.clone();
    let download_finish_app = app.clone();
    let downloaded = Arc::new(AtomicU64::new(0));
    let downloaded_for_progress = downloaded.clone();
    let downloaded_for_finish = downloaded.clone();
    let mut started = false;

    update
        .download_and_install(
            move |chunk_length, content_length| {
                if !started {
                    let _ = progress_app.emit(
                        UPDATE_PROGRESS_EVENT,
                        AppUpdateProgress {
                            phase: "started",
                            downloaded: 0,
                            chunk_length: None,
                            content_length,
                        },
                    );
                    started = true;
                }

                let downloaded = downloaded_for_progress
                    .fetch_add(chunk_length as u64, Ordering::Relaxed)
                    + chunk_length as u64;
                let _ = progress_app.emit(
                    UPDATE_PROGRESS_EVENT,
                    AppUpdateProgress {
                        phase: "progress",
                        downloaded,
                        chunk_length: Some(chunk_length),
                        content_length,
                    },
                );
            },
            move || {
                let downloaded = downloaded_for_finish.load(Ordering::Relaxed);
                let _ = download_finish_app.emit(
                    UPDATE_PROGRESS_EVENT,
                    AppUpdateProgress {
                        phase: "downloaded",
                        downloaded,
                        chunk_length: None,
                        content_length: None,
                    },
                );
            },
        )
        .await
        .map_err(update_error)?;

    {
        let mut pending = pending_update
            .0
            .lock()
            .map_err(|_| AppError::AppUpdate("pending update state is unavailable".to_string()))?;
        *pending = None;
    }

    let downloaded = downloaded.load(Ordering::Relaxed);
    let _ = app.emit(
        UPDATE_PROGRESS_EVENT,
        AppUpdateProgress {
            phase: "installed",
            downloaded,
            chunk_length: None,
            content_length: None,
        },
    );

    app.request_restart();
    Ok(())
}

fn update_info(update: &Update) -> AppUpdateInfo {
    AppUpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        date: update.date.and_then(|date| date.format(&Rfc3339).ok()),
        body: update.body.clone(),
        can_install: can_install_app_update(),
    }
}

fn update_error(error: tauri_plugin_updater::Error) -> AppError {
    AppError::AppUpdate(error.to_string())
}

fn can_install_app_update() -> bool {
    !cfg!(debug_assertions) && can_install_bundle_update(bundle_type())
}

fn can_install_bundle_update(bundle: Option<BundleType>) -> bool {
    matches!(
        bundle,
        Some(
            BundleType::App
                | BundleType::AppImage
                | BundleType::Dmg
                | BundleType::Msi
                | BundleType::Nsis
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_manager_bundles_are_not_in_app_installable() {
        assert!(!can_install_bundle_update(Some(BundleType::Deb)));
        assert!(!can_install_bundle_update(Some(BundleType::Rpm)));
    }

    #[test]
    fn unknown_bundle_types_are_not_in_app_installable() {
        assert!(!can_install_bundle_update(None));
    }

    #[test]
    fn known_non_package_bundles_remain_in_app_installable() {
        assert!(can_install_bundle_update(Some(BundleType::App)));
        assert!(can_install_bundle_update(Some(BundleType::AppImage)));
        assert!(can_install_bundle_update(Some(BundleType::Dmg)));
        assert!(can_install_bundle_update(Some(BundleType::Msi)));
        assert!(can_install_bundle_update(Some(BundleType::Nsis)));
    }

    #[test]
    fn debug_builds_are_not_in_app_installable() {
        if cfg!(debug_assertions) {
            assert!(!can_install_app_update());
        }
    }
}
