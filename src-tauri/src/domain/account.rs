use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountSourceKind {
    Ambient,
    Managed,
    ManualPath,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub id: String,
    pub label: String,
    pub email: Option<String>,
    pub provider_account_id: Option<String>,
    pub workspace_account_id: Option<String>,
    pub workspace_label: Option<String>,
    pub home_path: String,
    pub source: AccountSourceKind,
    pub authenticated: bool,
    pub is_live_system: bool,
    pub can_set_system: bool,
    pub can_remove: bool,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub last_authenticated_at: Option<i64>,
}

impl AccountSummary {
    pub fn ambient(
        home_path: String,
        email: Option<String>,
        provider_account_id: Option<String>,
        workspace_account_id: Option<String>,
        workspace_label: Option<String>,
    ) -> Self {
        let label = email
            .clone()
            .or_else(|| workspace_label.clone())
            .or_else(|| workspace_account_id.clone())
            .or_else(|| provider_account_id.clone())
            .unwrap_or_else(|| "Local Codex account".to_string());

        Self {
            id: "ambient".to_string(),
            label,
            email,
            provider_account_id,
            workspace_account_id,
            workspace_label,
            home_path,
            source: AccountSourceKind::Ambient,
            authenticated: true,
            is_live_system: true,
            can_set_system: false,
            can_remove: false,
            created_at: None,
            updated_at: None,
            last_authenticated_at: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn managed(
        id: String,
        email: Option<String>,
        provider_account_id: Option<String>,
        workspace_account_id: Option<String>,
        workspace_label: Option<String>,
        home_path: String,
        created_at: i64,
        updated_at: i64,
        last_authenticated_at: Option<i64>,
        is_live_system: bool,
    ) -> Self {
        let label = email
            .clone()
            .or_else(|| workspace_label.clone())
            .or_else(|| workspace_account_id.clone())
            .or_else(|| provider_account_id.clone())
            .unwrap_or_else(|| "Managed Codex account".to_string());

        Self {
            id,
            label,
            email,
            provider_account_id,
            workspace_account_id,
            workspace_label,
            home_path,
            source: AccountSourceKind::Managed,
            authenticated: true,
            is_live_system,
            can_set_system: !is_live_system,
            can_remove: !is_live_system,
            created_at: Some(created_at),
            updated_at: Some(updated_at),
            last_authenticated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_system_managed_accounts_cannot_be_removed() {
        let account = AccountSummary::managed(
            "account-id".to_string(),
            Some("user@example.com".to_string()),
            Some("provider-id".to_string()),
            Some("workspace-id".to_string()),
            Some("Personal".to_string()),
            "/tmp/codex".to_string(),
            1,
            2,
            Some(3),
            true,
        );

        assert!(!account.can_set_system);
        assert!(!account.can_remove);
    }
}
