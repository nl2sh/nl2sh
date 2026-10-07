//! Native background service lifecycle. Local files and control sockets are private to the UID.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A local service controller operation. This interface never changes configuration.
pub enum Operation {
    /// Idempotently start a service and await its own HTTP readiness.
    Start { port: u16, strict: bool },
    /// Gracefully stop the verified owned service.
    Stop,
    /// Stop and start under one operation lock.
    Restart { port: u16, strict: bool },
    /// Inspect identity and HTTP readiness without signaling a process.
    Status,
}

/// Public status omits the private shutdown token and process identity internals.
#[derive(Debug, Serialize)]
pub struct Status {
    /// Lifecycle protocol version.
    pub protocol: u32,
    /// `ready`, `starting`, or `stopped`.
    pub state: &'static str,
    /// Verified owned process, when present.
    pub pid: Option<u32>,
    /// Version of the running executable, which can differ from the controller.
    pub version: Option<String>,
    /// Actual bound port, which may differ from the preferred port.
    pub port: Option<u16>,
    /// Unix startup timestamp.
    pub started_at: Option<u64>,
}

impl Status {
    /// Compact human-readable summary.
    pub fn summary(&self) -> String {
        format!(
            "{} pid={} port={} version={}",
            self.state,
            self.pid.map_or_else(|| "-".into(), |v| v.to_string()),
            self.port.map_or_else(|| "-".into(), |v| v.to_string()),
            self.version.as_deref().unwrap_or("-")
        )
    }
}

#[derive(Serialize, Deserialize)]
struct Record {
    protocol: u32,
    pid: u32,
    uid: u32,
    start_ticks: u64,
    exe_device: u64,
    exe_inode: u64,
    version: String,
    port: u16,
    started_at: u64,
    token: String,
    control_port: u16,
}

/// Controller bound to a configuration and its adjacent private runtime directory.
pub struct Service {
    config: PathBuf,
    dir: PathBuf,
}

