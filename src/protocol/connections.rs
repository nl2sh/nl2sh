//! Read-only connection discovery shared by the CLI, TUI and Web.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::OnceLock,
};

const DEFAULT_ORIGIN: &str = "http://127.0.0.1:8765";
const RECORD_NAME: &str = "connection.json";
static WELCOME: OnceLock<ConnectionInfo> = OnceLock::new();

/// Connection methods contain no token values and do not start a protocol server.
#[derive(Clone, Debug, Serialize)]
pub struct ConnectionInfo {
    /// Absolute configuration path for a local MCP client to reuse.
    pub config_path: String,
    /// `running`, `stopped`, or `unknown`; running does not assert remote reachability.
    pub state: String,
    /// Active transport, if discovered: `http` or `stdio`.
    pub transport: Option<String>,
    /// Actual advertised MCP HTTP endpoint, only for an active HTTP process.
    pub mcp_url: Option<String>,
    /// Actual advertised A2A JSON-RPC endpoint, only for an active HTTP process.
    pub a2a_url: Option<String>,
    /// Actual public Agent Card endpoint, only for an active HTTP process.
    pub agent_card_url: Option<String>,
    /// Local loopback example; actual advertisement is determined at server startup.
    pub default_http_origin: &'static str,
    /// Environment variable name, never its value.
    pub token_env: &'static str,
    /// Device command to start the default network HTTP server with automatic advertisement.
    pub http_command: String,
    /// Optional manual advertised-address override for VPN or multiple interfaces.
    pub network_command: String,
    /// Local MCP process command; the client must execute it on the same device.
    pub stdio_command: String,
    /// Local terminal command to list pending approvals.
    pub approvals_command: String,
}

impl ConnectionInfo {
    fn new(path: &Path) -> Self {
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
        let path = absolute
            .parent()
            .and_then(|parent| fs::canonicalize(parent).ok())
            .zip(absolute.file_name())
            .map(|(parent, name)| parent.join(name))
            .unwrap_or(absolute);
        let base = format!(
            "nl2sh --config '{}' protocol",
            path.to_string_lossy().replace('\'', "'\\''")
        );
        Self {
            config_path: path.to_string_lossy().into_owned(),
            state: "stopped".into(),
            transport: None,
            mcp_url: None,
            a2a_url: None,
            agent_card_url: None,
            default_http_origin: DEFAULT_ORIGIN,
            token_env: "NL2SH_PROTOCOL_TOKEN",
            http_command: format!("{base} serve"),
            network_command: format!("{base} serve --advertised-url http://DEVICE_IP:8765"),
            stdio_command: format!("{base} stdio"),
            approvals_command: format!("{base} approvals"),
        }
    }

