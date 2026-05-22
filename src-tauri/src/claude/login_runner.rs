use crate::claude::account_store::default_wovo_claude_root;
use crate::error::AppError;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, PtySize};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const MAX_OUTPUT_BYTES: usize = 12_000;
const MAX_PTY_RAW_BYTES: usize = 64_000;
const PTY_READ_INTERVAL: Duration = Duration::from_millis(100);
const PTY_STABLE_AFTER_READY: Duration = Duration::from_millis(1_200);
const PTY_ROWS: u16 = 34;
const PTY_COLS: u16 = 120;
const PTY_SCROLLBACK: usize = 120;

#[derive(Default)]
pub struct ClaudeLoginRunnerState {
    active_login: Mutex<Option<ActiveClaudeLogin>>,
}

struct ActiveClaudeLogin {
    killer: Box<dyn ChildKiller + Send + Sync>,
    cancelled: Arc<AtomicBool>,
}

struct SpawnedPtyChild {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    receiver: mpsc::Receiver<Vec<u8>>,
    writer: Box<dyn Write + Send>,
}

pub async fn run_login(
    state: &ClaudeLoginRunnerState,
    home_path: Option<&Path>,
    timeout: Duration,
) -> Result<(), AppError> {
    {
        let active_login = state.active_login.lock().await;
        if active_login.is_some() {
            return Err(AppError::ClaudeLoginInProgress);
        }
    }

    let mut spawned = spawn_login_pty(home_path)?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let killer = spawned.child.clone_killer();

    {
        let mut active_login = state.active_login.lock().await;
        if active_login.is_some() {
            let _ = spawned.child.kill();
            let _ = spawned.child.wait();
            return Err(AppError::ClaudeLoginInProgress);
        }
        *active_login = Some(ActiveClaudeLogin {
            killer,
            cancelled: cancelled.clone(),
        });
    }

    let cleanup_cancelled = cancelled.clone();
    let wait_result = tokio::task::spawn_blocking(move || {
        wait_for_login_pty(
            spawned.child,
            spawned.receiver,
            spawned.writer,
            timeout,
            cancelled.clone(),
        )
    })
    .await;
    state
        .clear_active_login_if_current(&cleanup_cancelled)
        .await;
    wait_result.map_err(|error| AppError::ClaudeLoginFailed(error.to_string()))?
}

pub async fn cancel_login(state: &ClaudeLoginRunnerState) -> Result<bool, AppError> {
    let active_login = {
        let mut active_login = state.active_login.lock().await;
        active_login.take()
    };

    let Some(mut active_login) = active_login else {
        return Ok(false);
    };

    active_login.cancelled.store(true, Ordering::Release);
    active_login
        .killer
        .kill()
        .map_err(|error| AppError::ClaudeLoginFailed(error.to_string()))?;
    Ok(true)
}

fn spawn_login_pty(home_path: Option<&Path>) -> Result<SpawnedPtyChild, AppError> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: PTY_ROWS,
            cols: PTY_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| AppError::ClaudeLoginFailed(error.to_string()))?;

    let mut command = CommandBuilder::new("claude");
    command.arg("/login");
    command.cwd(
        ensure_cli_workspace_dir()
            .map_err(|error| AppError::ClaudeLoginFailed(error.to_string()))?
            .as_os_str(),
    );
    if let Some(home_path) = home_path {
        command.env("CLAUDE_CONFIG_DIR", home_path);
    }
    command.env("TERM", "xterm-256color");

    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| map_login_pty_spawn_error(error.to_string()))?;
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| AppError::ClaudeLoginFailed(error.to_string()))?;
    let receiver = spawn_pty_reader(reader);
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| AppError::ClaudeLoginFailed(error.to_string()))?;
    Ok(SpawnedPtyChild {
        child,
        receiver,
        writer,
    })
}

