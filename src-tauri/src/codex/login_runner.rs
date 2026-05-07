use crate::error::AppError;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time;

const MAX_OUTPUT_BYTES: usize = 4_000;
const POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Default)]
pub struct LoginRunnerState {
    active_child: Mutex<Option<Child>>,
}

pub async fn run_login(
    state: &LoginRunnerState,
    home_path: Option<&Path>,
    timeout: Duration,
) -> Result<(), AppError> {
    {
        let active_child = state.active_child.lock().await;
        if active_child.is_some() {
            return Err(AppError::CodexLoginInProgress);
        }
    }

    let mut command = Command::new("codex");
    command.arg("login");
    if let Some(home_path) = home_path {
        command.env("CODEX_HOME", home_path);
    }
    command.kill_on_drop(true);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::CodexBinaryNotFound)
        }
        Err(error) => return Err(AppError::CodexLoginFailed(error.to_string())),
    };

    {
        let mut active_child = state.active_child.lock().await;
        if active_child.is_some() {
            let mut child = child;
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(AppError::CodexLoginInProgress);
        }
        *active_child = Some(child);
    }

    let wait_result = wait_for_active_child(state, timeout).await;
    state.clear_active_child().await;
    let (mut child, status) = wait_result?;
    let stdout = read_child_pipe(child.stdout.take()).await;
    let stderr = read_child_pipe(child.stderr.take()).await;

    if status.success() {
        return Ok(());
    }

    let message = combined_output(&stdout, &stderr);
    Err(AppError::CodexLoginFailed(message))
}

pub async fn cancel_login(state: &LoginRunnerState) -> Result<bool, AppError> {
    let child = {
        let mut active_child = state.active_child.lock().await;
        active_child.take()
    };

    let Some(mut child) = child else {
        return Ok(false);
    };

    child
        .kill()
        .await
        .map_err(|error| AppError::CodexLoginFailed(error.to_string()))?;
    let _ = child.wait().await;
    Ok(true)
}

async fn wait_for_active_child(
    state: &LoginRunnerState,
    timeout: Duration,
) -> Result<(Child, std::process::ExitStatus), AppError> {
    let deadline = time::Instant::now() + timeout;

    loop {
        {
            let mut active_child = state.active_child.lock().await;
            let Some(child) = active_child.as_mut() else {
                return Err(AppError::CodexLoginCancelled);
            };
            match child.try_wait() {
                Ok(Some(status)) => {
                    let child = active_child.take().ok_or(AppError::CodexLoginCancelled)?;
                    return Ok((child, status));
                }
                Ok(None) => {}
                Err(error) => return Err(AppError::CodexLoginFailed(error.to_string())),
            }
        }

        if time::Instant::now() >= deadline {
            let _ = cancel_login(state).await;
            return Err(AppError::CodexLoginTimedOut);
        }

        time::sleep(POLL_INTERVAL).await;
    }
}

async fn read_child_pipe<T>(pipe: Option<T>) -> Vec<u8>
where
    T: tokio::io::AsyncRead + Unpin,
{
    let Some(mut pipe) = pipe else {
        return Vec::new();
    };
    let mut buffer = Vec::new();
    let _ = pipe.read_to_end(&mut buffer).await;
    buffer
}

impl LoginRunnerState {
    async fn clear_active_child(&self) {
        let mut active_child = self.active_child.lock().await;
        *active_child = None;
    }
}

fn combined_output(stdout: &[u8], stderr: &[u8]) -> String {
    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(stdout));
    if !stdout.is_empty() && !stderr.is_empty() {
        combined.push('\n');
    }
    combined.push_str(&String::from_utf8_lossy(stderr));
    let trimmed = combined.trim();
    if trimmed.is_empty() {
        return "codex login exited with a non-zero status".to_string();
    }
    trimmed.chars().take(MAX_OUTPUT_BYTES).collect()
}
