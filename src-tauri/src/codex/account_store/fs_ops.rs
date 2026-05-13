use super::ACCOUNT_LOCAL_CODEX_ENTRIES;
use crate::error::AppError;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[cfg(windows)]
pub(super) fn create_symlink(source: &Path, target: &Path) -> Result<(), AppError> {
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(source, target)
            .or_else(|_| create_directory_junction(source, target))
            .map_err(|error| AppError::AccountStore(error.to_string()))
    } else {
        std::os::windows::fs::symlink_file(source, target).map_err(|error| {
            AppError::AccountStore(format!(
                "failed to create shared Codex file link from {} to {}: {error}",
                target.to_string_lossy(),
                source.to_string_lossy()
            ))
        })
    }
}

#[cfg(windows)]
fn create_directory_junction(source: &Path, target: &Path) -> std::io::Result<()> {
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(target)
        .arg(source)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            ErrorKind::Other,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

#[cfg(all(not(unix), not(windows)))]
pub(super) fn create_symlink(_source: &Path, _target: &Path) -> Result<(), AppError> {
    Err(AppError::AccountStore(
        "directory links are not supported on this platform".to_string(),
    ))
}

#[cfg(unix)]
pub(super) fn create_symlink(source: &Path, target: &Path) -> Result<(), AppError> {
    std::os::unix::fs::symlink(source, target)
        .map_err(|error| AppError::AccountStore(error.to_string()))
}

#[cfg(unix)]
pub(super) fn apply_secure_file_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions)
        .map_err(|error| AppError::AccountStore(error.to_string()))
}

#[cfg(not(unix))]
pub(super) fn apply_secure_file_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

pub(super) fn is_account_local_entry(name: &str) -> bool {
    if ACCOUNT_LOCAL_CODEX_ENTRIES.contains(&name) {
        return true;
    }

    let lower = name.to_ascii_lowercase();
    lower.starts_with("auth.") || lower.contains("token") || lower.contains("credential")
}

pub(super) fn shared_link_points_to(path: &Path, expected_target: &Path) -> Result<bool, AppError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(AppError::AccountStore(error.to_string())),
    };
    if !metadata.file_type().is_symlink() {
        return Ok(false);
    }

    let link_target =
        fs::read_link(path).map_err(|error| AppError::AccountStore(error.to_string()))?;
    let normalized_target = if link_target.is_absolute() {
        link_target
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new(""))
            .join(link_target)
    };
    Ok(canonical_or_original(&normalized_target)? == canonical_or_original(expected_target)?)
}

pub(super) fn promote_entry_to_shared(
    source: &Path,
    target: &Path,
    backup_root: &Path,
) -> Result<(), AppError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| AppError::AccountStore(error.to_string()))?;
    }

    let source_metadata =
        fs::symlink_metadata(source).map_err(|error| AppError::AccountStore(error.to_string()))?;
    let target_metadata = match fs::symlink_metadata(target) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(AppError::AccountStore(error.to_string())),
    };

    match target_metadata {
        None => move_path(source, target),
        Some(target_metadata)
            if source_metadata.is_dir()
                && !source_metadata.file_type().is_symlink()
                && target_metadata.is_dir()
                && !target_metadata.file_type().is_symlink() =>
        {
            merge_dir_contents(source, target, &backup_root.join(path_file_name(source)))?;
            fs::remove_dir_all(source).map_err(|error| AppError::AccountStore(error.to_string()))
        }
        Some(target_metadata)
            if source_metadata.is_file()
                && target_metadata.is_file()
                && same_file_contents(source, target)? =>
        {
            fs::remove_file(source).map_err(|error| AppError::AccountStore(error.to_string()))
        }
        Some(_) => backup_and_remove_path(source, backup_root),
    }
}

fn merge_dir_contents(source: &Path, target: &Path, backup_root: &Path) -> Result<(), AppError> {
    fs::create_dir_all(target).map_err(|error| AppError::AccountStore(error.to_string()))?;
    for entry in fs::read_dir(source).map_err(|error| AppError::AccountStore(error.to_string()))? {
        let entry = entry.map_err(|error| AppError::AccountStore(error.to_string()))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let source_metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| AppError::AccountStore(error.to_string()))?;
        let target_metadata = match fs::symlink_metadata(&target_path) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(AppError::AccountStore(error.to_string())),
        };

        match target_metadata {
            None => move_path(&source_path, &target_path)?,
            Some(target_metadata)
                if source_metadata.is_dir()
                    && !source_metadata.file_type().is_symlink()
                    && target_metadata.is_dir()
                    && !target_metadata.file_type().is_symlink() =>
            {
                merge_dir_contents(
                    &source_path,
                    &target_path,
                    &backup_root.join(entry.file_name()),
                )?;
                fs::remove_dir_all(&source_path)
                    .map_err(|error| AppError::AccountStore(error.to_string()))?;
            }
            Some(target_metadata)
                if source_metadata.is_file()
                    && target_metadata.is_file()
                    && same_file_contents(&source_path, &target_path)? =>
            {
                fs::remove_file(&source_path)
                    .map_err(|error| AppError::AccountStore(error.to_string()))?;
            }
            Some(_) => backup_and_remove_path(&source_path, backup_root)?,
        }
    }
    Ok(())
}

