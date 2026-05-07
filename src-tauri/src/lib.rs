mod codex;
mod domain;
mod error;

use codex::auth_store::{detected_ambient_account, load_ambient_credentials, save_credentials};
use codex::token_refresh;
use codex::usage_fetcher;
use domain::account::AccountSummary;
use domain::usage::UsageSnapshot;
use error::AppError;

#[tauri::command]
fn get_detected_codex_account() -> Result<Option<AccountSummary>, AppError> {
    detected_ambient_account()
}

#[tauri::command]
async fn refresh_codex_usage(account_id: String) -> Result<UsageSnapshot, AppError> {
    if account_id != "ambient" {
        return Err(AppError::UnknownAccount(account_id));
    }

    let credentials = load_fresh_ambient_credentials().await?;
    usage_fetcher::fetch_usage("ambient".to_string(), &credentials).await
}

#[tauri::command]
async fn refresh_all_usage() -> Result<Vec<UsageSnapshot>, AppError> {
    match detected_ambient_account()? {
        Some(account) => refresh_codex_usage(account.id)
            .await
            .map(|snapshot| vec![snapshot]),
        None => Ok(Vec::new()),
    }
}

async fn load_fresh_ambient_credentials(
) -> Result<codex::auth_store::CodexOAuthCredentials, AppError> {
    let mut credentials = load_ambient_credentials()?;

    if credentials.needs_refresh() {
        credentials = token_refresh::refresh(credentials).await?;
        save_credentials(&credentials)?;
    }

    Ok(credentials)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_detected_codex_account,
            refresh_codex_usage,
            refresh_all_usage
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
