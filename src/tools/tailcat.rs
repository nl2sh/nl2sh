//! Opt-in Tailcat operations with argv-only execution and managed listeners.

use super::{
    definition, parse_args, PreparedExecution, PreparedToolCall, Tool, ToolCategory, ToolContext,
    ToolMetadata, ToolOutput, ToolRisk,
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
#[cfg(any(target_os = "linux", target_os = "android"))]
use std::os::unix::process::CommandExt;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::OnceLock,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader},
    process::{Child, Command},
    sync::{mpsc, Mutex},
    time::timeout,
};

static META: &[ToolMetadata] = &[
    ToolMetadata { name: "tailcat_check", description: "Check the configured Tailcat executable and version.", category: ToolCategory::Network, risk: ToolRisk::ReadOnly, requires: &[], parallel_safe: true },
    ToolMetadata { name: "tailcat_receive", description: "Start a managed Tailcat file drop box in an existing directory and return its address. Incoming peers can write files there.", category: ToolCategory::Network, risk: ToolRisk::Mutating, requires: &[], parallel_safe: false },
    ToolMetadata { name: "tailcat_receive_stream", description: "Start a managed raw Tailcat receiver, saving one incoming byte stream to a new file. Return its address.", category: ToolCategory::Network, risk: ToolRisk::Mutating, requires: &[], parallel_safe: false },
    ToolMetadata { name: "tailcat_send_file", description: "Send an existing file to a Tailcat address. Use mode=stream for a raw receiver or mode=copy for a file drop box (requires scp).", category: ToolCategory::Network, risk: ToolRisk::Dangerous, requires: &[], parallel_safe: false },
    ToolMetadata { name: "tailcat_serve", description: "Expose one local TCP port through a managed Tailcat listener and return its address.", category: ToolCategory::Network, risk: ToolRisk::Dangerous, requires: &[], parallel_safe: false },
    ToolMetadata { name: "tailcat_status", description: "Inspect this nl2sh process's managed Tailcat listener.", category: ToolCategory::Network, risk: ToolRisk::ReadOnly, requires: &[], parallel_safe: true },
    ToolMetadata { name: "tailcat_stop", description: "Stop this nl2sh process's managed Tailcat listener.", category: ToolCategory::Network, risk: ToolRisk::Mutating, requires: &[], parallel_safe: false },
];

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Receive {
    directory: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReceiveStream {
    path: String,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum SendMode {
    Stream,
    Copy,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SendFile {
    path: String,
    address: String,
    mode: SendMode,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Serve {
    port: u16,
}

struct TailcatTool(&'static ToolMetadata);

pub(super) fn builtin_tools() -> Vec<Box<dyn Tool>> {
    META.iter()
        .map(|meta| Box::new(TailcatTool(meta)) as Box<dyn Tool>)
        .collect()
}

#[async_trait]
impl Tool for TailcatTool {
    fn metadata(&self) -> &'static ToolMetadata {
        self.0
    }

    fn definition(&self) -> crate::llm::ToolDefinition {
        let name = self.0.name;
        let description = self.0.description;
        match name {
            "tailcat_receive" => definition::<Receive>(name, description),
            "tailcat_receive_stream" => definition::<ReceiveStream>(name, description),
            "tailcat_send_file" => definition::<SendFile>(name, description),
            "tailcat_serve" => definition::<Serve>(name, description),
            _ => definition::<Empty>(name, description),
        }
    }

    async fn prepare(&self, ctx: &ToolContext<'_>, arguments: Value) -> Result<PreparedToolCall> {
        let binary = ctx
            .config
            .context("Tailcat configuration unavailable")?
            .tailcat_binary_path
            .clone();
        let action = match self.0.name {
            "tailcat_check" => {
                let _: Empty = parse_args(self.0.name, arguments)?;
                Action::Check
            }
            "tailcat_receive" => {
                let args: Receive = parse_args(self.0.name, arguments)?;
                let path = existing_path(&args.directory, true)?;
                Action::Receive(path)
            }
            "tailcat_receive_stream" => {
                let args: ReceiveStream = parse_args(self.0.name, arguments)?;
                let path = new_file_path(&args.path)?;
                Action::ReceiveStream(path)
            }
            "tailcat_send_file" => {
                let args: SendFile = parse_args(self.0.name, arguments)?;
                if !valid_address(&args.address) {
                    bail!("invalid Tailcat address")
                }
                Action::SendFile(existing_path(&args.path, false)?, args.address, args.mode)
            }
            "tailcat_serve" => {
                let args: Serve = parse_args(self.0.name, arguments)?;
                if args.port == 0 {
                    bail!("Tailcat port must be 1–65535")
                }
                Action::Serve(args.port)
            }
            "tailcat_status" => {
                let _: Empty = parse_args(self.0.name, arguments)?;
                Action::Status
            }
            "tailcat_stop" => {
                let _: Empty = parse_args(self.0.name, arguments)?;
                Action::Stop
            }
            _ => bail!("unsupported Tailcat action"),
        };
        let preview = match &action {
            Action::Receive(dir) => format!(
                "Start Tailcat file drop box in {}. Peers with its address can write files.",
                dir.display()
            ),
            Action::ReceiveStream(path) => format!(
                "Receive one raw Tailcat stream into new file {}",
                path.display()
            ),
            Action::SendFile(path, address, mode) => format!(
                "Send local file {} to Tailcat address {} using {} mode",
                path.display(),
                address,
                match mode {
                    SendMode::Stream => "raw stream",
                    SendMode::Copy => "file copy",
                }
            ),
            Action::Serve(port) => {
                format!("Expose localhost TCP port {port} to peers with the Tailcat address")
            }
            Action::Stop => "Stop managed Tailcat listener".into(),
            _ => String::new(),
        };
        Ok(PreparedToolCall::operation(
            preview,
            Box::new(TailcatOperation { binary, action }),
        ))
    }
}

fn existing_path(value: &str, directory: bool) -> Result<PathBuf> {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        bail!("invalid Tailcat path")
    }
    let path = Path::new(value)
        .canonicalize()
        .with_context(|| format!("cannot access {value}"))?;
    let meta = std::fs::metadata(&path)?;
    if (directory && !meta.is_dir()) || (!directory && !meta.is_file()) {
        bail!("Tailcat path has the wrong file type")
    }
    Ok(path)
}

fn new_file_path(value: &str) -> Result<PathBuf> {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        bail!("invalid Tailcat output path")
    }
    let input = Path::new(value);
    let parent = input
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .context("Tailcat output needs a parent directory")?;
    let parent = existing_path(parent.to_str().context("non-UTF8 Tailcat directory")?, true)?;
    let name = input
        .file_name()
        .context("Tailcat output needs a file name")?;
    let output = parent.join(name);
    if output.exists() {
        bail!("Tailcat output already exists")
    }
    Ok(output)
}

fn valid_address(address: &str) -> bool {
    address.starts_with("tc")
        && (16..=512).contains(&address.len())
        && address
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

enum Action {
    Check,
    Receive(PathBuf),
    ReceiveStream(PathBuf),
    SendFile(PathBuf, String, SendMode),
    Serve(u16),
    Status,
    Stop,
}
struct TailcatOperation {
    binary: PathBuf,
    action: Action,
}

struct Listener {
    child: Child,
    address: String,
    description: String,
}
static LISTENER: OnceLock<Mutex<Option<Listener>>> = OnceLock::new();
fn listener() -> &'static Mutex<Option<Listener>> {
    LISTENER.get_or_init(|| Mutex::new(None))
}

#[async_trait]
impl PreparedExecution for TailcatOperation {
    async fn execute(self: Box<Self>, _: &mut ToolContext<'_>) -> Result<ToolOutput> {
        let content = match self.action {
            Action::Check => {
                let output = timeout(
                    Duration::from_secs(8),
                    Command::new(&self.binary).arg("version").output(),
                )
                .await
                .context("Tailcat version timed out")??;
                if !output.status.success() {
                    bail!(
                        "Tailcat version failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    )
                }
                format!(
                    "binary={}\n{}",
                    self.binary.display(),
                    String::from_utf8_lossy(&output.stdout).trim()
                )
            }
            Action::Receive(directory) => {
                start(
                    &self.binary,
                    vec!["recv".into(), directory.display().to_string()],
                    format!("receive files in {}", directory.display()),
                    None,
                )
                .await?
            }
            Action::ReceiveStream(path) => {
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .context("cannot create Tailcat output file")?;
                match start(
                    &self.binary,
                    Vec::new(),
                    format!("receive raw stream in {}", path.display()),
                    Some(file),
                )
                .await
                {
                    Ok(content) => content,
                    Err(error) => {
                        let _ = std::fs::remove_file(&path);
                        return Err(error);
                    }
                }
            }
            Action::Serve(port) => {
                start(
                    &self.binary,
                    vec!["serve".into(), port.to_string()],
                    format!("serve localhost TCP {port}"),
                    None,
                )
                .await?
            }
            Action::SendFile(path, address, mode) => {
                let mut command = Command::new(&self.binary);
                match mode {
                    SendMode::Copy => {
                        command.arg("cp").arg(&path).arg(format!("{address}:"));
                    }
                    SendMode::Stream => {
                        command.arg(&address).stdin(
                            std::fs::File::open(&path).context("cannot open Tailcat input file")?,
                        );
                    }
                }
                let mut child = command
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .kill_on_drop(true)
                    .spawn()
                    .context("cannot start Tailcat file sender")?;
                let stderr = child
                    .stderr
                    .take()
                    .context("Tailcat sender stderr unavailable")?;
                let diagnostics = tokio::spawn(async move {
                    let mut pipe = stderr;
                    let mut saved = Vec::new();
                    let mut chunk = [0_u8; 4096];
                    loop {
                        let count = pipe.read(&mut chunk).await?;
                        if count == 0 {
                            break;
                        }
                        let room = 4096_usize.saturating_sub(saved.len());
                        saved.extend_from_slice(&chunk[..count.min(room)]);
                    }
                    Ok::<Vec<u8>, std::io::Error>(saved)
                });
                let status = match timeout(Duration::from_secs(600), child.wait()).await {
                    Ok(result) => result.context("cannot wait for Tailcat sender")?,
                    Err(_) => {
                        let _ = child.start_kill();
                        let _ = child.wait().await;
                        let _ = diagnostics.await;
                        bail!("Tailcat file send timed out")
                    }
                };
                let diagnostic = diagnostics
                    .await
                    .context("Tailcat diagnostics task failed")??;
                if !status.success() {
                    bail!(
                        "Tailcat file send failed: {}",
                        String::from_utf8_lossy(&diagnostic)
                    )
                }
                format!(
                    "sent {} bytes from {} to {}",
                    std::fs::metadata(&path)?.len(),
                    path.display(),
                    address
                )
            }
            Action::Status => {
                let mut guard = listener().lock().await;
                if let Some(session) = guard.as_mut() {
                    if session.child.try_wait()?.is_none() {
                        format!(
                            "running: {}\naddress={}",
                            session.description, session.address
                        )
                    } else {
                        *guard = None;
                        "no managed Tailcat listener".into()
                    }
                } else {
                    "no managed Tailcat listener".into()
                }
            }
            Action::Stop => {
                let mut guard = listener().lock().await;
                if let Some(mut session) = guard.take() {
                    if session.child.try_wait()?.is_none() {
                        session
                            .child
                            .start_kill()
                            .context("cannot stop Tailcat listener")?;
                        session
                            .child
                            .wait()
                            .await
                            .context("cannot reap Tailcat listener")?;
                    }
                    format!("stopped Tailcat listener {}", session.address)
                } else {
                    "no managed Tailcat listener".into()
                }
            }
        };
        Ok(ToolOutput::success(content))
    }
}

async fn start(
    binary: &Path,
    args: Vec<String>,
    description: String,
    output: Option<std::fs::File>,
) -> Result<String> {
    let mut guard = listener().lock().await;
    if let Some(session) = guard.as_mut() {
        if session.child.try_wait()?.is_none() {
            bail!(
                "a Tailcat listener is already running at {}; stop it first",
                session.address
            )
        }
        *guard = None;
    }
    let mut command = Command::new(binary);
    command
        .arg("--key=new")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(output.map(Stdio::from).unwrap_or_else(Stdio::piped))
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let parent = std::process::id();
        // SAFETY: The pre-exec closure only calls async-signal-safe libc functions and
        // captures a plain integer. It leaves no Rust allocation or lock in the child.
        unsafe {
            command.as_std_mut().pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() as u32 != parent {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "Tailcat parent exited",
                    ));
                }
                Ok(())
            });
        }
    }
    let mut child = command.spawn().context("cannot start Tailcat listener")?;
    let (tx, mut rx) = mpsc::channel::<String>(32);
    if let Some(pipe) = child.stdout.take() {
        collect_lines(pipe, tx.clone());
    }
    if let Some(pipe) = child.stderr.take() {
        collect_lines(pipe, tx.clone());
    }
    drop(tx);
    let address = timeout(Duration::from_secs(15), async {
        while let Some(line) = rx.recv().await {
            if let Some(address) = line
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                .find(|part| valid_address(part))
            {
                return Some(address.to_owned());
            }
        }
        None
    })
    .await
    .ok()
    .flatten();
    if let Some(address) = address {
        if child.try_wait()?.is_none() {
            *guard = Some(Listener {
                child,
                address: address.clone(),
                description,
            });
            return Ok(format!("Tailcat listener running\naddress={address}"));
        }
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
    bail!("Tailcat did not start a listener or print an address within 15 seconds")
}

fn collect_lines(pipe: impl AsyncRead + Unpin + Send + 'static, sender: mpsc::Sender<String>) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(pipe).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = sender.try_send(line);
        }
    });
}
