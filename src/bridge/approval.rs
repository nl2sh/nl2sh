//! One-time, device-local approval channel for direct bridge tool calls.

use crate::{
    agent::{ConfirmationDecision, Confirmer},
    config,
    security::SecurityAssessment,
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io::{self, IsTerminal, Write},
    os::fd::AsRawFd,
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    net::UnixListener,
    time::{timeout, Instant},
};

const ROOT_NAME: &str = ".nl2sh-a";
const REQUEST_FILE: &str = "request.json";
#[cfg(not(target_os = "android"))]
const SOCKET_FILE: &str = "s";
const LIVE_FILE: &str = "live";
const LOCK_FILE: &str = "lock";
const MAX_PENDING: usize = 8;
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingRequest {
    id: String,
    preview: String,
    risk: String,
    requires_root: bool,
    requires_double_confirmation: bool,
    rules: Vec<String>,
}

/// Confirmer that pauses a direct bridge call for one local terminal decision.
pub(super) struct BridgeApprovalConfirmer {
    root: PathBuf,
}

impl BridgeApprovalConfirmer {
    pub(super) fn new(config_path: &Path) -> Result<Self> {
        Ok(Self {
            root: approval_root(config_path)?,
        })
    }
}

struct ApprovalLock(File);

impl ApprovalLock {
    async fn acquire(root: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(root.join(LOCK_FILE))
            .context("cannot open approval state lock")?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            bail!("approval state lock must be a private file owned by the current user")
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(Self(file));
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::WouldBlock {
                return Err(error).context("cannot lock approval state");
            }
            if Instant::now() >= deadline {
                bail!("approval state is busy")
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

impl Drop for ApprovalLock {
    fn drop(&mut self) {
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

#[async_trait]
impl Confirmer for BridgeApprovalConfirmer {
    async fn confirm(
        &self,
        preview: &str,
        assessment: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        if preview.is_empty() || preview.len() > 64 * 1024 {
            bail!("approval preview must contain 1–65536 bytes")
        }
        let gate = ApprovalLock::acquire(&self.root).await?;
        let pending = prune_stale_and_count(&self.root)?;
        if pending >= MAX_PENDING {
            bail!("too many pending device approvals")
        }
        let directory = tempfile::Builder::new()
            .prefix("r")
            .rand_bytes(16)
            .tempdir_in(&self.root)
            .context("cannot create private approval request")?;
        let id = directory
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .context("approval request has no valid identifier")?
            .to_owned();
        let request = PendingRequest {
            id,
            preview: preview.to_owned(),
            risk: format!("{:?}", assessment.risk_level),
            requires_root: assessment.requires_root,
            requires_double_confirmation: assessment.requires_double_confirmation,
            rules: assessment
                .matched_rules
                .iter()
                .map(|rule| format!("{}: {}", rule.id, rule.message))
                .collect(),
        };
        let socket = bind_socket(directory.path(), &request.id)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.path().join(REQUEST_FILE))
            .context("cannot write approval request")?;
        serde_json::to_writer(&mut file, &request).context("cannot encode approval request")?;
        file.flush().context("cannot flush approval request")?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.path().join(LIVE_FILE))
            .context("cannot publish approval request")?;
        drop(gate);
        let deadline = Instant::now() + APPROVAL_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(ConfirmationDecision::Reject);
            }
            let (stream, _) = match timeout(remaining, socket.accept()).await {
                Ok(Ok(connection)) => connection,
                Ok(Err(error)) => return Err(error).context("approval socket failed"),
                Err(_) => return Ok(ConfirmationDecision::Reject),
            };
            if !peer_is_current_user(&stream)? {
                continue;
            }
            let mut decision = Vec::new();
            match timeout(
                Duration::from_secs(3),
                stream.take(16).read_to_end(&mut decision),
            )
            .await
            {
                Ok(Ok(_)) if decision == b"approve\n" => return Ok(ConfirmationDecision::Approve),
                Ok(Ok(_)) if decision == b"reject\n" => return Ok(ConfirmationDecision::Reject),
                _ => continue,
            }
        }
    }
}

fn prune_stale_and_count(root: &Path) -> Result<usize> {
    let mut count = 0;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let file_type = match entry.file_type() {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).context("cannot inspect approval request"),
        };
        if !file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        let Some(id) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if request_path(root, id).is_err() || !path.join(LIVE_FILE).exists() {
            continue;
        }
        if connect_socket(&path).is_ok() {
            count += 1;
        } else {
            match fs::remove_dir_all(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).context("cannot remove expired approval request"),
            }
        }
    }
    Ok(count)
}