impl Service {
    /// Resolve an absolute configuration path without requiring model setup or an existing file.
    pub fn new(config: &Path) -> Result<Self> {
        let absolute = if config.is_absolute() {
            config.to_owned()
        } else {
            std::env::current_dir()?.join(config)
        };
        let parent = absolute.parent().context("configuration has no parent")?;
        fs::create_dir_all(parent).context("cannot create configuration directory")?;
        let config = fs::canonicalize(parent)?
            .join(absolute.file_name().context("configuration has no name")?);
        let dir = config.with_extension("service");
        if !dir.exists() {
            match fs::DirBuilder::new().mode(0o700).create(&dir) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error).context("cannot create service directory"),
            }
        }
        let metadata = fs::symlink_metadata(&dir)?;
        if !metadata.is_dir()
            || metadata.uid() != nix::unistd::geteuid().as_raw()
            || metadata.permissions().mode() & 0o077 != 0
        {
            bail!(
                "service directory {} must be a real directory owned by current UID {} with private permissions 0700 (found UID {}, mode {:04o}); use the directory owner's UID to stop the service before switching users, then move the stopped service directory aside; do not chown active service state or relax its permissions",
                dir.display(),
                nix::unistd::geteuid().as_raw(),
                metadata.uid(),
                metadata.permissions().mode() & 0o7777,
            )
        }
        Ok(Self { config, dir })
    }

    /// Execute one lifecycle operation; mutations are serialized across controllers.
    pub async fn control(&self, operation: Operation) -> Result<Status> {
        if matches!(operation, Operation::Status) {
            return self.status().await;
        }
        let _operation = self.lock("operation.lock").await?;
        match operation {
            Operation::Start { port, strict } => self.start(port, strict).await,
            Operation::Stop => {
                self.stop().await?;
                self.status().await
            }
            Operation::Restart { port, strict } => {
                self.stop().await?;
                self.start(port, strict).await
            }
            Operation::Status => self.status().await,
        }
    }

    async fn lock(&self, name: &str) -> Result<nix::fcntl::Flock<File>> {
        let mut file = private_file(&self.dir.join(name), false)?;
        for _ in 0..200 {
            match nix::fcntl::Flock::lock(file, nix::fcntl::FlockArg::LockExclusiveNonblock) {
                Ok(lock) => return Ok(lock),
                Err((returned, nix::errno::Errno::EWOULDBLOCK)) => {
                    file = returned;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err((_, error)) => return Err(error).context("cannot lock service lifecycle"),
            }
        }
        bail!("service lifecycle is busy; retry after the active operation finishes")
    }

    fn record(&self) -> Result<Option<Record>> {
        let path = self.dir.join("state.json");
        let mut file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).context("cannot read private service state"),
        };
        let mut bytes = Vec::new();
        (&mut file).take(4097).read_to_end(&mut bytes)?;
        if bytes.len() > 4096 {
            bail!("service state exceeds size limit")
        }
        let record: Record = serde_json::from_slice(&bytes)
            .context("invalid service state; refusing to control an unknown process")?;
        if record.protocol != 1
            || record.uid != nix::unistd::geteuid().as_raw()
            || record.pid <= 1
            || record.token.len() != 64
            || record.port == 0
            || record.control_port == 0
        {
            bail!("invalid service ownership record")
        }
        Ok(Some(record))
    }

    async fn status(&self) -> Result<Status> {
        let Some(record) = self.record()? else {
            return Ok(stopped());
        };
        if !owned(&record)? {
            return Ok(stopped());
        }
        let ready = self.healthy(&record).await;
        Ok(Status {
            protocol: 1,
            state: if ready { "ready" } else { "starting" },
            pid: Some(record.pid),
            version: Some(record.version),
            port: Some(record.port),
            started_at: Some(record.started_at),
        })
    }

    async fn healthy(&self, record: &Record) -> bool {
        let Ok(client) = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
        else {
            return false;
        };
        let Ok(response) = client
            .get(format!("http://127.0.0.1:{}/api/info", record.port))
            .send()
            .await
        else {
            return false;
        };
        if !response.status().is_success() || response.content_length().is_some_and(|n| n > 16384) {
            return false;
        }
        let mut response = response;
        let mut bytes = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) if bytes.len() + chunk.len() <= 16384 => {
                    bytes.extend_from_slice(&chunk)
                }
                Ok(None) => break,
                _ => return false,
            }
        }
        let Ok(info) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return false;
        };
        info["protocol"] == 1
            && info["pid"] == record.pid
            && info["port"] == record.port
            && info["version"] == record.version
    }

    async fn start(&self, port: u16, strict: bool) -> Result<Status> {
        let status = self.status().await?;
        if status.state == "ready" {
            return Ok(status);
        }
        if status.pid.is_some() {
            bail!("owned service is running but unhealthy; inspect service.log or use service restart")
        }
        let log = private_file(&self.dir.join("service.log"), true)?;
        let mut command = Command::new(std::env::current_exe()?);
        command.arg("--config").arg(&self.config).args([
            "service",
            "run",
            "--port",
            &port.to_string(),
        ]);
        if strict {
            command.arg("--port-strict");
        }
        command
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        // Detach from ADB/launcher terminal; the child owns its runtime lock and control socket.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().context("cannot launch native service")?;
        for _ in 0..150 {
            if let Some(exit) = child.try_wait()? {
                bail!(
                    "service startup failed ({exit}); inspect {}",
                    self.dir.join("service.log").display()
                )
            }
            let status = self.status().await?;
            if status.state == "ready" && status.pid == Some(child.id()) {
                // Reap when hosted by a persistent runtime; detached CLI exit also releases this waiter.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(status);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // This PID is our direct child, so killing it does not rely on stale state.
        let _ = child.kill();
        let _ = child.wait();
        bail!("service did not become ready within 15 seconds; inspect service.log")
    }

    async fn stop(&self) -> Result<()> {
        let Some(record) = self.record()? else {
            return Ok(());
        };
        if !owned(&record)? {
            return Ok(());
        }
        let mut stream = tokio::time::timeout(
            Duration::from_secs(2),
            TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, record.control_port)),
        )
        .await
        .context("service control connection timed out")??;
        // Only the token from a verified owned record authorizes shutdown. No signal-by-PID fallback.
        stream
            .write_all(format!("{}\n", record.token).as_bytes())
            .await?;
        let mut reply = [0u8; 3];
        tokio::time::timeout(Duration::from_secs(2), stream.read_exact(&mut reply)).await??;
        if &reply != b"ok\n" {
            bail!("service rejected shutdown authorization")
        }
        for _ in 0..100 {
            if !owned(&record)? {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        bail!(
            "service did not stop gracefully within 10 seconds; no unrelated process was signaled"
        )
    }

    /// Foreground implementation used by a detached child. Accepts SIGTERM/Ctrl+C and private shutdown.
    pub async fn serve(&self, port: u16, strict: bool) -> Result<()> {
        let _running = self.lock("running.lock").await?;
        // Android shell SELinux domains may reject filesystem Unix sockets. Keep control local
        // and authorize it with a private random token; no unauthenticated HTTP stop endpoint.
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .context("cannot bind local service control channel")?;
        let control_port = listener.local_addr()?.port();
        let mut web = crate::web_ui::start_on_port(self.config.clone(), port, strict).await?;
        let pid = std::process::id();
        let metadata = fs::metadata(format!("/proc/{pid}/exe"))?;
        let mut random = [0u8; 32];
        File::open("/dev/urandom")?.read_exact(&mut random)?;
        let record = Record {
            protocol: 1,
            pid,
            uid: nix::unistd::geteuid().as_raw(),
            start_ticks: start_ticks(pid)?.context("cannot identify current process")?,
            exe_device: metadata.dev(),
            exe_inode: metadata.ino(),
            version: VERSION.into(),
            port: web.port(),
            started_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            control_port,
            token: random.iter().map(|byte| format!("{byte:02x}")).collect(),
        };
        let mut temporary = tempfile::NamedTempFile::new_in(&self.dir)?;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        serde_json::to_writer(&mut temporary, &record)?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        temporary.persist(self.dir.join("state.json"))?;
        let _cleanup = Cleanup {
            state: self.dir.join("state.json"),
        };
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => { result?; },
            _ = terminate.recv() => {},
            result = authorized_shutdown(listener, &record.token) => { result?; },
            result = web.wait() => { return result.context("service web server stopped"); },
        }
        web.shutdown().await
    }
}

