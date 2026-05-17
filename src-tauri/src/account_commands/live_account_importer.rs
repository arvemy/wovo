use crate::codex::account_store::{ManagedCodexAccountRecord, ManagedCodexAccountStore};
use crate::codex::auth_store::CodexOAuthCredentials;
use crate::codex::workspace_resolver::{self, WorkspaceResolution};
use crate::domain::account::AccountSummary;
use crate::error::AppError;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(super) struct LiveCodexIdentity {
    pub(super) email: Option<String>,
    pub(super) provider_account_id: Option<String>,
    pub(super) workspace_account_id: Option<String>,
    pub(super) record: Option<ManagedCodexAccountRecord>,
}

pub(super) fn ambient_summary_from_credentials(
    credentials: &CodexOAuthCredentials,
) -> AccountSummary {
    AccountSummary::ambient(
        credentials.home_path.to_string_lossy().to_string(),
        credentials.email(),
        credentials.provider_account_id(),
        None,
        None,
    )
}

pub(super) async fn ensure_live_account_imported(
    store: &ManagedCodexAccountStore,
    credentials: &CodexOAuthCredentials,
) -> Result<Option<LiveCodexIdentity>, AppError> {
    let workspace = resolve_workspace_without_failing(credentials).await;
    ensure_live_account_imported_with_workspace(store, credentials, workspace)
}

pub(super) fn ensure_live_account_imported_with_workspace(
    store: &ManagedCodexAccountStore,
    credentials: &CodexOAuthCredentials,
    workspace: Option<WorkspaceResolution>,
) -> Result<Option<LiveCodexIdentity>, AppError> {
    let email = credentials.email();
    let provider_account_id = credentials.provider_account_id();
    let workspace_account_id = workspace
        .as_ref()
        .and_then(|workspace| workspace.account_id.clone());
    let workspace_label = workspace
        .as_ref()
        .and_then(|workspace| workspace.label.clone());
    if email.is_none() && provider_account_id.is_none() && workspace_account_id.is_none() {
        return Ok(None);
    }

    if let Some(existing) = store.find_matching_account(
        email.as_deref(),
        provider_account_id.as_deref(),
        workspace_account_id.as_deref(),
    )? {
        let record = sync_live_account_record(
            store,
            credentials,
            existing.id,
            PathBuf::from(&existing.home_path),
            email.clone(),
            provider_account_id.clone(),
            workspace.clone(),
        )?;
        return Ok(Some(LiveCodexIdentity {
            email,
            provider_account_id,
            workspace_account_id,
            record: Some(record),
        }));
    }

    let preferred_id = Uuid::new_v4();
    let home_path = store.create_home(preferred_id)?;
    let result = (|| {
        store.import_auth_from_home(&credentials.home_path, &home_path)?;
        let (account, replaced_home_paths) = store.upsert_authenticated_account_with_workspace(
            preferred_id,
            email.clone(),
            provider_account_id.clone(),
            workspace_account_id.clone(),
            workspace_label.clone(),
            home_path.clone(),
        )?;
        remove_replaced_homes(store, replaced_home_paths);
        Ok::<ManagedCodexAccountRecord, AppError>(account)
    })();

    match result {
        Ok(record) => Ok(Some(LiveCodexIdentity {
            email,
            provider_account_id,
            workspace_account_id,
            record: Some(record),
        })),
        Err(error) => {
            let _ = store.remove_home_if_safe(&home_path);
            Err(error)
        }
    }
}

pub(super) fn remove_replaced_homes(store: &ManagedCodexAccountStore, home_paths: Vec<PathBuf>) {
    for home_path in home_paths {
        let _ = store.remove_home_if_safe(&home_path);
    }
}

pub(super) async fn resolve_workspace_without_failing(
    credentials: &CodexOAuthCredentials,
) -> Option<WorkspaceResolution> {
    workspace_resolver::resolve_workspace(credentials)
        .await
        .ok()
        .flatten()
}

pub(super) fn canonical_or_original(path: &std::path::Path) -> Result<PathBuf, AppError> {
    path.canonicalize().or_else(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Ok(path.to_path_buf())
        } else {
            Err(AppError::AccountStore(error.to_string()))
        }
    })
}

fn sync_live_account_record(
    store: &ManagedCodexAccountStore,
    credentials: &CodexOAuthCredentials,
    preferred_id: Uuid,
    home_path: PathBuf,
    email: Option<String>,
    provider_account_id: Option<String>,
    workspace: Option<WorkspaceResolution>,
) -> Result<ManagedCodexAccountRecord, AppError> {
    if canonical_or_original(&credentials.home_path)? != canonical_or_original(&home_path)? {
        store.import_auth_from_home(&credentials.home_path, &home_path)?;
    }
    let workspace_account_id = workspace
        .as_ref()
        .and_then(|workspace| workspace.account_id.clone());
    let workspace_label = workspace.and_then(|workspace| workspace.label);
    let (account, replaced_home_paths) = store.upsert_authenticated_account_with_workspace(
        preferred_id,
        email,
        provider_account_id,
        workspace_account_id,
        workspace_label,
        home_path,
    )?;
    remove_replaced_homes(store, replaced_home_paths);
    Ok(account)
}