fn wait_for_login_pty(
    mut child: Box<dyn portable_pty::Child + Send + Sync>,
    receiver: mpsc::Receiver<Vec<u8>>,
    mut writer: Box<dyn Write + Send>,
    timeout_duration: Duration,
    cancelled: Arc<AtomicBool>,
) -> Result<(), AppError> {
    let mut parser = vt100::Parser::new(PTY_ROWS, PTY_COLS, PTY_SCROLLBACK);
    let mut raw = Vec::new();
    let started_at = Instant::now();
    let deadline = started_at + timeout_duration;
    let mut prompt_state = ClaudeLoginPromptState::default();

    loop {
        if cancelled.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::ClaudeLoginCancelled);
        }

        match receiver.recv_timeout(PTY_READ_INTERVAL) {
            Ok(chunk) => {
                parser.process(&chunk);
                push_raw_limited(&mut raw, &chunk);
                let screen = parser.screen().contents();
                handle_login_prompt(&screen, &mut prompt_state, writer.as_mut());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let output = terminal_capture_text(&parser, &raw);
                if cancelled.load(Ordering::Acquire) {
                    return Err(AppError::ClaudeLoginCancelled);
                }
                return finish_login_pty_exit(child.as_mut(), output);
            }
        }

        let output = terminal_capture_text(&parser, &raw);
        if let Some(status) = child
            .try_wait()
            .map_err(|error| AppError::ClaudeLoginFailed(error.to_string()))?
        {
            if cancelled.load(Ordering::Acquire) {
                return Err(AppError::ClaudeLoginCancelled);
            }
            return finish_login_status(status.success(), &output);
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::ClaudeLoginTimedOut);
        }
    }
}

pub async fn run_slash_command(
    home_path: &Path,
    slash_command: &str,
    timeout_duration: Duration,
) -> Result<String, AppError> {
    let home_path = home_path.to_path_buf();
    let slash_command = slash_command.to_string();
    tokio::task::spawn_blocking(move || {
        run_slash_command_pty_blocking(&home_path, &slash_command, timeout_duration)
    })
    .await
    .map_err(|error| AppError::ClaudeUsageFetch(error.to_string()))?
}

fn run_slash_command_pty_blocking(
    home_path: &Path,
    slash_command: &str,
    timeout_duration: Duration,
) -> Result<String, AppError> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: PTY_ROWS,
            cols: PTY_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| AppError::ClaudeUsageFetch(error.to_string()))?;

    let mut command = CommandBuilder::new("claude");
    command.arg(slash_command);
    command.cwd(
        ensure_cli_workspace_dir()
            .map_err(|error| AppError::ClaudeUsageFetch(error.to_string()))?
            .as_os_str(),
    );
    command.env("CLAUDE_CONFIG_DIR", home_path);
    command.env("TERM", "xterm-256color");

    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| map_pty_spawn_error(error.to_string()))?;
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| AppError::ClaudeUsageFetch(error.to_string()))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|error| AppError::ClaudeUsageFetch(error.to_string()))?;
    let receiver = spawn_pty_reader(reader);
    let mut parser = vt100::Parser::new(PTY_ROWS, PTY_COLS, PTY_SCROLLBACK);
    let mut raw = Vec::new();
    let started_at = Instant::now();
    let deadline = started_at + timeout_duration;
    let mut ready_since = None;
    let mut prompt_state = SlashCommandPromptState::default();

    loop {
        match receiver.recv_timeout(PTY_READ_INTERVAL) {
            Ok(chunk) => {
                parser.process(&chunk);
                push_raw_limited(&mut raw, &chunk);
                let screen = parser.screen().contents();
                handle_slash_command_prompt(&screen, &mut prompt_state, writer.as_mut());
                if slash_command_blocked_by_login_setup(slash_command, &screen) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::ClaudeUsageFetch(
                        "Claude CLI requires interactive login setup".to_string(),
                    ));
                }
                update_slash_command_ready_since(
                    &mut ready_since,
                    slash_command_ready(slash_command, &screen),
                    Instant::now(),
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let output = terminal_capture_text(&parser, &raw);
                return finish_pty_exit(child.as_mut(), output);
            }
        }

        let output = terminal_capture_text(&parser, &raw);
        if let Some(status) = child
            .try_wait()
            .map_err(|error| AppError::ClaudeUsageFetch(error.to_string()))?
        {
            if status.success() {
                return Ok(trimmed_output(&output, "Claude CLI exited without output"));
            }
            return Err(AppError::ClaudeUsageFetch(trimmed_output(
                &output,
                "Claude CLI exited with a non-zero status",
            )));
        }

        if ready_since
            .map(|ready_at| ready_at.elapsed() >= PTY_STABLE_AFTER_READY)
            .unwrap_or(false)
        {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(trimmed_output(&output, "Claude CLI exited without output"));
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            if slash_command_ready(slash_command, &parser.screen().contents()) {
                return Ok(trimmed_output(&output, "Claude CLI exited without output"));
            }
            return Err(AppError::ClaudeUsageFetch(
                "Claude CLI command timed out".to_string(),
            ));
        }
    }
}

