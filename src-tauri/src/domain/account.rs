use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub home_path: String,
    pub source: AccountSourceKind,
    pub authenticated: bool,
    pub is_active: bool,
    pub is_live_system: bool,
    pub can_switch: bool,
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
    ) -> Self {
        let label = email
            .clone()
            .or_else(|| provider_account_id.clone())
            .unwrap_or_else(|| "Local Codex account".to_string());

        Self {
            id: "ambient".to_string(),
            label,
            email,
            provider_account_id,
            home_path,
            source: AccountSourceKind::Ambient,
            authenticated: true,
            is_active: false,
            is_live_system: true,
            can_switch: false,
            can_remove: false,
            created_at: None,
            updated_at: None,
            last_authenticated_at: None,
        }
    }

    pub fn managed(
        id: String,
        email: Option<String>,
        provider_account_id: Option<String>,
        home_path: String,
        created_at: i64,
        updated_at: i64,
        last_authenticated_at: Option<i64>,
        is_active: bool,
        is_live_system: bool,
    ) -> Self {
        let label = email
            .clone()
            .or_else(|| provider_account_id.clone())
            .unwrap_or_else(|| "Managed Codex account".to_string());

        Self {
            id,
            label,
            email,
            provider_account_id,
            home_path,
            source: AccountSourceKind::Managed,
            authenticated: true,
            is_active,
            is_live_system,
            can_switch: !is_active,
            can_remove: !is_active && !is_live_system,
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
            "/tmp/codex".to_string(),
            1,
            2,
            Some(3),
            false,
            true,
        );

        assert!(account.can_switch);
        assert!(!account.can_remove);
    }
}
