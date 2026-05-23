use crate::codex::account_store::default_wovo_codex_root;
use crate::codex::atomic_file::{replace_file, temporary_file_path, write_new_file};
use crate::domain::usage::UsageSnapshot;
use crate::error::AppError;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;
use tokio::sync::Mutex as AsyncMutex;

const AUTH_FILE_NAME: &str = "auth.json";

static ACCOUNT_FETCH_LOCKS: OnceLock<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuthFingerprint {
    len: u64,
    modified_unix_ms: Option<i128>,
    sha256: [u8; 32],
}

pub(crate) async fn fetch_cli_usage_with_runtime_auth<F, Fut>(
    account_id: String,
    managed_home: PathBuf,
    fetch: F,
) -> Result<UsageSnapshot, AppError>
where
    F: FnOnce(String, PathBuf) -> Fut,
    Fut: Future<Output = Result<UsageSnapshot, AppError>>,
{
    let account_lock = lock_for_account(&account_id);
    let _guard = account_lock.lock().await;

    let runtime_home = runtime_home_for_account(&account_id);
    fs::create_dir_all(&runtime_home).map_err(|error| AppError::AuthWrite(error.to_string()))?;

    let managed_auth = managed_home.join(AUTH_FILE_NAME);
    let runtime_auth = runtime_home.join(AUTH_FILE_NAME);
    // Capture the managed fingerprint BEFORE the copy. If we sampled it
    // afterward, a concurrent reauth/OAuth refresh that landed between the
    // copy and the fingerprint would be baked into the baseline and later
    // appear unchanged, letting the post-fetch copy-back overwrite newer
    // managed credentials with a copy derived from the prior managed state.
    let managed_before = auth_fingerprint(&managed_auth)?;
    copy_auth_file_atomically(&managed_auth, &runtime_auth)?;
    let runtime_before = auth_fingerprint(&runtime_auth)?;

    let fetch_result = fetch(account_id, runtime_home.clone()).await;
    let copy_back_result = copy_back_if_runtime_auth_changed(
        &runtime_auth,
        &managed_auth,
        runtime_before,
        managed_before,
    );
    cleanup_runtime_auth(&runtime_auth);

    copy_back_result?;
    fetch_result
}

fn lock_for_account(account_id: &str) -> Arc<AsyncMutex<()>> {
    let locks = ACCOUNT_FETCH_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .expect("Codex account fetch lock registry was poisoned");
    locks
        .entry(account_id.to_string())
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

pub(crate) fn runtime_home_for_account(account_id: &str) -> PathBuf {
    default_wovo_codex_root().join("runtime").join(account_id)
}

pub(crate) fn remove_runtime_home(account_id: &str) {
    let runtime_home = runtime_home_for_account(account_id);
    match fs::remove_dir_all(&runtime_home) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => eprintln!(
            "Codex runtime home cleanup failed for {}: {error}",
            runtime_home.to_string_lossy()
        ),
    }
}

fn copy_back_if_runtime_auth_changed(
    runtime_auth: &Path,
    managed_auth: &Path,
    runtime_before: AuthFingerprint,
    managed_before: AuthFingerprint,
) -> Result<(), AppError> {
    let runtime_after = match auth_fingerprint(runtime_auth) {
        Ok(after) => after,
        // The runtime auth file is staged inside this call and removed in
        // cleanup; if app-server or an external actor deleted it before we
        // could fingerprint, treat it as no change rather than a fetch error.
        Err(AppError::AuthNotFound | AppError::AuthRead(_)) => return Ok(()),
        Err(error) => return Err(error),
    };
    if runtime_after == runtime_before {
        return Ok(());
    }
    // A concurrent reauth, OAuth refresh, or account removal may have moved
    // or rewritten the managed auth file while the CLI fetch was running.
    // Refuse to overwrite a newer file (or recreate one at a stale path)
    // with the runtime copy that was forked from an older managed snapshot.
    match auth_fingerprint(managed_auth) {
        Ok(managed_after) if managed_after == managed_before => {
            copy_auth_file_atomically(runtime_auth, managed_auth)
        }
        Ok(_) => Ok(()),
        Err(AppError::AuthNotFound | AppError::AuthRead(_)) => Ok(()),
        Err(error) => Err(error),
    }
}

