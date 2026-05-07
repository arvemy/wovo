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
            created_at: Some(created_at),
            updated_at: Some(updated_at),
            last_authenticated_at,
        }
    }
}
