use super::{pipeline, pty, resolve_invocation, RootProbe, SystemRootProbe};
use crate::config::Config;
use anyhow::Result;
use async_trait::async_trait;
use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[derive(Clone)]
/// Fully resolved process invocation used by PTY and pipeline backends.
pub struct ExecutionRequest {
    /// Executable path.
    pub program: OsString,
    /// Argument vector; shell source remains a single argument after `-c`.
    pub args: Vec<OsString>,
    /// Timeout in seconds, or zero for no timeout.
    pub timeout_secs: u64,
    /// Selects the PTY backend when true.
    pub use_pty: bool,
    /// Enables bidirectional local-terminal bridging.
    pub interactive: bool,
    /// Receives safe incremental output.
    pub output: Arc<dyn OutputSink>,
    /// Maximum bytes retained in each captured output stream.
    pub capture_max_bytes: usize,
    /// True when an interactive child must temporarily suspend a live TUI.
    pub tui_active: bool,
    /// Shared flag telling the TUI not to draw while a fullscreen child owns the terminal.
    pub tui_suspended: Option<Arc<AtomicBool>>,
}
#[derive(Debug, Clone)]
/// Captured outcome of a shell command.
pub struct ExecutionResult {
    /// Standard output, or merged PTY output.
    pub stdout: String,
    /// Standard error; empty for PTY execution.
    pub stderr: String,
    /// Exit code when representable as an integer.
    pub exit_code: Option<i32>,
    /// True when the configured timeout terminated the process group.
    pub timed_out: bool,
    /// True when Ctrl+C terminated the process group.
    pub interrupted: bool,
}
#[async_trait]
/// Security-agnostic command execution boundary used by the Agent.
pub trait CommandExecutor: Send + Sync {
    /// Returns a low-sensitivity runtime summary for model compatibility hints.
    /// The summary is advisory and must never affect security or confirmation.
    async fn runtime_context(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Executes an already assessed and approved command.
    async fn execute(
        &self,
        command: &str,
        needs_root: bool,
        interactive: bool,
    ) -> Result<ExecutionResult>;
}
/// Receives incremental command output without coupling shell code to a UI.
pub trait OutputSink: Send + Sync {
    /// Receives stdout or merged PTY text.
    fn stdout(&self, text: &str);
    /// Receives pipeline stderr text.
    fn stderr(&self, text: &str);
}
/// Output sink that discards chunks while results remain captured.
pub struct NullOutput;
impl OutputSink for NullOutput {
    fn stdout(&self, _: &str) {}
    fn stderr(&self, _: &str) {}
}
/// Output sink that streams chunks to the process console.
pub struct ConsoleOutput;
impl OutputSink for ConsoleOutput {
    fn stdout(&self, text: &str) {
        print!("{text}");
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
    fn stderr(&self, text: &str) {
        eprint!("{text}");
        let _ = std::io::Write::flush(&mut std::io::stderr());
    }
}
/// Android-aware PTY/pipeline executor with injectable root probe and output.
pub struct ShellExecutor {
    config: Config,
    probe: Box<dyn RootProbe>,
    output: Arc<dyn OutputSink>,
    tui_active: bool,
    tui_suspended: Option<Arc<AtomicBool>>,
}
impl ShellExecutor {
    /// Creates an executor with system root detection and no live output sink.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            probe: Box::new(SystemRootProbe),
            output: Arc::new(NullOutput),
            tui_active: false,
            tui_suspended: None,
        }
    }
    /// Creates an executor with a mockable root probe.
    pub fn with_probe(config: Config, probe: Box<dyn RootProbe>) -> Self {
        Self {
            config,
            probe,
            output: Arc::new(NullOutput),
            tui_active: false,
            tui_suspended: None,
        }
    }
    /// Replaces the incremental output destination.
    pub fn with_output(mut self, output: Arc<dyn OutputSink>) -> Self {
        self.output = output;
        self
    }

    /// Marks that interactive execution must leave and later restore the TUI screen.
    pub fn with_tui_active(mut self, active: bool) -> Self {
        self.tui_active = active;
        self
    }

    /// Installs a shared fullscreen-suspension flag for a live TUI.
    pub fn with_tui_suspend_flag(mut self, flag: Arc<AtomicBool>) -> Self {
        self.tui_suspended = Some(flag);
        self
    }

    /// Opens a direct user-controlled interactive shell through a PTY.
    ///
    /// This bypasses Agent classification because its input comes directly
    /// from the user, while retaining terminal suspension and child cleanup.
    pub async fn execute_user_shell(&self, command: &str) -> Result<ExecutionResult> {
        self.execute_resolved(command, false, true, true).await
    }

    async fn execute_resolved(
        &self,
        command: &str,
        needs_root: bool,
        interactive: bool,
        force_pty: bool,
    ) -> Result<ExecutionResult> {
        let (program, args) = resolve_invocation(
            command,
            self.config.execute_user_mode,
            needs_root,
            self.probe.as_ref(),
        )?;
        let timeout = if interactive {
            self.config.interactive_execute_timeout_secs
        } else {
            self.config.execute_timeout_secs
        };
        let req = ExecutionRequest {
            program,
            args,
            timeout_secs: timeout,
            use_pty: force_pty || self.config.enable_pty,
            interactive,
            output: self.output.clone(),
            capture_max_bytes: self.config.tool_output_max_bytes,
            tui_active: self.tui_active,
            tui_suspended: self.tui_suspended.clone(),
        };
        let _activity = TuiActivityGuard::new(req.interactive, req.tui_suspended.clone());
        if req.use_pty {
            pty::execute(req).await
        } else {
            pipeline::execute(req).await
        }
    }
}
#[async_trait]
impl CommandExecutor for ShellExecutor {
    async fn runtime_context(&self) -> Result<Option<String>> {
        #[cfg(target_os = "android")]
        {
            let uid = self.probe.uid();
            let su_available = self.probe.su_available();
            let api_level = android_api_level().await;
            let mut fields = vec![
                "platform=Android".to_owned(),
                format!("abi={}", std::env::consts::ARCH),
                "shell=/system/bin/sh".to_owned(),
                format!("uid={uid}"),
                format!("root={}", uid == 0),
                format!("su_available={su_available}"),
            ];
            if let Some(api_level) = api_level {
                fields.insert(1, format!("api={api_level}"));
            }
            Ok(Some(fields.join(" ")))
        }
        #[cfg(not(target_os = "android"))]
        {
            Ok(None)
        }
    }

    async fn execute(
        &self,
        command: &str,
        needs_root: bool,
        interactive: bool,
    ) -> Result<ExecutionResult> {
        self.execute_resolved(command, needs_root, interactive, false)
            .await
    }
}

#[cfg(target_os = "android")]
async fn android_api_level() -> Option<String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio::process::Command::new("/system/bin/getprop")
            .arg("ro.build.version.sdk")
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim();
    (!value.is_empty() && value.chars().all(|character| character.is_ascii_digit()))
        .then(|| value.to_owned())
}

struct TuiActivityGuard(Option<Arc<AtomicBool>>);
impl TuiActivityGuard {
    fn new(interactive: bool, flag: Option<Arc<AtomicBool>>) -> Self {
        if interactive {
            if let Some(value) = &flag {
                value.store(true, Ordering::Release);
            }
            Self(flag)
        } else {
            Self(None)
        }
    }
}
impl Drop for TuiActivityGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.0 {
            value.store(false, Ordering::Release);
        }
    }
}