fn copy_auth_file_atomically(source: &Path, target: &Path) -> Result<(), AppError> {
    let contents = match fs::read(source) {
        Ok(contents) => contents,
        // Mirror load_credentials_from_home: a missing auth file is an
        // auth-related condition (AuthNotFound is auth_related, AuthRead is
        // not), so callers can surface it as "needs re-authentication".
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::AuthNotFound)
        }
        Err(error) => return Err(AppError::AuthRead(error.to_string())),
    };
    let parent = target.parent().ok_or_else(|| {
        AppError::AuthWrite(format!(
            "auth path has no parent: {}",
            target.to_string_lossy()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| AppError::AuthWrite(error.to_string()))?;
    let tmp = temporary_file_path(parent, AUTH_FILE_NAME);
    write_new_file(&tmp, &contents).map_err(|error| AppError::AuthWrite(error.to_string()))?;
    if let Err(error) = apply_secure_file_permissions(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    replace_file(&tmp, target).map_err(|error| AppError::AuthWrite(error.to_string()))
}

fn auth_fingerprint(path: &Path) -> Result<AuthFingerprint, AppError> {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::AuthNotFound)
        }
        Err(error) => return Err(AppError::AuthRead(error.to_string())),
    };
    let metadata = fs::metadata(path).map_err(|error| AppError::AuthRead(error.to_string()))?;
    let modified_unix_ms = metadata.modified().ok().and_then(|modified| {
        modified
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_millis() as i128)
    });
    let mut hasher = Sha256::new();
    hasher.update(&contents);
    Ok(AuthFingerprint {
        len: metadata.len(),
        modified_unix_ms,
        sha256: hasher.finalize().into(),
    })
}

fn cleanup_runtime_auth(runtime_auth: &Path) {
    if let Err(error) = fs::remove_file(runtime_auth) {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!(
                "Codex runtime auth cleanup failed for {}: {error}",
                runtime_auth.to_string_lossy()
            );
        }
    }
}

#[cfg(unix)]
fn apply_secure_file_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions).map_err(|error| AppError::AuthWrite(error.to_string()))
}