fn bind_socket(path: &Path, id: &str) -> Result<UnixListener> {
    #[cfg(target_os = "android")]
    {
        use std::os::{
            android::net::SocketAddrExt,
            unix::net::{SocketAddr, UnixListener as StdListener},
        };
        let _ = path;
        let address = SocketAddr::from_abstract_name(format!("nl2sh-approval-{id}").as_bytes())?;
        let listener =
            StdListener::bind_addr(&address).context("cannot open Android approval socket")?;
        listener.set_nonblocking(true)?;
        UnixListener::from_std(listener).context("cannot register Android approval socket")
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = id;
        UnixListener::bind(path.join(SOCKET_FILE)).context("cannot open local approval socket")
    }
}

fn peer_is_current_user(stream: &tokio::net::UnixStream) -> Result<bool> {
    #[cfg(target_os = "android")]
    {
        use std::os::fd::AsRawFd;
        let mut credentials = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let result = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&raw mut credentials).cast(),
                &raw mut size,
            )
        };
        if result != 0 || size as usize != std::mem::size_of::<libc::ucred>() {
            return Err(std::io::Error::last_os_error())
                .context("cannot verify approval peer credentials");
        }
        Ok(credentials.uid == unsafe { libc::geteuid() })
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = stream;
        Ok(true)
    }
}

fn approval_root(config_path: &Path) -> Result<PathBuf> {
    let state = config::state_dir(config_path)?;
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&state)
        .with_context(|| format!("cannot create approval state directory {}", state.display()))?;
    let root = state.join(ROOT_NAME);
    match fs::symlink_metadata(&root) {
        Ok(metadata) => validate_root(&metadata)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match DirBuilder::new().mode(0o700).create(&root) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("cannot create approval directory {}", root.display())
                    })
                }
            }
            validate_root(&fs::symlink_metadata(&root)?)?;
        }
        Err(error) => return Err(error).context("cannot inspect approval directory"),
    }
    Ok(root)
}

fn validate_root(metadata: &fs::Metadata) -> Result<()> {
    if !metadata.file_type().is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
    {
        bail!("approval directory must be a private directory owned by the current user")
    }
    Ok(())
}

fn request_path(root: &Path, id: &str) -> Result<PathBuf> {
    if id.len() < 2
        || id.len() > 32
        || !id.starts_with('r')
        || !id.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        bail!("invalid approval request identifier")
    }
    Ok(root.join(id))
}

/// Print currently live local approval requests without deciding them.
pub(super) fn list_pending(config_path: &Path) -> Result<()> {
    let root = approval_root(config_path)?;
    prune_stale_and_count(&root)?;
    let mut entries = fs::read_dir(root)?
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| read_pending(&entry.path()).ok())
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    if entries.is_empty() {
        println!("No pending device approvals.");
    }
    for request in entries {
        println!(
            "{}  risk={} root={}  {:?}",
            request.id,
            request.risk,
            request.requires_root,
            request.preview.lines().next().unwrap_or("")
        );
    }
    Ok(())
}

fn read_pending(path: &Path) -> Result<PendingRequest> {
    if !path.join(LIVE_FILE).exists() {
        bail!("approval request is no longer active")
    }
    let file = File::open(path.join(REQUEST_FILE)).context("cannot read approval request")?;
    let request: PendingRequest =
        serde_json::from_reader(file).context("invalid approval request")?;
    if path.file_name().and_then(|name| name.to_str()) != Some(request.id.as_str()) {
        bail!("approval request identifier mismatch")
    }
    Ok(request)
}

