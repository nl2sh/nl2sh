//! Opt-in Tailcat operations with argv-only execution and managed listeners.

use super::{
    definition, parse_args, PreparedExecution, PreparedToolCall, Tool, ToolCategory, ToolContext,
    ToolMetadata, ToolOutput, ToolRisk,
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use flate2::read::GzDecoder;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
#[cfg(any(target_os = "linux", target_os = "android"))]
use std::os::unix::process::CommandExt;
use std::{
    io::{Read, Write},
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
    ToolMetadata { name: "tailcat_install", description: "Install the pinned official Tailcat release for this device ABI after checksum verification. Replaces the configured executable.", category: ToolCategory::Network, risk: ToolRisk::Mutating, requires: &[], parallel_safe: false },
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
            "tailcat_install" => {
                let _: Empty = parse_args(self.0.name, arguments)?;
                let target = install_target(&binary)?;
                let asset = device_asset().await?;
                let config = ctx.config.context("Tailcat configuration unavailable")?;
                Action::Install {
                    target,
                    asset,
                    client: crate::network::build_http_client(config)?,
                }
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
            Action::Install { target, asset, .. } => format!(
                "Download official Tailcat {TAILCAT_VERSION} for {} from {}, verify pinned SHA-256 {}, and replace {}",
                asset.abi,
                asset.url(),
                asset.sha256,
                target.display()
            ),
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

const TAILCAT_VERSION: &str = "v0.7.0";
const MAX_ARCHIVE_BYTES: usize = 16 * 1024 * 1024;
const MAX_BINARY_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy)]
struct ReleaseAsset {
    abi: &'static str,
    arch: &'static str,
    sha256: &'static str,
    elf_class: u8,
    elf_machine: u16,
}

impl ReleaseAsset {
    fn url(self) -> String {
        format!(
            "https://github.com/tailscale/tailcat/releases/download/{TAILCAT_VERSION}/tailcat_0.7.0_linux_{}.tar.gz",
            self.arch
        )
    }
}

const ARM64_ASSET: ReleaseAsset = ReleaseAsset {
    abi: "arm64-v8a",
    arch: "arm64",
    sha256: "bbb1ab50f24f00effe1e1fd86d0501803fb80793a90785a2a16ff3428f03d8ef",
    elf_class: 2,
    elf_machine: 183,
};
const ARMV7_ASSET: ReleaseAsset = ReleaseAsset {
    abi: "armeabi-v7a",
    arch: "armv7",
    sha256: "cad3994b1f336b67e8a3a5273a9cecb44331d14d57eb32bfe7d0919adeab22b7",
    elf_class: 1,
    elf_machine: 40,
};
#[cfg(not(target_os = "android"))]
const AMD64_ASSET: ReleaseAsset = ReleaseAsset {
    abi: "x86_64",
    arch: "amd64",
    sha256: "23c0b1887a5ec422f0d18a9c52b4f5357815febdaae738a1eb54036d10bd9ee6",
    elf_class: 2,
    elf_machine: 62,
};

#[cfg(any(target_os = "android", test))]
fn asset_for_android_abis(abis: &str) -> Result<ReleaseAsset> {
    for abi in abis.split(',').map(str::trim) {
        match abi {
            "arm64-v8a" => return Ok(ARM64_ASSET),
            "armeabi-v7a" => return Ok(ARMV7_ASSET),
            _ => {}
        }
    }
    bail!("no supported Tailcat release for Android ABIs: {abis}")
}

async fn device_asset() -> Result<ReleaseAsset> {
    #[cfg(target_os = "android")]
    {
        for property in ["ro.product.cpu.abilist", "ro.product.cpu.abi"] {
            let output = timeout(
                Duration::from_secs(5),
                Command::new("/system/bin/getprop").arg(property).output(),
            )
            .await
            .context("Android ABI probe timed out")??;
            if output.status.success() {
                let abis = String::from_utf8(output.stdout).context("Android ABI is not UTF-8")?;
                if !abis.trim().is_empty() {
                    return asset_for_android_abis(abis.trim());
                }
            }
        }
        bail!("cannot determine Android ABI for Tailcat installation")
    }
    #[cfg(target_os = "linux")]
    {
        match std::env::consts::ARCH {
            "x86_64" => Ok(AMD64_ASSET),
            "aarch64" => Ok(ARM64_ASSET),
            "arm" => Ok(ARMV7_ASSET),
            other => bail!("no supported Tailcat release for architecture {other}"),
        }
    }
    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    {
        bail!("Tailcat automatic installation is unsupported on this platform")
    }
}

fn install_target(configured: &Path) -> Result<PathBuf> {
    if !configured.is_absolute() {
        bail!("tailcat_binary_path must be absolute for automatic installation")
    }
    let parent = configured
        .parent()
        .context("Tailcat installation path has no parent")?
        .canonicalize()
        .context("Tailcat installation directory does not exist")?;
    let name = configured
        .file_name()
        .context("Tailcat installation path has no file name")?;
    let target = parent.join(name);
    match std::fs::symlink_metadata(&target) {
        Ok(metadata) if !metadata.is_file() => {
            bail!("Tailcat installation target must be a regular file")
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("cannot inspect Tailcat installation target"),
    }
    Ok(target)
}

async fn download_release(client: &reqwest::Client, asset: ReleaseAsset) -> Result<Vec<u8>> {
    let mut response = client
        .get(asset.url())
        .header("User-Agent", concat!("nl2sh/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .context("Tailcat download failed")?
        .error_for_status()
        .context("Tailcat release asset returned an error")?;
    if response
        .content_length()
        .is_some_and(|len| len > MAX_ARCHIVE_BYTES as u64)
    {
        bail!("Tailcat release archive is too large")
    }
    let mut archive = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("cannot read Tailcat release")?
    {
        if archive.len().saturating_add(chunk.len()) > MAX_ARCHIVE_BYTES {
            bail!("Tailcat release archive is too large")
        }
        archive.extend_from_slice(&chunk);
    }
    Ok(archive)
}

fn extract_verified_binary(archive: &[u8], asset: ReleaseAsset) -> Result<Vec<u8>> {
    verify_checksum(archive, asset.sha256)?;
    extract_binary(archive, asset)
}

fn verify_checksum(archive: &[u8], expected: &str) -> Result<()> {
    let actual = format!("{:x}", Sha256::digest(archive));
    if actual != expected {
        bail!("Tailcat release checksum mismatch")
    }
    Ok(())
}

fn extract_binary(archive: &[u8], asset: ReleaseAsset) -> Result<Vec<u8>> {
    let mut tar = tar::Archive::new(GzDecoder::new(archive));
    for entry in tar
        .entries()
        .context("cannot read Tailcat release archive")?
    {
        let mut entry = entry.context("invalid Tailcat archive entry")?;
        if entry.path()?.as_ref() != Path::new("tailcat") {
            continue;
        }
        if !entry.header().entry_type().is_file() || entry.header().size()? > MAX_BINARY_BYTES {
            bail!("Tailcat archive contains an invalid executable")
        }
        let mut binary = Vec::new();
        entry
            .read_to_end(&mut binary)
            .context("cannot read Tailcat executable")?;
        verify_elf(&binary, asset)?;
        return Ok(binary);
    }
    bail!("Tailcat release archive does not contain tailcat")
}

fn verify_elf(binary: &[u8], asset: ReleaseAsset) -> Result<()> {
    if binary.len() < 20
        || &binary[..4] != b"\x7fELF"
        || binary[4] != asset.elf_class
        || binary[5] != 1
        || u16::from_le_bytes([binary[18], binary[19]]) != asset.elf_machine
    {
        bail!(
            "Tailcat executable architecture does not match {}",
            asset.abi
        )
    }
    Ok(())
}

fn stage_binary(
    archive: Vec<u8>,
    asset: ReleaseAsset,
    target: &Path,
) -> Result<tempfile::TempPath> {
    let binary = extract_verified_binary(&archive, asset)?;
    let parent = target
        .parent()
        .context("Tailcat installation path has no parent")?;
    let mut staged = tempfile::Builder::new()
        .prefix(".tailcat-install-")
        .tempfile_in(parent)
        .context("cannot create temporary Tailcat executable")?;
    staged
        .write_all(&binary)
        .context("cannot write temporary Tailcat executable")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        staged
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o700))
            .context("cannot make temporary Tailcat executable runnable")?;
    }
    staged
        .as_file()
        .sync_all()
        .context("cannot sync temporary Tailcat executable")?;
    Ok(staged.into_temp_path())
}

async fn install_release(
    client: &reqwest::Client,
    asset: ReleaseAsset,
    target: &Path,
) -> Result<String> {
    let archive = download_release(client, asset).await?;
    let target = target.to_path_buf();
    let stage_target = target.clone();
    let staged = tokio::task::spawn_blocking(move || stage_binary(archive, asset, &stage_target))
        .await
        .context("Tailcat installation worker failed")??;
    let staged_path: &Path = staged.as_ref();
    let output = timeout(
        Duration::from_secs(8),
        Command::new(staged_path).arg("version").output(),
    )
    .await
    .context("downloaded Tailcat version check timed out")??;
    if !output.status.success()
        || !String::from_utf8_lossy(&output.stdout).contains(TAILCAT_VERSION)
    {
        bail!("downloaded Tailcat did not report the expected version")
    }
    install_target(&target)?;
    staged
        .persist(&target)
        .map_err(|error| error.error)
        .context("cannot replace Tailcat executable")?;
    Ok(format!(
        "Installed Tailcat {TAILCAT_VERSION} for {} at {}",
        asset.abi,
        target.display()
    ))
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
    Install {
        target: PathBuf,
        asset: ReleaseAsset,
        client: reqwest::Client,
    },
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
            Action::Install {
                target,
                asset,
                client,
            } => install_release(&client, asset, &target).await?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::{ConfirmationDecision, Confirmer},
        config::Config,
        security::{RiskLevel, SecurityAssessment},
        shell::ShellExecutor,
        tools::runtime::invoke,
    };
    use anyhow::Result;
    use serde_json::json;

    struct RejectInstall;

    #[async_trait]
    impl Confirmer for RejectInstall {
        async fn confirm(
            &self,
            preview: &str,
            assessment: &SecurityAssessment,
        ) -> Result<ConfirmationDecision> {
            assert_eq!(assessment.risk_level, RiskLevel::Mutating);
            assert!(preview.contains(TAILCAT_VERSION));
            assert!(preview.contains("SHA-256"));
            Ok(ConfirmationDecision::Reject)
        }
    }

    fn sample_elf(asset: ReleaseAsset) -> Vec<u8> {
        let mut binary = vec![0_u8; 20];
        binary[..4].copy_from_slice(b"\x7fELF");
        binary[4] = asset.elf_class;
        binary[5] = 1;
        binary[18..20].copy_from_slice(&asset.elf_machine.to_le_bytes());
        binary
    }

    #[test]
    fn android_abi_selects_matching_official_archive() -> Result<()> {
        assert_eq!(
            asset_for_android_abis("arm64-v8a,armeabi-v7a")?.arch,
            "arm64"
        );
        assert_eq!(asset_for_android_abis("armeabi-v7a,armeabi")?.arch, "armv7");
        assert!(asset_for_android_abis("x86_64,x86").is_err());
        Ok(())
    }

    #[test]
    fn checksum_and_elf_checks_reject_wrong_artifacts() -> Result<()> {
        let binary = sample_elf(ARMV7_ASSET);
        verify_elf(&binary, ARMV7_ASSET)?;
        assert!(verify_elf(&binary, ARM64_ASSET).is_err());
        let digest = format!("{:x}", Sha256::digest(&binary));
        verify_checksum(&binary, &digest)?;
        assert!(verify_checksum(&binary, ARMV7_ASSET.sha256).is_err());
        Ok(())
    }

    #[test]
    fn verified_archive_extracts_only_the_expected_executable() -> Result<()> {
        let binary = sample_elf(ARMV7_ASSET);
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(binary.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append_data(&mut header, "tailcat", binary.as_slice())?;
        let tar = builder.into_inner()?;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&tar)?;
        let archive = encoder.finish()?;
        assert_eq!(extract_binary(&archive, ARMV7_ASSET)?, binary);
        assert!(extract_verified_binary(&archive, ARMV7_ASSET).is_err());
        Ok(())
    }

    #[test]
    fn installer_requires_absolute_regular_file_target() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("tailcat");
        assert_eq!(install_target(&target)?, target);
        assert!(install_target(Path::new("tailcat")).is_err());
        std::fs::create_dir(&target)?;
        assert!(install_target(&target).is_err());
        Ok(())
    }

    #[tokio::test]
    async fn rejected_install_preserves_existing_binary() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("tailcat");
        std::fs::write(&target, b"existing Tailcat")?;
        let mut config = Config {
            tailcat_binary_path: target.clone(),
            ..Config::default()
        };
        config.tool_overrides.insert("tailcat_install".into(), true);
        let executor = ShellExecutor::new(config.clone());
        let result = invoke(
            &config,
            &executor,
            &RejectInstall,
            "tailcat_install",
            json!({}),
        )
        .await?;
        assert!(!result.success);
        assert_eq!(std::fs::read(target)?, b"existing Tailcat");
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    #[ignore = "downloads the pinned official Tailcat release"]
    async fn live_installer_downloads_and_runs_pinned_release() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("tailcat");
        let asset = device_asset().await?;
        let mut config = crate::config::Config::default();
        if let Ok(proxy) = std::env::var("NL2SH_TAILCAT_TEST_PROXY") {
            config.proxy_enabled = true;
            config.proxy_address = proxy;
        }
        let client = crate::network::build_http_client(&config)?;
        let installed = install_release(&client, asset, &target).await?;
        assert!(installed.contains(TAILCAT_VERSION));
        let output = Command::new(&target).arg("version").output().await?;
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains(TAILCAT_VERSION));
        Ok(())
    }
}
