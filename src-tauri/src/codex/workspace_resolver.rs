use crate::codex::auth_store::CodexOAuthCredentials;
use crate::error::AppError;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

const ACCOUNTS_ENDPOINT: &str = "https://chatgpt.com/backend-api/accounts";
const DEFAULT_WORKSPACE_LABEL: &str = "Personal";
const WORKSPACE_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceResolution {
    pub account_id: Option<String>,
    pub label: Option<String>,
}

pub async fn resolve_workspace(
    credentials: &CodexOAuthCredentials,
) -> Result<Option<WorkspaceResolution>, AppError> {
    let preferred_account_id = credentials.provider_account_id();
    let client = Client::builder()
        .timeout(WORKSPACE_RESOLUTION_TIMEOUT)
        .build()
        .map_err(|error| AppError::UsageFetch(error.to_string()))?;
    let mut request = client
        .get(ACCOUNTS_ENDPOINT)
        .bearer_auth(&credentials.access_token)
        .header("Accept", "application/json")
        .header("User-Agent", "codex-cli");

    if let Some(account_id) = preferred_account_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        request = request.header("ChatGPT-Account-Id", account_id);
    }

    let response = request
        .send()
        .await
        .map_err(|error| AppError::UsageFetch(error.to_string()))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| AppError::UsageFetch(error.to_string()))?;

    if !status.is_success() {
        return Err(AppError::UsageFetch(format!("status {}", status.as_u16())));
    }

    workspace_from_accounts_response(&body, preferred_account_id.as_deref())
}

pub fn workspace_from_accounts_response(
    body: &str,
    preferred_account_id: Option<&str>,
) -> Result<Option<WorkspaceResolution>, AppError> {
    let value: Value =
        serde_json::from_str(body).map_err(|error| AppError::UsageFetch(error.to_string()))?;
    Ok(workspace_from_accounts_value(
        &value,
        normalize_optional_str(preferred_account_id).as_deref(),
    ))
}

fn workspace_from_accounts_value(
    value: &Value,
    preferred_account_id: Option<&str>,
) -> Option<WorkspaceResolution> {
    let accounts = account_values(value);
    let selected = select_account_value(value, &accounts, preferred_account_id);

    let account_id = selected
        .and_then(account_id_from_value)
        .or_else(|| normalize_optional_str(preferred_account_id));
    let label = selected.and_then(workspace_label_from_value);

    if account_id.is_none() && label.is_none() {
        return None;
    }

    Some(WorkspaceResolution {
        account_id,
        label: Some(label.unwrap_or_else(|| DEFAULT_WORKSPACE_LABEL.to_string())),
    })
}

fn account_values(value: &Value) -> Vec<&Value> {
    if let Some(items) = value.as_array() {
        return items.iter().collect();
    }

    let Some(object) = value.as_object() else {
        return Vec::new();
    };

    for key in ["accounts", "items", "data"] {
        if let Some(items) = object.get(key).and_then(Value::as_array) {
            return items.iter().collect();
        }
    }

    if let Some(account) = object.get("account") {
        return vec![account];
    }

    vec![value]
}

fn select_account_value<'a>(
    root: &'a Value,
    accounts: &[&'a Value],
    preferred_account_id: Option<&str>,
) -> Option<&'a Value> {
    if let Some(preferred_account_id) = preferred_account_id {
        if let Some(account) = accounts
            .iter()
            .copied()
            .find(|account| account_id_from_value(account).as_deref() == Some(preferred_account_id))
        {
            return Some(account);
        }
    }

    let default_account_id = account_id_from_keys(
        root,
        &[
            "current_account_id",
            "currentAccountId",
            "default_account_id",
            "defaultAccountId",
            "selected_account_id",
            "selectedAccountId",
        ],
    );
    if let Some(default_account_id) = default_account_id {
        if let Some(account) = accounts
            .iter()
            .copied()
            .find(|account| account_id_from_value(account).as_deref() == Some(&default_account_id))
        {
            return Some(account);
        }
    }

    accounts.first().copied()
}

fn account_id_from_value(value: &Value) -> Option<String> {
    account_id_from_keys(
        value,
        &[
            "account_id",
            "accountId",
            "id",
            "workspace_account_id",
            "workspaceAccountId",
            "chatgpt_account_id",
            "chatgptAccountId",
        ],
    )
}

fn account_id_from_keys(value: &Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .and_then(|value| normalize_optional_str(Some(value)))
}

fn workspace_label_from_value(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    [
        "workspace_name",
        "workspaceName",
        "name",
        "label",
        "title",
        "account_name",
        "accountName",
    ]
    .iter()
    .find_map(|key| object.get(*key).and_then(Value::as_str))
    .and_then(|value| normalize_optional_str(Some(value)))
}

fn normalize_optional_str(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_selected_workspace() {
        let workspace = workspace_from_accounts_response(
            r#"{
                "accounts": [
                    {"account_id": " account-one ", "workspace_name": "Personal"},
                    {"account_id": "account-two", "workspace_name": "Team Workspace"}
                ],
                "default_account_id": "account-one"
            }"#,
            Some("account-two"),
        )
        .unwrap()
        .unwrap();

        assert_eq!(workspace.account_id.as_deref(), Some("account-two"));
        assert_eq!(workspace.label.as_deref(), Some("Team Workspace"));
    }

    #[test]
    fn falls_back_to_personal_label() {
        let workspace = workspace_from_accounts_response(
            r#"{"accounts":[{"account_id":"account-one"}]}"#,
            Some("account-one"),
        )
        .unwrap()
        .unwrap();

        assert_eq!(workspace.account_id.as_deref(), Some("account-one"));
        assert_eq!(workspace.label.as_deref(), Some("Personal"));
    }
}