/// Ask one local terminal user to approve or reject a pending request.
pub(super) fn approve_interactive(config_path: &Path, id: &str) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("approval requires an interactive device terminal")
    }
    let root = approval_root(config_path)?;
    let path = request_path(&root, id)?;
    let request = read_pending(&path)?;
    println!(
        "Request: {}\nRisk: {}  Root: {}\nRules:",
        request.id, request.risk, request.requires_root
    );
    for rule in &request.rules {
        println!("  {rule:?}");
    }
    println!("Exact action:\n{:?}", request.preview);
    print!("Approve once? Type yes: ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    let first_answer = answer.trim().to_owned();
    let mut strong_answer = None;
    if first_answer == "yes" && request.requires_double_confirmation {
        print!("Strong confirmation: type APPROVE {}: ", request.id);
        io::stdout().flush()?;
        answer.clear();
        io::stdin().read_line(&mut answer)?;
        strong_answer = Some(answer.trim().to_owned());
    }
    let approved = approval_matches(&request, &first_answer, strong_answer.as_deref());
    send_decision(&path, approved)?;
    println!(
        "{}",
        if approved {
            "Approved once."
        } else {
            "Rejected."
        }
    );
    Ok(())
}

fn approval_matches(request: &PendingRequest, first: &str, strong: Option<&str>) -> bool {
    first == "yes"
        && (!request.requires_double_confirmation
            || strong.is_some_and(|answer| answer == format!("APPROVE {}", request.id)))
}

fn send_decision(path: &Path, approved: bool) -> Result<()> {
    let mut stream = connect_socket(path)?;
    stream
        .write_all(if approved { b"approve\n" } else { b"reject\n" })
        .context("cannot send approval decision")
}

