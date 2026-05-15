use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub fn temporary_file_path(parent: &Path, file_name: &str) -> PathBuf {
    parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()))
}

pub fn write_new_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    if let Err(error) = file.write_all(contents) {
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}

pub fn replace_file(tmp: &Path, target: &Path) -> io::Result<()> {
    match replace_file_inner(tmp, target) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(tmp);
            Err(error)
        }
    }
}

#[cfg(not(windows))]
fn replace_file_inner(tmp: &Path, target: &Path) -> io::Result<()> {
    fs::rename(tmp, target)
}

#[cfg(windows)]
fn replace_file_inner(tmp: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let tmp = wide_path(tmp);
    let target = wide_path(target);
    let result = unsafe {
        MoveFileExW(
            tmp.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };

    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}
