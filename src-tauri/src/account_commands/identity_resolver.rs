use crate::codex::account_store::ManagedCodexAccountRecord;
use crate::codex::auth_store::CodexOAuthCredentials;
use crate::domain::account::AccountSummary;
use uuid::Uuid;

use super::live_account_importer::LiveCodexIdentity;

pub(super) fn account_matches_identity(
    account: &AccountSummary,
    email: Option<&str>,
    provider_account_id: Option<&str>,
    workspace_account_id: Option<&str>,
) -> bool {
    identities_match(
        account.email.as_deref(),
        account.provider_account_id.as_deref(),
        account.workspace_account_id.as_deref(),
        email,
        provider_account_id,
        workspace_account_id,
    )
}

pub(super) fn managed_record_matches_credentials(
    account: &ManagedCodexAccountRecord,
    credentials: &CodexOAuthCredentials,
) -> bool {
    let email = credentials.email();
    let provider_account_id = credentials.provider_account_id();
    identities_match(
        account.email.as_deref(),
        account.provider_account_id.as_deref(),
        account.workspace_account_id.as_deref(),
        email.as_deref(),
        provider_account_id.as_deref(),
        None,
    )
}

pub(super) fn live_credential_mirror_home_for_account_with_ambient(
    account: &ManagedCodexAccountRecord,
    ambient: &CodexOAuthCredentials,
) -> bool {
    live_system_account_id_for_credentials(std::slice::from_ref(account), ambient)
        == Some(account.id)
}

pub(super) fn live_system_account_id_for_identity(
    records: &[ManagedCodexAccountRecord],
    live_identity: Option<&LiveCodexIdentity>,
) -> Option<Uuid> {
    let live_identity = live_identity?;
    let preferred_record_id = live_identity.record.as_ref().map(|record| record.id);
    live_system_account_id(
        records,
        live_identity.email.as_deref(),
        live_identity.provider_account_id.as_deref(),
        live_identity.workspace_account_id.as_deref(),
        preferred_record_id,
    )
}

pub(super) fn live_system_account_id_for_credentials(
    records: &[ManagedCodexAccountRecord],
    credentials: &CodexOAuthCredentials,
) -> Option<Uuid> {
    let email = credentials.email();
    let provider_account_id = credentials.provider_account_id();
    live_system_account_id(
        records,
        email.as_deref(),
        provider_account_id.as_deref(),
        None,
        None,
    )
}

fn live_system_account_id(
    records: &[ManagedCodexAccountRecord],
    email: Option<&str>,
    provider_account_id: Option<&str>,
    workspace_account_id: Option<&str>,
    preferred_record_id: Option<Uuid>,
) -> Option<Uuid> {
    if let Some(preferred_record_id) = preferred_record_id {
        if records
            .iter()
            .any(|record| record.id == preferred_record_id)
        {
            return Some(preferred_record_id);
        }
    }

    if let Some(workspace_account_id) = workspace_account_id {
        if let Some(record) = records
            .iter()
            .find(|record| record.workspace_account_id.as_deref() == Some(workspace_account_id))
        {
            return Some(record.id);
        }

        return provider_account_id.and_then(|provider_account_id| {
            records
                .iter()
                .find(|record| {
                    record.workspace_account_id.is_none()
                        && record.provider_account_id.as_deref() == Some(provider_account_id)
                })
                .map(|record| record.id)
        });
    }

    if let Some(provider_account_id) = provider_account_id {
        return records
            .iter()
            .find(|record| record.provider_account_id.as_deref() == Some(provider_account_id))
            .map(|record| record.id);
    }

    records
        .iter()
        .find(|record| {
            record.workspace_account_id.is_none()
                && record.provider_account_id.is_none()
                && emails_match(record.email.as_deref(), email)
        })
        .map(|record| record.id)
}

fn identities_match(
    existing_email: Option<&str>,
    existing_provider_account_id: Option<&str>,
    existing_workspace_account_id: Option<&str>,
    candidate_email: Option<&str>,
    candidate_provider_account_id: Option<&str>,
    candidate_workspace_account_id: Option<&str>,
) -> bool {
    if let Some(candidate_workspace_account_id) = candidate_workspace_account_id {
        if existing_workspace_account_id == Some(candidate_workspace_account_id) {
            return true;
        }
        return existing_workspace_account_id.is_none()
            && candidate_provider_account_id.is_some()
            && existing_provider_account_id == candidate_provider_account_id;
    }

    if let Some(candidate_provider_account_id) = candidate_provider_account_id {
        return existing_provider_account_id == Some(candidate_provider_account_id);
    }

    existing_workspace_account_id.is_none()
        && existing_provider_account_id.is_none()
        && emails_match(existing_email, candidate_email)
}

pub(super) fn record_identity_id(record: &ManagedCodexAccountRecord) -> Option<&str> {
    record
        .workspace_account_id
        .as_deref()
        .or(record.provider_account_id.as_deref())
}

fn emails_match(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
        _ => false,
    }
}
