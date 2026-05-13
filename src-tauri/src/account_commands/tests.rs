use super::*;
use crate::domain::account::AccountSourceKind;
use std::fs;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("wovo-{name}-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    root
}
fn write_auth(home: &Path, access_token: &str, account_id: &str) {
    fs::create_dir_all(home).unwrap();
    fs::write(
            home.join("auth.json"),
            format!(
                r#"{{"tokens":{{"access_token":"{access_token}","refresh_token":"refresh-{access_token}","account_id":"{account_id}"}}}}"#
            ),
        )
        .unwrap();
}
fn auth_credentials(home: &Path, account_id: &str) -> CodexOAuthCredentials {
    CodexOAuthCredentials {
        access_token: format!("access-{account_id}"),
        refresh_token: format!("refresh-{account_id}"),
        id_token: None,
        account_id: Some(account_id.to_string()),
        last_refresh: None,
        home_path: home.to_path_buf(),
    }
}
fn summary(email: Option<&str>, provider_account_id: Option<&str>) -> AccountSummary {
    AccountSummary {
        id: "test".to_string(),
        label: email.or(provider_account_id).unwrap_or("test").to_string(),
        email: email.map(str::to_string),
        provider_account_id: provider_account_id.map(str::to_string),
        workspace_account_id: None,
        workspace_label: None,
        home_path: "/tmp/codex".to_string(),
        source: AccountSourceKind::Managed,
        authenticated: true,
        is_live_system: false,
        can_set_system: true,
        can_remove: true,
        created_at: None,
        updated_at: None,
        last_authenticated_at: None,
    }
}

mod identity;
mod live_import;
mod summaries;
mod system_account;