    /// Connection guidance in the terminal's selected language.
    pub fn terminal_text(&self, language: crate::config::UiLanguage) -> String {
        let zh = language == crate::config::UiLanguage::ZhCn;
        let state = match (zh, self.state.as_str()) {
            (true, "running") => "运行中",
            (true, "stopped") => "未启动",
            (true, _) => "状态未知",
            (false, "running") => "running",
            (false, "stopped") => "stopped",
            (false, _) => "unknown",
        };
        let mut lines = vec![format!(
            "MCP / A2A: {state}{}",
            self.transport
                .as_deref()
                .map(|value| format!(" ({value})"))
                .unwrap_or_default()
        )];
        if let (Some(mcp), Some(a2a), Some(card)) =
            (&self.mcp_url, &self.a2a_url, &self.agent_card_url)
        {
            lines.extend([
                format!("MCP (Streamable HTTP): {mcp}"),
                format!("A2A (JSON-RPC 1.0): {a2a}"),
                format!("Agent Card: {card}"),
            ]);
        } else {
            lines.push(
                if zh {
                    "HTTP 尚无已确认的运行地址；以下仅本机示例，启动后显示自动获取的设备地址："
                } else {
                    "No confirmed HTTP endpoint; local examples only; startup reports the detected device address:"
                }
                .into(),
            );
            lines.extend([
                format!("MCP: {DEFAULT_ORIGIN}/mcp"),
                format!("A2A: {DEFAULT_ORIGIN}/a2a"),
                format!("Agent Card: {DEFAULT_ORIGIN}/.well-known/agent-card.json"),
            ]);
        }
        lines.push(if zh { "HTTP 鉴权：Authorization: Bearer <token>；未设 NL2SH_PROTOCOL_TOKEN 时启动自动生成并打印，设置该变量可复用固定令牌。" } else { "HTTP auth: Authorization: Bearer <token>; generated and printed at startup unless NL2SH_PROTOCOL_TOKEN is set." }.into());
        lines.push(format!(
            "{}{}",
            if zh {
                "设备另一个终端启动 HTTP："
            } else {
                "Start HTTP in another device terminal: "
            },
            self.http_command
        ));
        lines.push(format!(
            "{}{}",
            if zh {
                "可选：覆盖公告地址（多网卡/VPN，替换 DEVICE_IP）："
            } else {
                "Optional advertised-address override (multiple interfaces/VPN; replace DEVICE_IP): "
            },
            self.network_command
        ));
        lines.push(format!(
            "{}{}",
            if zh {
                "同设备本地 MCP stdio："
            } else {
                "Local MCP stdio on the same device: "
            },
            self.stdio_command
        ));
        lines.push(format!(
            "{}{}",
            if zh {
                "本地审批："
            } else {
                "Local approvals: "
            },
            self.approvals_command
        ));
        lines.push(if zh { "默认监听 0.0.0.0:8765 并允许 HTTP；自动获取设备 IPv4。仅本机使用可加 --host 127.0.0.1。HTTP 明文传输令牌，远程推荐 HTTPS。TUI/Web 不自动启动协议服务。" } else { "Defaults: 0.0.0.0:8765, HTTP allowed, device IPv4 detected automatically. Use --host 127.0.0.1 for local only. HTTP sends tokens in plaintext; prefer HTTPS remotely. TUI/Web does not start protocols." }.into());
        lines.join("\n")
    }
}

/// Capture read-only protocol status for the TUI startup page.
pub fn set_welcome_connections(info: ConnectionInfo) {
    let _ = WELCOME.set(info);
}
pub(crate) fn welcome_connections() -> Option<&'static ConnectionInfo> {
    WELCOME.get()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    pid: u32,
    start_ticks: u64,
    config_path: String,
    transport: String,
    origin: Option<String>,
}