#[cfg(not(unix))]
fn apply_secure_file_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::usage::UsageSnapshot;
    use crate::provider::ProviderSourceMode;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use time::OffsetDateTime;
    use uuid::Uuid;

    fn temp_home(name: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!("wovo-runtime-{name}-{}", Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        home
    }

    fn snapshot(account_id: String) -> UsageSnapshot {
        UsageSnapshot {
            account_id,
            source: "cli".to_string(),
            source_mode: Some(ProviderSourceMode::Cli),
            fetch_attempts: Vec::new(),
            plan_type: None,
            primary: None,
            secondary: None,
            tertiary: None,
            credits: None,
            updated_at: OffsetDateTime::now_utc().unix_timestamp(),
        }
    }

    #[tokio::test]
    async fn runtime_auth_change_is_copied_back_and_cleaned_up() {
        let managed = temp_home("copy-back-managed");
        fs::write(
            managed.join(AUTH_FILE_NAME),
            r#"{"tokens":{"access_token":"old"}}"#,
        )
        .unwrap();
        let account_id = format!("account-{}", Uuid::new_v4());
        let runtime_home = runtime_home_for_account(&account_id);

        let result = fetch_cli_usage_with_runtime_auth(
            account_id.clone(),
            managed.clone(),
            |id, runtime| async move {
                fs::write(
                    runtime.join(AUTH_FILE_NAME),
                    r#"{"tokens":{"access_token":"new"}}"#,
                )
                .unwrap();
                Ok(snapshot(id))
            },
        )
        .await
        .unwrap();

        assert_eq!(result.account_id, account_id);
        assert_eq!(
            fs::read_to_string(managed.join(AUTH_FILE_NAME)).unwrap(),
            r#"{"tokens":{"access_token":"new"}}"#
        );
        assert!(!runtime_home.join(AUTH_FILE_NAME).exists());

        let _ = fs::remove_dir_all(managed);
        let _ = fs::remove_dir_all(runtime_home);
    }

    #[tokio::test]
    async fn concurrent_managed_auth_write_is_not_overwritten() {
        let managed = temp_home("copy-back-race");
        let managed_auth = managed.join(AUTH_FILE_NAME);
        fs::write(&managed_auth, r#"{"tokens":{"access_token":"old"}}"#).unwrap();
        let account_id = format!("account-{}", Uuid::new_v4());
        let runtime_home = runtime_home_for_account(&account_id);
        let managed_auth_in_fetch = managed_auth.clone();

        fetch_cli_usage_with_runtime_auth(
            account_id.clone(),
            managed.clone(),
            move |id, runtime| {
                let managed_auth = managed_auth_in_fetch.clone();
                async move {
                    fs::write(
                        runtime.join(AUTH_FILE_NAME),
                        r#"{"tokens":{"access_token":"runtime-refresh"}}"#,
                    )
                    .unwrap();
                    // Simulate a concurrent reauth/OAuth refresh landing fresh
                    // credentials in the managed home while the fetch ran.
                    fs::write(
                        &managed_auth,
                        r#"{"tokens":{"access_token":"managed-newer"}}"#,
                    )
                    .unwrap();
                    Ok(snapshot(id))
                }
            },
        )
        .await
        .unwrap();

        assert_eq!(
            fs::read_to_string(&managed_auth).unwrap(),
            r#"{"tokens":{"access_token":"managed-newer"}}"#,
            "managed auth written during fetch must win over the runtime copy"
        );
        assert!(!runtime_home.join(AUTH_FILE_NAME).exists());
        let _ = fs::remove_dir_all(managed);
        let _ = fs::remove_dir_all(runtime_home);
    }

    #[tokio::test]
    async fn managed_auth_removed_during_fetch_is_not_recreated() {
        let managed = temp_home("copy-back-removed");
        let managed_auth = managed.join(AUTH_FILE_NAME);
        fs::write(&managed_auth, r#"{"tokens":{"access_token":"old"}}"#).unwrap();
        let account_id = format!("account-{}", Uuid::new_v4());
        let runtime_home = runtime_home_for_account(&account_id);
        let managed_auth_in_fetch = managed_auth.clone();

        fetch_cli_usage_with_runtime_auth(
            account_id.clone(),
            managed.clone(),
            move |id, runtime| {
                let managed_auth = managed_auth_in_fetch.clone();
                async move {
                    fs::write(
                        runtime.join(AUTH_FILE_NAME),
                        r#"{"tokens":{"access_token":"runtime-refresh"}}"#,
                    )
                    .unwrap();
                    // Simulate account removal or home rotation while the
                    // fetch ran.
                    let _ = fs::remove_file(&managed_auth);
                    Ok(snapshot(id))
                }
            },
        )
        .await
        .unwrap();

        assert!(
            !managed_auth.exists(),
            "managed auth must not be recreated at the stale path"
        );
        assert!(!runtime_home.join(AUTH_FILE_NAME).exists());
        let _ = fs::remove_dir_all(managed);
        let _ = fs::remove_dir_all(runtime_home);
    }

    #[tokio::test]
    async fn missing_managed_auth_surfaces_as_auth_not_found() {
        let managed = temp_home("missing-auth-managed");
        // managed dir exists but contains no auth.json
        let account_id = format!("account-{}", Uuid::new_v4());
        let runtime_home = runtime_home_for_account(&account_id);

        let error = fetch_cli_usage_with_runtime_auth(
            account_id.clone(),
            managed.clone(),
            |id, _runtime| async move { Ok(snapshot(id)) },
        )
        .await
        .expect_err("expected an error when managed auth.json is missing");

        assert!(matches!(error, AppError::AuthNotFound));
        assert!(error.auth_related());
        let _ = fs::remove_dir_all(managed);
        let _ = fs::remove_dir_all(runtime_home);
    }

    #[test]
    fn remove_runtime_home_deletes_directory_when_present() {
        let account_id = format!("account-{}", Uuid::new_v4());
        let runtime_home = runtime_home_for_account(&account_id);
        fs::create_dir_all(&runtime_home).unwrap();
        fs::write(runtime_home.join(AUTH_FILE_NAME), b"x").unwrap();
        fs::write(runtime_home.join("logs_2.sqlite"), b"y").unwrap();

        remove_runtime_home(&account_id);

        assert!(!runtime_home.exists());
    }

    #[test]
    fn remove_runtime_home_is_a_noop_when_missing() {
        let account_id = format!("account-{}", Uuid::new_v4());
        let runtime_home = runtime_home_for_account(&account_id);
        assert!(!runtime_home.exists());

        remove_runtime_home(&account_id);

        assert!(!runtime_home.exists());
    }

    #[tokio::test]
    async fn same_account_fetches_are_serialized() {
        let managed = temp_home("lock-managed");
        fs::write(
            managed.join(AUTH_FILE_NAME),
            r#"{"tokens":{"access_token":"old"}}"#,
        )
        .unwrap();
        let account_id = format!("account-{}", Uuid::new_v4());
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..2 {
            let managed = managed.clone();
            let account_id = account_id.clone();
            let active = active.clone();
            let max_active = max_active.clone();
            handles.push(tokio::spawn(async move {
                fetch_cli_usage_with_runtime_auth(account_id, managed, |id, _runtime| async move {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(current, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok(snapshot(id))
                })
                .await
            }));
        }

        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        assert_eq!(max_active.load(Ordering::SeqCst), 1);
        let _ = fs::remove_dir_all(managed);
        let _ = fs::remove_dir_all(runtime_home_for_account(&account_id));
    }
}