impl ClaudeLoginRunnerState {
    async fn clear_active_login_if_current(&self, cancelled: &Arc<AtomicBool>) {
        let mut active_login = self.active_login.lock().await;
        if active_login
            .as_ref()
            .is_some_and(|active_login| Arc::ptr_eq(&active_login.cancelled, cancelled))
        {
            *active_login = None;
        }
    }
}

fn map_pty_spawn_error(message: String) -> AppError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("no such file")
        || lower.contains("not found")
        || lower.contains("cannot find")
        || lower.contains("os error 2")
    {
        AppError::ClaudeBinaryNotFound
    } else {
        AppError::ClaudeUsageFetch(message)
    }
}

fn map_login_pty_spawn_error(message: String) -> AppError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("no such file")
        || lower.contains("not found")
        || lower.contains("cannot find")
        || lower.contains("os error 2")
    {
        AppError::ClaudeBinaryNotFound
    } else {
        AppError::ClaudeLoginFailed(message)
    }
}

fn spawn_pty_reader(mut reader: Box<dyn Read + Send>) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if sender.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    });
    receiver
}

fn finish_pty_exit(
    child: &mut dyn portable_pty::Child,
    output: String,
) -> Result<String, AppError> {
    match child.try_wait() {
        Ok(Some(status)) if status.success() => {
            Ok(trimmed_output(&output, "Claude CLI exited without output"))
        }
        Ok(Some(_)) => Err(AppError::ClaudeUsageFetch(trimmed_output(
            &output,
            "Claude CLI exited with a non-zero status",
        ))),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            Ok(trimmed_output(&output, "Claude CLI exited without output"))
        }
        Err(error) => Err(AppError::ClaudeUsageFetch(error.to_string())),
    }
}

fn finish_login_pty_exit(
    child: &mut dyn portable_pty::Child,
    output: String,
) -> Result<(), AppError> {
    match child.try_wait() {
        Ok(Some(status)) => finish_login_status(status.success(), &output),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(AppError::ClaudeLoginFailed(trimmed_output(
                &output,
                "Claude CLI exited without a status",
            )))
        }
        Err(error) => Err(AppError::ClaudeLoginFailed(error.to_string())),
    }
}

fn finish_login_status(success: bool, output: &str) -> Result<(), AppError> {
    if success && !login_command_unavailable(output) {
        return Ok(());
    }

    Err(AppError::ClaudeLoginFailed(trimmed_output(
        output,
        "Claude CLI exited with a non-zero status",
    )))
}

fn login_command_unavailable(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("/login isn't available") || lower.contains("/login is not available")
}

fn push_raw_limited(raw: &mut Vec<u8>, chunk: &[u8]) {
    raw.extend_from_slice(chunk);
    if raw.len() > MAX_PTY_RAW_BYTES {
        let excess = raw.len() - MAX_PTY_RAW_BYTES;
        raw.drain(..excess);
    }
}

fn terminal_capture_text(parser: &vt100::Parser, raw: &[u8]) -> String {
    let screen = parser.screen().contents();
    let raw = String::from_utf8_lossy(raw);
    if screen.trim().is_empty() {
        raw.into_owned()
    } else if raw.trim().is_empty() {
        screen
    } else {
        format!("{}\n{}", screen.trim_end(), raw)
    }
}

fn slash_command_ready(slash_command: &str, text: &str) -> bool {
    match slash_command {
        "/usage" => usage_command_ready(text),
        "/status" => status_command_ready(text),
        _ => false,
    }
}