/// Lives inside the already exclusively locked protocol state directory.
pub(super) struct Registration {
    path: PathBuf,
}
impl Registration {
    pub fn publish(config: &Path, origin: Option<&str>) -> Result<Self> {
        let root = crate::config::state_dir(config)?.join("protocol");
        let pid = std::process::id();
        let record = Record {
            pid,
            start_ticks: crate::service::start_ticks(pid)?
                .context("cannot identify protocol process")?,
            config_path: ConnectionInfo::new(config).config_path,
            transport: if origin.is_some() { "http" } else { "stdio" }.into(),
            origin: origin.map(str::to_owned),
        };
        let path = root.join(RECORD_NAME);
        let mut file = tempfile::NamedTempFile::new_in(&root)?;
        serde_json::to_writer(&mut file, &record)?;
        file.flush()?;
        file.persist(&path).map_err(|error| error.error)?;
        Ok(Self { path })
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Discover a live protocol process without a network request, model or credential access.
pub async fn connection_info(path: &Path) -> ConnectionInfo {
    let fallback = ConnectionInfo::new(path);
    let path = path.to_owned();
    match tokio::task::spawn_blocking(move || read(&path)).await {
        Ok(info) => info,
        Err(_) => ConnectionInfo {
            state: "unknown".into(),
            ..fallback
        },
    }
}
fn read(path: &Path) -> ConnectionInfo {
    let mut info = ConnectionInfo::new(path);
    if let Err(error) = discover(path, &mut info) {
        if !error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            info.state = "unknown".into();
        }
    }
    info
}
fn private_file(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        bail!("unsafe protocol discovery file");
    }
    Ok(file)
}
fn discover(path: &Path, info: &mut ConnectionInfo) -> Result<()> {
    let root = crate::config::state_dir(path)?.join("protocol");
    let metadata = fs::symlink_metadata(&root)?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        bail!("unsafe protocol discovery directory");
    }
    let lock = private_file(&root.join("lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        // A stale announcement must never advertise a stopped listener as active.
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.kind() != std::io::ErrorKind::WouldBlock {
        return Err(error.into());
    }
    info.state = "unknown".into();
    let file = private_file(&root.join(RECORD_NAME))?;
    if file.metadata()?.len() > 16 * 1024 {
        bail!("oversized protocol announcement");
    }
    let mut bytes = Vec::new();
    file.take(16 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 {
        bail!("oversized protocol announcement");
    }
    let record: Record = serde_json::from_slice(&bytes)?;
    if record.config_path != info.config_path {
        bail!("protocol uses a different configuration");
    }
    if record.pid <= 1 || crate::service::start_ticks(record.pid)? != Some(record.start_ticks) {
        bail!("unverified protocol process");
    }
    if fs::metadata(format!("/proc/{}", record.pid))?.uid() != unsafe { libc::geteuid() } {
        bail!("protocol process belongs to another user");
    }
    match (record.transport.as_str(), record.origin) {
        ("http", Some(origin)) => {
            let url = url::Url::parse(&origin)?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || url.path() != "/"
            {
                bail!("invalid protocol announcement origin");
            }
            let origin = url.origin().ascii_serialization();
            info.mcp_url = Some(format!("{origin}/mcp"));
            info.a2a_url = Some(format!("{origin}/a2a"));
            info.agent_card_url = Some(format!("{origin}/.well-known/agent-card.json"));
        }
        ("stdio", None) => {}
        _ => bail!("invalid protocol announcement transport"),
    }
    info.state = "running".into();
    info.transport = Some(record.transport);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::store::Store;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn live_announcement_uses_actual_origin_and_stale_records_do_not_connect() -> Result<()> {
        let root = tempfile::tempdir()?;
        let config = root.path().join("quoted ' config.toml");
        let stopped = connection_info(&config).await;
        assert_eq!(stopped.state, "stopped");
        assert!(stopped.mcp_url.is_none());
        assert!(
            !root.path().join("protocol").exists(),
            "discovery created state"
        );
        assert!(stopped.http_command.contains("'\\''"));
        let store = Store::open(&config)?;
        let registration = Registration::publish(&config, Some("https://agent.example:9443"))?;
        let active = connection_info(&config).await;
        assert_eq!(active.state, "running");
        assert_eq!(
            active.mcp_url.as_deref(),
            Some("https://agent.example:9443/mcp")
        );
        assert_eq!(
            active.a2a_url.as_deref(),
            Some("https://agent.example:9443/a2a")
        );
        assert_eq!(
            connection_info(&root.path().join("other.toml")).await.state,
            "unknown"
        );
        assert_eq!(fs::metadata(&registration.path)?.mode() & 0o777, 0o600);
        assert!(active
            .terminal_text(crate::config::UiLanguage::ZhCn)
            .contains("9443/mcp"));
        assert!(active
            .terminal_text(crate::config::UiLanguage::En)
            .contains("Authorization: Bearer <token>"));
        // Process termination releases the lock, even if the private record remains.
        drop(store);
        let stale = connection_info(&config).await;
        assert_eq!(stale.state, "stopped");
        assert!(stale.mcp_url.is_none());
        drop(registration);
        assert!(!root.path().join("protocol/connection.json").exists());
        Ok(())
    }

    #[tokio::test]
    async fn stdio_and_unsafe_announcements_never_advertise_http() -> Result<()> {
        let root = tempfile::tempdir()?;
        let config = root.path().join("config.toml");
        let _store = Store::open(&config)?;
        let registration = Registration::publish(&config, None)?;
        let info = connection_info(&config).await;
        assert_eq!(info.state, "running");
        assert_eq!(info.transport.as_deref(), Some("stdio"));
        assert!(info.mcp_url.is_none());
        fs::set_permissions(&registration.path, fs::Permissions::from_mode(0o644))?;
        assert_eq!(connection_info(&config).await.state, "unknown");
        fs::set_permissions(&registration.path, fs::Permissions::from_mode(0o600))?;
        let mut record: Record = serde_json::from_slice(&fs::read(&registration.path)?)?;
        record.start_ticks += 1;
        fs::write(&registration.path, serde_json::to_vec(&record)?)?;
        let invalid = connection_info(&config).await;
        assert_eq!(invalid.state, "unknown");
        assert!(invalid.mcp_url.is_none());
        Ok(())
    }
}