fn private_file(path: &Path, append: bool) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .append(append)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != nix::unistd::geteuid().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        bail!("service file has unsafe ownership or permissions")
    }
    Ok(file)
}

fn stopped() -> Status {
    Status {
        protocol: 1,
        state: "stopped",
        pid: None,
        version: None,
        port: None,
        started_at: None,
    }
}

fn start_ticks(pid: u32) -> Result<Option<u64>> {
    let stat = match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("cannot verify service process start time"),
    };
    let fields = stat.rsplit_once(')').context("invalid process stat")?.1;
    let ticks = fields
        .split_whitespace()
        .nth(19)
        .context("missing process start time")?
        .parse()?;
    Ok(Some(ticks))
}

fn owned(record: &Record) -> Result<bool> {
    if start_ticks(record.pid)? != Some(record.start_ticks) {
        return Ok(false);
    }
    let metadata = match fs::metadata(format!("/proc/{}/exe", record.pid)) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("cannot verify service executable identity"),
    };
    Ok(metadata.dev() == record.exe_device
        && metadata.ino() == record.exe_inode
        && fs::metadata(format!("/proc/{}", record.pid))?.uid() == record.uid)
}

async fn authorized_shutdown(listener: TcpListener, token: &str) -> Result<()> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut bytes = [0u8; 65];
        let received =
            tokio::time::timeout(Duration::from_secs(1), stream.read_exact(&mut bytes)).await;
        if matches!(received, Ok(Ok(_))) && bytes[64] == b'\n' && &bytes[..64] == token.as_bytes() {
            stream.write_all(b"ok\n").await?;
            return Ok(());
        }
    }
}

struct Cleanup {
    state: PathBuf,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_other_uid_without_changing_ownership() {
        if !nix::unistd::geteuid().is_root() {
            return; // Changing a fixture's owner requires root.
        }
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config.toml");
        let dir = config.with_extension("service");
        fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        nix::unistd::chown(&dir, Some(nix::unistd::Uid::from_raw(2000)), None).unwrap();
        let error = Service::new(&config).err().unwrap().to_string();
        assert!(error.contains(&dir.display().to_string()));
        assert!(error.contains("current UID 0"));
        assert!(error.contains("found UID 2000, mode 0700"));
        assert!(error.contains("stop the service before switching users"));
        assert_eq!(fs::symlink_metadata(&dir).unwrap().uid(), 2000);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn refuses_shared_directory_and_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config.toml");
        let dir = config.with_extension("service");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Service::new(&config).is_err());
        fs::remove_dir(&dir).unwrap();
        std::os::unix::fs::symlink(temp.path(), &dir).unwrap();
        assert!(Service::new(&config).is_err());
    }
    #[test]
    fn reused_pid_identity_is_never_owned() {
        let pid = std::process::id();
        let metadata = fs::metadata(format!("/proc/{pid}/exe")).unwrap();
        let mut record = Record {
            protocol: 1,
            pid,
            uid: nix::unistd::geteuid().as_raw(),
            start_ticks: start_ticks(pid).unwrap().unwrap(),
            exe_device: metadata.dev(),
            exe_inode: metadata.ino(),
            version: VERSION.into(),
            port: 9999,
            started_at: 0,
            token: "0".repeat(64),
            control_port: 1,
        };
        assert!(owned(&record).unwrap());
        record.start_ticks += 1;
        assert!(!owned(&record).unwrap());
        record.start_ticks -= 1;
        record.exe_inode += 1;
        assert!(!owned(&record).unwrap());
    }
}