pub(super) fn copy_account_local_entries(
    source_home: &Path,
    target_home: &Path,
) -> Result<(), AppError> {
    fs::create_dir_all(target_home).map_err(|error| AppError::AccountStore(error.to_string()))?;
    for entry in
        fs::read_dir(source_home).map_err(|error| AppError::AccountStore(error.to_string()))?
    {
        let entry = entry.map_err(|error| AppError::AccountStore(error.to_string()))?;
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !is_account_local_entry(name_str) {
            continue;
        }
        let target = target_home.join(&name);
        remove_path_if_exists(&target)?;
        copy_path(&entry.path(), &target)?;
        if name_str == "auth.json" {
            apply_secure_file_permissions(&target)?;
        }
    }
    Ok(())
}

pub(super) fn move_path(source: &Path, target: &Path) -> Result<(), AppError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| AppError::AccountStore(error.to_string()))?;
    }
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_path(source, target)?;
            remove_path_if_exists(source)
        }
    }
}

fn copy_path(source: &Path, target: &Path) -> Result<(), AppError> {
    let metadata =
        fs::symlink_metadata(source).map_err(|error| AppError::AccountStore(error.to_string()))?;
    if metadata.file_type().is_symlink() {
        let link_target =
            fs::read_link(source).map_err(|error| AppError::AccountStore(error.to_string()))?;
        create_symlink(&link_target, target)
    } else if metadata.is_dir() {
        copy_dir_contents(source, target)
    } else {
        fs::copy(source, target)
            .map(|_| ())
            .map_err(|error| AppError::AccountStore(error.to_string()))
    }
}

fn backup_and_remove_path(source: &Path, backup_root: &Path) -> Result<(), AppError> {
    let backup_path = backup_root.join(format!("{}-{}", path_file_name(source), Uuid::new_v4()));
    move_path(source, &backup_path)
}

fn path_file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn same_file_contents(left: &Path, right: &Path) -> Result<bool, AppError> {
    let left = fs::read(left).map_err(|error| AppError::AccountStore(error.to_string()))?;
    let right = fs::read(right).map_err(|error| AppError::AccountStore(error.to_string()))?;
    Ok(left == right)
}

fn copy_dir_contents(source: &Path, target: &Path) -> Result<(), AppError> {
    fs::create_dir_all(target).map_err(|error| AppError::AccountStore(error.to_string()))?;
    for entry in fs::read_dir(source).map_err(|error| AppError::AccountStore(error.to_string()))? {
        let entry = entry.map_err(|error| AppError::AccountStore(error.to_string()))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());

        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| AppError::AccountStore(error.to_string()))?;
        if let Ok(target_metadata) = fs::symlink_metadata(&target_path) {
            if metadata.is_dir()
                && target_metadata.is_dir()
                && !target_metadata.file_type().is_symlink()
            {
                copy_dir_contents(&source_path, &target_path)?;
            }
            continue;
        }

        if metadata.file_type().is_symlink() {
            let link_target = fs::read_link(&source_path)
                .map_err(|error| AppError::AccountStore(error.to_string()))?;
            create_symlink(&link_target, &target_path)?;
        } else if metadata.is_dir() {
            copy_dir_contents(&source_path, &target_path)?;
        } else {
            fs::copy(&source_path, &target_path)
                .map_err(|error| AppError::AccountStore(error.to_string()))?;
        }
    }
    Ok(())
}

pub(super) fn directory_is_empty(path: &Path) -> Result<bool, AppError> {
    let mut entries =
        fs::read_dir(path).map_err(|error| AppError::AccountStore(error.to_string()))?;
    Ok(entries.next().is_none())
}

pub(super) fn cleanup_error_if_unsafe(result: Result<(), AppError>) -> Result<(), AppError> {
    match result {
        Ok(()) => Ok(()),
        Err(error @ AppError::UnsafeManagedHome(_)) => Err(error),
        Err(_) => Ok(()),
    }
}

pub(super) fn remove_path_if_exists(path: &Path) -> Result<(), AppError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => remove_symlink_path(path),
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path).map_err(|error| AppError::AccountStore(error.to_string()))
        }
        Ok(_) => fs::remove_file(path).map_err(|error| AppError::AccountStore(error.to_string())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::AccountStore(error.to_string())),
    }
}

#[cfg(windows)]
pub(super) fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(windows)]
pub(super) fn remove_symlink_path(path: &Path) -> Result<(), AppError> {
    if fs::metadata(path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
    {
        fs::remove_dir(path).map_err(|error| AppError::AccountStore(error.to_string()))
    } else {
        fs::remove_file(path).map_err(|error| AppError::AccountStore(error.to_string()))
    }
}

#[cfg(not(windows))]
pub(super) fn remove_symlink_path(path: &Path) -> Result<(), AppError> {
    fs::remove_file(path).map_err(|error| AppError::AccountStore(error.to_string()))
}

pub(super) fn canonical_or_original(path: &Path) -> Result<PathBuf, AppError> {
    path.canonicalize().or_else(|error| {
        if error.kind() == ErrorKind::NotFound {
            Ok(path.to_path_buf())
        } else {
            Err(AppError::AccountStore(error.to_string()))
        }
    })
}