fn connect_socket(path: &Path) -> Result<UnixStream> {
    #[cfg(target_os = "android")]
    {
        use std::os::{android::net::SocketAddrExt, unix::net::SocketAddr};
        let id = path
            .file_name()
            .and_then(|part| part.to_str())
            .context("invalid approval request path")?;
        let address = SocketAddr::from_abstract_name(format!("nl2sh-approval-{id}").as_bytes())?;
        UnixStream::connect_addr(&address)
            .context("approval request expired or is no longer active")
    }
    #[cfg(not(target_os = "android"))]
    {
        UnixStream::connect(path.join(SOCKET_FILE))
            .context("approval request expired or is no longer active")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        approval_matches, read_pending, request_path, send_decision, BridgeApprovalConfirmer,
        PendingRequest,
    };
    use crate::{config::Config, shell::ShellExecutor, tools::runtime::invoke};
    use anyhow::{Context, Result};
    use serde_json::json;
    use std::{fs, path::PathBuf, sync::Arc, time::Duration};
    use tokio::sync::Barrier;

    async fn run_decision(approved: bool, stale_requests: usize) -> Result<()> {
        let directory = tempfile::tempdir()?;
        let config_path = directory.path().join("config.toml");
        let target = directory.path().join("target.txt");
        let confirmer = BridgeApprovalConfirmer::new(&config_path)?;
        let root = confirmer.root.clone();
        let stale_paths = (0..stale_requests)
            .map(|index| root.join(format!("rdead{index:08}")))
            .collect::<Vec<_>>();
        for path in &stale_paths {
            fs::create_dir(path)?;
            fs::write(path.join(super::LIVE_FILE), "")?;
        }
        let config = Config::default();
        let executor = ShellExecutor::new(config.clone());
        let path = target.to_string_lossy().to_string();
        let task = tokio::spawn(async move {
            invoke(
                &config,
                &executor,
                &confirmer,
                "apply_patch",
                json!({
                    "path": path, "old_text": "", "new_text": "approved content"
                }),
            )
            .await
        });
        let request_path = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let candidate = fs::read_dir(&root)?
                    .filter_map(std::result::Result::ok)
                    .map(|entry| entry.path())
                    .find(|path| read_pending(path).is_ok());
                if let Some(path) = candidate {
                    return Ok::<PathBuf, anyhow::Error>(path);
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .context("approval request was not published")??;
        let request = read_pending(&request_path)?;
        for path in &stale_paths {
            assert!(!path.exists());
        }
        assert!(request.preview.contains("approved content"));
        assert!(!request.requires_double_confirmation);
        send_decision(&request_path, approved)?;
        let result = task.await.context("tool task failed")??;
        assert_eq!(result.success, approved);
        assert_eq!(target.exists(), approved);
        if approved {
            assert_eq!(fs::read_to_string(target)?, "approved content");
        }
        assert!(!request_path.exists());
        Ok(())
    }

    #[tokio::test]
    async fn local_approval_is_one_time_and_bound_to_pending_tool() -> Result<()> {
        run_decision(true, 0).await?;
        run_decision(false, 0).await?;
        Ok(())
    }

    #[tokio::test]
    async fn stale_requests_do_not_exhaust_approval_slots() -> Result<()> {
        run_decision(false, super::MAX_PENDING).await
    }

    #[tokio::test]
    async fn concurrent_calls_cannot_exceed_pending_approval_limit() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let config_path = directory.path().join("config.toml");
        let root = BridgeApprovalConfirmer::new(&config_path)?.root;
        let barrier = Arc::new(Barrier::new(super::MAX_PENDING * 2 + 1));
        let mut tasks = Vec::new();
        let mut targets = Vec::new();
        for index in 0..super::MAX_PENDING * 2 {
            let target = directory.path().join(format!("target-{index}.txt"));
            targets.push(target.clone());
            let config_path = config_path.clone();
            let barrier = barrier.clone();
            tasks.push(tokio::spawn(async move {
                let config = Config::default();
                let executor = ShellExecutor::new(config.clone());
                let confirmer = BridgeApprovalConfirmer::new(&config_path)?;
                barrier.wait().await;
                invoke(
                    &config,
                    &executor,
                    &confirmer,
                    "apply_patch",
                    json!({
                        "path": target.to_string_lossy(),
                        "old_text": "", "new_text": "unapproved",
                    }),
                )
                .await
            }));
        }
        barrier.wait().await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let pending = fs::read_dir(&root)?
                    .filter_map(std::result::Result::ok)
                    .filter(|entry| read_pending(&entry.path()).is_ok())
                    .count();
                let finished = tasks.iter().filter(|task| task.is_finished()).count();
                if pending == super::MAX_PENDING && finished == super::MAX_PENDING {
                    break Ok::<(), anyhow::Error>(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .context("concurrent approval limit did not settle")??;
        for entry in fs::read_dir(&root)? {
            let path = entry?.path();
            if read_pending(&path).is_ok() {
                send_decision(&path, false)?;
            }
        }
        let mut over_limit = 0;
        let mut refused = 0;
        for task in tasks {
            match task.await.context("approval task failed")? {
                Err(error)
                    if error
                        .to_string()
                        .contains("too many pending device approvals") =>
                {
                    over_limit += 1
                }
                Ok(result) if !result.success => refused += 1,
                other => anyhow::bail!("unexpected approval outcome: {other:?}"),
            }
        }
        assert_eq!(over_limit, super::MAX_PENDING);
        assert_eq!(refused, super::MAX_PENDING);
        assert!(targets.iter().all(|path| !path.exists()));
        Ok(())
    }

    #[test]
    fn approval_id_cannot_escape_private_directory() {
        assert!(request_path(std::path::Path::new("/tmp"), "../other").is_err());
    }

    #[test]
    fn dangerous_request_needs_second_exact_confirmation() {
        let request = PendingRequest {
            id: "rabc123".into(),
            preview: "action".into(),
            risk: "Dangerous".into(),
            requires_root: false,
            requires_double_confirmation: true,
            rules: vec![],
        };
        assert!(!approval_matches(&request, "yes", None));
        assert!(!approval_matches(&request, "yes", Some("APPROVE other")));
        assert!(!approval_matches(&request, "no", Some("APPROVE rabc123")));
        assert!(approval_matches(&request, "yes", Some("APPROVE rabc123")));
    }
}
