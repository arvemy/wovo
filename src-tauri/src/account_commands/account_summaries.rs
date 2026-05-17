use crate::codex::account_store::ManagedCodexAccountRecord;
use crate::domain::account::AccountSummary;

use super::identity_resolver::{live_system_account_id_for_identity, record_identity_id};
use super::live_account_importer::LiveCodexIdentity;

pub(super) fn summarize_account_list(
    records: Vec<ManagedCodexAccountRecord>,
    live_identity: Option<&LiveCodexIdentity>,
    ambient_fallback: Option<AccountSummary>,
) -> Vec<AccountSummary> {
    let mut summaries = summarize_accounts(records, live_identity);
    if let Some(ambient) = ambient_fallback {
        summaries.push(ambient);
    }
    summaries
}

pub(super) fn summarize_accounts(
    mut records: Vec<ManagedCodexAccountRecord>,
    live_identity: Option<&LiveCodexIdentity>,
) -> Vec<AccountSummary> {
    let live_system_account_id = live_system_account_id_for_identity(&records, live_identity);
    let duplicate_emails = duplicate_emails(&records);
    records.sort_by(|left, right| {
        let left_system = live_system_account_id == Some(left.id);
        let right_system = live_system_account_id == Some(right.id);
        right_system
            .cmp(&left_system)
            .then_with(|| left.email.cmp(&right.email))
            .then_with(|| left.workspace_account_id.cmp(&right.workspace_account_id))
            .then_with(|| left.provider_account_id.cmp(&right.provider_account_id))
            .then_with(|| left.id.cmp(&right.id))
    });

    records
        .into_iter()
        .map(|record| {
            let is_live_system = live_system_account_id == Some(record.id);
            let duplicate_email = record
                .email
                .as_deref()
                .map(|email| duplicate_emails.contains(email))
                .unwrap_or(false);
            let mut summary = record.summary_with_status(is_live_system);
            summary.label = display_label_for_record(&record, duplicate_email);
            summary
        })
        .collect()
}

fn duplicate_emails(records: &[ManagedCodexAccountRecord]) -> std::collections::HashSet<String> {
    let mut counts = std::collections::HashMap::<String, usize>::new();
    for record in records {
        if let Some(email) = record.email.as_deref() {
            *counts.entry(email.to_string()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter_map(|(email, count)| (count > 1).then_some(email))
        .collect()
}

fn display_label_for_record(record: &ManagedCodexAccountRecord, duplicate_email: bool) -> String {
    let identity_id = record_identity_id(record);
    let workspace_label = record.workspace_label.as_deref();
    let is_personal = workspace_label
        .map(|label| label.eq_ignore_ascii_case("personal"))
        .unwrap_or(true);

    if let Some(email) = record.email.as_deref() {
        if duplicate_email || !is_personal {
            let suffix = workspace_label
                .filter(|label| !label.trim().is_empty())
                .map(str::to_string)
                .or_else(|| identity_id.map(short_account_id))
                .unwrap_or_else(|| "workspace".to_string());
            return format!("{email} - {suffix}");
        }
        return email.to_string();
    }

    workspace_label
        .map(str::to_string)
        .or_else(|| identity_id.map(str::to_string))
        .unwrap_or_else(|| "Managed Codex account".to_string())
}

fn short_account_id(account_id: &str) -> String {
    let trimmed = account_id.trim();
    let compact = trimmed.strip_prefix("account-").unwrap_or(trimmed);
    let short: String = compact.chars().take(8).collect();
    if short.is_empty() {
        trimmed.chars().take(8).collect()
    } else {
        short
    }
}
