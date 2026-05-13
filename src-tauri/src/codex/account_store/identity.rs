use super::ManagedCodexAccountRecord;

pub(super) fn sanitized_accounts(
    accounts: Vec<ManagedCodexAccountRecord>,
) -> Vec<ManagedCodexAccountRecord> {
    let mut sanitized = Vec::new();
    for account in accounts {
        let account_identity = account_identity_id(&account).map(str::to_string);
        let duplicate = sanitized
            .iter()
            .any(|existing: &ManagedCodexAccountRecord| {
                let existing_identity_id = account_identity_id(existing);
                existing.id == account.id
                    || (account_identity.is_some()
                        && existing_identity_id == account_identity.as_deref())
                    || (account_identity.is_none()
                        && existing_identity_id.is_none()
                        && existing.email == account.email)
            });
        if !duplicate {
            sanitized.push(account);
        }
    }
    sanitized
}

pub(super) fn find_matching_account_index(
    accounts: &[ManagedCodexAccountRecord],
    email: Option<&str>,
    provider_account_id: Option<&str>,
    workspace_account_id: Option<&str>,
) -> Option<usize> {
    if let Some(workspace_account_id) = workspace_account_id {
        if let Some(index) = accounts.iter().position(|account| {
            account.workspace_account_id.as_deref() == Some(workspace_account_id)
        }) {
            return Some(index);
        }

        return provider_account_id.and_then(|provider_account_id| {
            accounts.iter().position(|account| {
                account.workspace_account_id.is_none()
                    && account.provider_account_id.as_deref() == Some(provider_account_id)
            })
        });
    }

    if let Some(provider_account_id) = provider_account_id {
        return accounts.iter().position(|account| {
            account.provider_account_id.as_deref() == Some(provider_account_id)
        });
    }

    let email = email?;
    accounts.iter().position(|account| {
        account.workspace_account_id.is_none()
            && account.provider_account_id.is_none()
            && account.email.as_deref() == Some(email)
    })
}

pub(super) fn authenticated_identity_matches(
    existing: &ManagedCodexAccountRecord,
    email: Option<&str>,
    provider_account_id: Option<&str>,
    workspace_account_id: Option<&str>,
) -> bool {
    if let Some(workspace_account_id) = workspace_account_id {
        if existing.workspace_account_id.as_deref() == Some(workspace_account_id) {
            return true;
        }
        return existing.workspace_account_id.is_none()
            && provider_account_id.is_some()
            && existing.provider_account_id.as_deref() == provider_account_id;
    }

    if let Some(provider_account_id) = provider_account_id {
        return existing.provider_account_id.as_deref() == Some(provider_account_id);
    }

    existing.workspace_account_id.is_none()
        && existing.provider_account_id.is_none()
        && existing
            .email
            .as_deref()
            .map(|existing_email| {
                email
                    .map(|email| email.eq_ignore_ascii_case(existing_email))
                    .unwrap_or(false)
            })
            .unwrap_or(true)
}

fn account_identity_id(account: &ManagedCodexAccountRecord) -> Option<&str> {
    account
        .workspace_account_id
        .as_deref()
        .or(account.provider_account_id.as_deref())
}

pub(super) fn normalize_optional_email(value: Option<String>) -> Option<String> {
    normalize_optional(value).map(|value| value.to_ascii_lowercase())
}

pub(super) fn normalize_optional(value: Option<String>) -> Option<String> {
    let trimmed = value?.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}
