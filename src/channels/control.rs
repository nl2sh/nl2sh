//! Same-UID local management IPC lets a TUI coexist with the managed Web process.
//! The socket exposes status, owner actions and already-assessed delivery, never approval decisions.
use super::*;
#[cfg(target_os = "android")]
use std::os::android::net::SocketAddrExt;
#[cfg(target_os = "linux")]
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener as StdListener, UnixStream as StdStream};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

fn address(path: &Path) -> Result<SocketAddr> {
    let root = std::fs::canonicalize(crate::config::state_dir(path)?.join("channels"))?;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        SocketAddr::from_abstract_name(
            format!(
                "nl2sh-channels-{}-{}",
                unsafe { libc::geteuid() },
                hex_digest(&root.to_string_lossy())
            )
            .as_bytes(),
        )
        .context("cannot identify channel control socket")
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        SocketAddr::from_pathname(root.join("control.sock"))
            .context("cannot identify channel control socket")
    }
}
fn config_identity(path: &Path) -> Result<String> {
    let path = if path.exists() {
        std::fs::canonicalize(path)?
    } else {
        std::fs::canonicalize(
            path.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new(".")),
        )?
        .join(path.file_name().context("configuration filename missing")?)
    };
    Ok(hex_digest(&path.to_string_lossy()))
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    config: String,
    action: Option<ChannelAction>,
    #[serde(default)]
    list_targets: bool,
    #[serde(default)]
    outgoing: Option<outbound::Outbound>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    channels: Option<Vec<ChannelStatus>>,
    error: Option<String>,
    targets: Option<Vec<outbound::ChannelTarget>>,
    receipt: Option<Value>,
}
const MAX_REQUEST: u64 = 28 * 1024 * 1024 + 16384;
async fn exchange_request(
    path: &Path,
    action: Option<&ChannelAction>,
    list_targets: bool,
    outgoing: Option<outbound::Outbound>,
) -> Result<Reply> {
    let address = address(path)?;
    let stream = tokio::task::spawn_blocking(move || StdStream::connect_addr(&address)).await??;
    stream.set_nonblocking(true)?;
    let mut stream = UnixStream::from_std(stream)?;
    if stream.peer_cred()?.uid() != unsafe { libc::geteuid() } {
        bail!("channel manager UID does not match");
    }
    let request = serde_json::to_vec(&Request {
        config: config_identity(path)?,
        action: action.cloned(),
        list_targets,
        outgoing,
    })?;
    tokio::time::timeout(
        Duration::from_secs(if request.len() > 16384 {
            600
        } else if action.is_some() {
            60
        } else if list_targets {
            2
        } else {
            180
        }),
        async {
            stream.write_all(&request).await?;
            stream.shutdown().await?;
            let mut bytes = Vec::new();
            (&mut stream)
                .take(2 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .await?;
            if bytes.len() > 2 * 1024 * 1024 {
                bail!("channel control response exceeds limit");
            }
            let reply: Reply = serde_json::from_slice(&bytes)?;
            if let Some(error) = &reply.error {
                bail!("{error}");
            }
            Ok(reply)
        },
    )
    .await
    .context("channel manager did not respond")?
}
pub(super) async fn snapshot(path: &Path) -> Result<Vec<ChannelStatus>> {
    exchange_request(path, None, false, None)
        .await?
        .channels
        .context("channel status response incomplete")
}
pub(super) async fn apply(path: &Path, action: &ChannelAction) -> Result<()> {
    exchange_request(path, Some(action), false, None).await?;
    Ok(())
}
pub(super) async fn targets(path: &Path) -> Result<Vec<outbound::ChannelTarget>> {
    exchange_request(path, None, true, None)
        .await?
        .targets
        .context("channel target response incomplete")
}
pub(super) async fn deliver(path: &Path, outgoing: outbound::Outbound) -> Result<Value> {
    exchange_request(path, None, false, Some(outgoing))
        .await?
        .receipt
        .context("channel delivery response incomplete")
}
pub(super) async fn start(manager: &Arc<ChannelManager>) -> Result<()> {
    let address = address(&manager.path)?;
    let listener = StdListener::bind_addr(&address)?;
    listener.set_nonblocking(true)?;
    let listener = UnixListener::from_std(listener)?;
    let weak = Arc::downgrade(manager);
    let (cancel, mut rx) = watch::channel(false);
    let permits = Arc::new(tokio::sync::Semaphore::new(2));
    let task = tokio::spawn(async move {
        loop {
            let accepted =
                tokio::select! {biased;_=cancelled(&mut rx)=>break,r=listener.accept()=>r};
            let Ok((mut stream, _)) = accepted else {
                break;
            };
            if !stream
                .peer_cred()
                .is_ok_and(|c| c.uid() == unsafe { libc::geteuid() })
            {
                continue;
            }
            let Ok(permit) = permits.clone().try_acquire_owned() else {
                continue;
            };
            let manager = weak.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let result = async {
                    let mut bytes = Vec::new();
                    tokio::time::timeout(
                        Duration::from_secs(10),
                        (&mut stream).take(MAX_REQUEST + 1).read_to_end(&mut bytes),
                    )
                    .await
                    .context("channel control request read timed out")??;
                    if bytes.len() as u64 > MAX_REQUEST {
                        bail!("channel control request exceeds limit");
                    }
                    let request: Request = serde_json::from_slice(&bytes)?;
                    let manager = manager.upgrade().context("channel manager stopped")?;
                    if request.config != config_identity(&manager.path)? {
                        bail!("channel manager belongs to a different configuration");
                    }
                    let mut reply = Reply {
                        channels: None,
                        error: None,
                        targets: None,
                        receipt: None,
                    };
                    if request.outgoing.is_some()
                        && (request.action.is_some() || request.list_targets)
                    {
                        bail!("invalid channel request combination");
                    }
                    if let Some(outgoing) = request.outgoing {
                        reply.receipt = Some(Box::pin(manager.deliver(outgoing)).await?);
                    } else if request.list_targets {
                        reply.targets = Some(Box::pin(manager.targets()).await?);
                    } else {
                        if let Some(action) = request.action {
                            manager.apply(action).await?;
                        }
                        reply.channels = Some(manager.snapshot().await?);
                    }
                    Ok::<_, anyhow::Error>(reply)
                }
                .await;
                let reply = match result {
                    Ok(reply) => reply,
                    Err(error) => Reply {
                        channels: None,
                        error: Some(error.to_string()),
                        targets: None,
                        receipt: None,
                    },
                };
                if let Ok(bytes) = serde_json::to_vec(&reply) {
                    let _ = tokio::time::timeout(Duration::from_secs(3), stream.write_all(&bytes))
                        .await;
                }
            });
        }
    });
    *manager.controller.lock().await = Some(Job { cancel, task });
    Ok(())
}