fn update_slash_command_ready_since(
    ready_since: &mut Option<Instant>,
    is_ready: bool,
    now: Instant,
) {
    *ready_since = is_ready.then_some(now);
}

fn usage_command_ready(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let has_percent =
        lower.contains("% used") || lower.contains("% left") || lower.contains("% remaining");
    let has_usage_window = lower.contains("current session") || lower.contains("current week");
    has_usage_window && has_percent && !lower.contains("loading usage data")
}

fn status_command_ready(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("login method:")
        || lower.contains("email:")
        || lower.contains("/status isn't available")
}

fn slash_command_blocked_by_login_setup(slash_command: &str, text: &str) -> bool {
    if slash_command != "/usage" {
        return false;
    }

    let lower = text.to_ascii_lowercase();
    lower.contains("select login method")
        || lower.contains("browser didn't open")
        || lower.contains("opening browser to sign in")
        || lower.contains("paste code here if prompted")
}

fn ensure_cli_workspace_dir() -> std::io::Result<std::path::PathBuf> {
    let path = default_wovo_claude_root().join("cli-workspace");
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn should_accept_workspace_trust_prompt(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("yes, i trust this folder")
        && (lower.contains("quick safety check")
            || lower.contains("project you created")
            || lower.contains("workspace"))
}

fn should_select_default_usage_source(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("claude account with subscription")
        && (lower.contains("api usage billing") || lower.contains("third-party platform"))
}

fn should_select_default_login_method(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let asks_for_login_method = (lower.contains("login method")
        || lower.contains("log in")
        || lower.contains("login"))
        && (lower.contains("choose") || lower.contains("select") || lower.contains("how do you"));
    let has_claude_subscription_option =
        lower.contains("claude account with subscription") || lower.contains("subscription plan");
    let has_console_option =
        lower.contains("anthropic console") || lower.contains("api usage billing");
    asks_for_login_method && has_claude_subscription_option && has_console_option
}

#[derive(Default)]
struct ClaudeLoginPromptState {
    accepted_first_run_prompt: bool,
    accepted_workspace_trust_prompt: bool,
    selected_default_login_method: bool,
}

fn handle_login_prompt<W: Write + ?Sized>(
    text: &str,
    state: &mut ClaudeLoginPromptState,
    writer: &mut W,
) {
    if !state.accepted_first_run_prompt && should_accept_first_run_prompt(text) {
        let _ = writer.write_all(b"\r");
        let _ = writer.flush();
        state.accepted_first_run_prompt = true;
        return;
    }

    if !state.accepted_workspace_trust_prompt && should_accept_workspace_trust_prompt(text) {
        let _ = writer.write_all(b"\r");
        let _ = writer.flush();
        state.accepted_workspace_trust_prompt = true;
        return;
    }

    if !state.selected_default_login_method && should_select_default_login_method(text) {
        let _ = writer.write_all(b"\r");
        let _ = writer.flush();
        state.selected_default_login_method = true;
    }
}

#[derive(Default)]
struct SlashCommandPromptState {
    accepted_first_run_prompt: bool,
    accepted_workspace_trust_prompt: bool,
    selected_default_usage_source: bool,
}

fn handle_slash_command_prompt<W: Write + ?Sized>(
    text: &str,
    state: &mut SlashCommandPromptState,
    writer: &mut W,
) {
    if !state.accepted_first_run_prompt && should_accept_first_run_prompt(text) {
        let _ = writer.write_all(b"\r");
        let _ = writer.flush();
        state.accepted_first_run_prompt = true;
        return;
    }

    if !state.accepted_workspace_trust_prompt && should_accept_workspace_trust_prompt(text) {
        let _ = writer.write_all(b"\r");
        let _ = writer.flush();
        state.accepted_workspace_trust_prompt = true;
        return;
    }

    if !state.selected_default_usage_source && should_select_default_usage_source(text) {
        let _ = writer.write_all(b"\r");
        let _ = writer.flush();
        state.selected_default_usage_source = true;
    }
}

fn should_accept_first_run_prompt(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("theme")
        && (lower.contains("choose") || lower.contains("select") || lower.contains("welcome"))
        && (lower.contains("dark") || lower.contains("light"))
}

fn trimmed_output(text: &str, empty_message: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return empty_message.to_string();
    }
    trimmed.chars().take(MAX_OUTPUT_BYTES).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[derive(Debug, Clone)]
    struct TestKiller;

    impl ChildKiller for TestKiller {
        fn kill(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
            Box::new(self.clone())
        }
    }

    fn test_active_login(cancelled: Arc<AtomicBool>) -> ActiveClaudeLogin {
        ActiveClaudeLogin {
            killer: Box::new(TestKiller),
            cancelled,
        }
    }

    #[tokio::test]
    async fn clear_active_login_removes_matching_login_after_completion() {
        let state = ClaudeLoginRunnerState::default();
        let cancelled = Arc::new(AtomicBool::new(false));
        {
            let mut active_login = state.active_login.lock().await;
            *active_login = Some(test_active_login(cancelled.clone()));
        }

        state.clear_active_login_if_current(&cancelled).await;

        let active_login = state.active_login.lock().await;
        assert!(active_login.is_none());
    }

    #[tokio::test]
    async fn clear_active_login_keeps_newer_login_after_cancelled_run_finishes() {
        let state = ClaudeLoginRunnerState::default();
        let old_cancelled = Arc::new(AtomicBool::new(false));
        let new_cancelled = Arc::new(AtomicBool::new(false));
        {
            let mut active_login = state.active_login.lock().await;
            *active_login = Some(test_active_login(old_cancelled.clone()));
        }
        {
            let mut active_login = state.active_login.lock().await;
            let _cancelled_login = active_login.take();
            *active_login = Some(test_active_login(new_cancelled.clone()));
        }

        state.clear_active_login_if_current(&old_cancelled).await;

        let active_login = state.active_login.lock().await;
        assert!(active_login
            .as_ref()
            .is_some_and(|active_login| Arc::ptr_eq(&active_login.cancelled, &new_cancelled)));
    }

    #[test]
    fn usage_command_ready_accepts_rendered_usage_panel() {
        assert!(usage_command_ready(
            r#"
            Current session                                      4% used

            Current week (all models)
                                                              20% used

            Extra usage
            Extra usage not enabled
            "#
        ));
    }

    #[test]
    fn usage_command_ready_accepts_weekly_only_usage_panel() {
        assert!(usage_command_ready(
            r#"
            Current week (Opus)
                                                              100% used

            Current week (Sonnet only)
                                                               15% used
            "#
        ));
    }

    #[test]
    fn usage_command_ready_rejects_loading_panel() {
        assert!(!usage_command_ready(
            r#"
            Session
            Loading usage data...
            "#
        ));
    }

    #[test]
    fn usage_command_login_setup_prompt_is_a_blocking_state() {
        assert!(slash_command_blocked_by_login_setup(
            "/usage",
            r#"
            Claude Code can be used with your Claude subscription.

            Select login method:
            > Claude account with subscription
              Anthropic Console account
            "#
        ));
        assert!(slash_command_blocked_by_login_setup(
            "/usage",
            "Browser didn't open? Use the url below to sign in"
        ));
        assert!(!slash_command_blocked_by_login_setup(
            "/status",
            "Select login method:"
        ));
    }

    #[test]
    fn slash_command_ready_timer_resets_on_each_ready_chunk() {
        let first_ready = Instant::now();
        let later_ready = first_ready + Duration::from_millis(500);
        let mut ready_since = None;

        update_slash_command_ready_since(&mut ready_since, true, first_ready);
        assert_eq!(ready_since, Some(first_ready));

        update_slash_command_ready_since(&mut ready_since, true, later_ready);
        assert_eq!(ready_since, Some(later_ready));

        update_slash_command_ready_since(&mut ready_since, false, later_ready);
        assert_eq!(ready_since, None);
    }

    #[test]
    fn login_unavailable_output_is_not_success() {
        let error = finish_login_status(
            true,
            "/login isn't available in this environment without a terminal",
        )
        .unwrap_err();

        assert!(matches!(error, AppError::ClaudeLoginFailed(_)));
    }

    #[test]
    fn first_run_theme_prompt_is_auto_accepted() {
        assert!(should_accept_first_run_prompt(
            r#"
            Welcome to Claude Code
            Choose the theme for your terminal:
            > Dark mode
              Light mode
            "#
        ));
    }

    #[test]
    fn workspace_trust_prompt_is_auto_accepted() {
        assert!(should_accept_workspace_trust_prompt(
            r#"
            Accessing workspace:

            /home/user/.wovo/claude/cli-workspace

            Quick safety check: Is this a project you created or one you trust?

            > 1. Yes, I trust this folder
              2. No, exit
            "#
        ));
    }

    #[test]
    fn first_run_prompt_detection_ignores_login_output() {
        assert!(!should_accept_first_run_prompt(
            "Open the following URL to complete Claude login."
        ));
    }

    #[test]
    fn login_method_prompt_is_auto_selected() {
        assert!(should_select_default_login_method(
            r#"
            Select login method:
            > Claude account with subscription
              Anthropic Console account
            "#
        ));
    }

    #[test]
    fn login_method_prompt_detection_ignores_login_url() {
        assert!(!should_select_default_login_method(
            "Open the following URL to complete Claude login."
        ));
    }

    #[test]
    fn login_prompts_accept_first_run_prompt_before_login_method() {
        let mut state = ClaudeLoginPromptState::default();
        let mut written = Vec::new();

        handle_login_prompt(
            r#"
            Welcome to Claude Code
            Choose the theme for your terminal:
            > Dark mode
              Light mode
            "#,
            &mut state,
            &mut written,
        );
        handle_login_prompt(
            r#"
            Select login method:
            > Claude account with subscription
              Anthropic Console account
            "#,
            &mut state,
            &mut written,
        );

        assert_eq!(written, b"\r\r");
        assert!(state.accepted_first_run_prompt);
        assert!(state.selected_default_login_method);
    }

    #[test]
    fn login_prompts_accept_workspace_trust_before_login_method() {
        let mut state = ClaudeLoginPromptState::default();
        let mut written = Vec::new();

        handle_login_prompt(
            r#"
            Quick safety check: Is this a project you created or one you trust?
            > 1. Yes, I trust this folder
              2. No, exit
            "#,
            &mut state,
            &mut written,
        );
        handle_login_prompt(
            r#"
            Select login method:
            > Claude account with subscription
              Anthropic Console account
            "#,
            &mut state,
            &mut written,
        );

        assert_eq!(written, b"\r\r");
        assert!(state.accepted_workspace_trust_prompt);
        assert!(state.selected_default_login_method);
    }

    #[test]
    fn slash_command_prompts_accept_first_run_prompt_before_usage_source() {
        let mut state = SlashCommandPromptState::default();
        let mut written = Vec::new();

        handle_slash_command_prompt(
            r#"
            Welcome to Claude Code
            Choose the theme for your terminal:
            > Dark mode
              Light mode
            "#,
            &mut state,
            &mut written,
        );
        handle_slash_command_prompt(
            r#"
            How do you want to view usage?
            › 1. Claude account with subscription - Pro, Max, Team, or Enterprise
              2. API usage billing
              3. Third-party platform
            "#,
            &mut state,
            &mut written,
        );

        assert_eq!(written, b"\r\r");
        assert!(state.accepted_first_run_prompt);
        assert!(state.selected_default_usage_source);
    }

    #[test]
    fn slash_command_prompts_accept_workspace_trust_before_usage_source() {
        let mut state = SlashCommandPromptState::default();
        let mut written = Vec::new();

        handle_slash_command_prompt(
            r#"
            Accessing workspace:
            /home/user/.wovo/claude/cli-workspace
            Quick safety check: Is this a project you created or one you trust?
            > 1. Yes, I trust this folder
              2. No, exit
            "#,
            &mut state,
            &mut written,
        );
        handle_slash_command_prompt(
            r#"
            How do you want to view usage?
            > 1. Claude account with subscription - Pro, Max, Team, or Enterprise
              2. API usage billing
              3. Third-party platform
            "#,
            &mut state,
            &mut written,
        );

        assert_eq!(written, b"\r\r");
        assert!(state.accepted_workspace_trust_prompt);
        assert!(state.selected_default_usage_source);
    }
}
