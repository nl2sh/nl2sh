//! Shared, bounded delivery to runtime-known conversations; no credentials leave the owner.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use schemars::JsonSchema;

/// A native command button. Clicking inserts/sends this command through normal intake.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MessageButton {
    /// Visible label, at most ten characters.
    pub label: String,
    /// Text inserted in the conversation, at most 256 bytes.
    pub command: String,
}
/// A runtime-known destination and its supported message formats.
#[derive(Clone, Serialize, Deserialize)]
pub struct ChannelTarget {
    /// Opaque runtime ID; invalidated on disconnect/restart.
    pub id: String,
    /// Adapter identifier.
    pub kind: String,
    /// Bot account ID.
    pub account: String,
    /// Conversation peer ID.
    pub peer: String,
    /// Original sender, including the originating member for groups.
    pub sender: String,
    /// Whether the peer is a group.
    pub group: bool,
    /// Native Markdown or compatible filtered Markdown.
    pub markdown: String,
    /// Supported media kinds.
    pub media: Vec<String>,
    /// Whether native command buttons are supported.
    pub buttons: bool,
}
pub(super) struct KnownTarget {
    pub info: ChannelTarget,
    pub message: Incoming,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Outbound {
    pub target: String,
    pub kind: String,
    pub text: String,
    pub name: String,
    pub data: Option<String>,
    pub buttons: Vec<MessageButton>,
}
pub(crate) fn validate_buttons(buttons: &[MessageButton]) -> Result<()> {
    if buttons.len() > 5
        || buttons.iter().any(|b| {
            b.label.trim().is_empty()
                || b.label.chars().count() > 10
                || b.label.chars().any(char::is_control)
                || b.command.trim().is_empty()
                || b.command.len() > 256
                || b.command.chars().any(char::is_control)
                || matches!(
                    b.command.split_whitespace().next(),
                    Some("/approve" | "/deny" | "/answer")
                )
        })
    {
        bail!(
            "invalid channel buttons: at most five, labels 1–10 characters, commands 1–256 bytes; approval commands are reserved"
        );
    }
    Ok(())
}
impl ChannelManager {
    pub(super) fn remember_target(&self, message: &Incoming) -> Result<()> {
        let mut targets = self
            .targets
            .lock()
            .map_err(|_| anyhow!("channel targets unavailable"))?;
        let key = message.route.key();
        if targets.len() >= 512 && !targets.contains_key(&key) {
            return Ok(());
        }
        let route = &message.route;
        if [&route.account, &route.peer, &route.sender]
            .iter()
            .any(|s| s.is_empty() || s.len() > 512 || s.chars().any(char::is_control))
        {
            bail!("invalid channel route");
        }
        let id = targets
            .get(&key)
            .map(|t| t.info.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        targets.insert(
            key,
            KnownTarget {
                info: ChannelTarget {
                    id,
                    kind: route.kind.clone(),
                    account: route.account.clone(),
                    peer: route.peer.clone(),
                    sender: route.sender.clone(),
                    group: route.group,
                    markdown: if route.kind == "qq" {
                        "native"
                    } else {
                        "compatible"
                    }
                    .into(),
                    media: ["image", "audio", "video", "file"]
                        .map(str::to_owned)
                        .to_vec(),
                    buttons: route.kind == "qq",
                },
                message: Incoming {
                    route: route.clone(),
                    id: message.id.clone(),
                    text: String::new(),
                    context: message.context.clone(),
                    media: Vec::new(),
                },
            },
        );
        Ok(())
    }
    /// List currently known routes without exposing context tokens or credentials.
    pub async fn targets(&self) -> Result<Vec<ChannelTarget>> {
        if self.store.is_none() {
            return control::targets(&self.path).await;
        }
        Ok(self
            .targets
            .lock()
            .map_err(|_| anyhow!("channel targets unavailable"))?
            .values()
            .map(|t| t.info.clone())
            .collect())
    }
    pub(super) async fn deliver(&self, outgoing: Outbound) -> Result<Value> {
        if self.store.is_none() {
            return control::deliver(&self.path, outgoing).await;
        }
        if self.closing.load(std::sync::atomic::Ordering::Acquire) {
            bail!("channel gateway is shutting down");
        }
        validate_buttons(&outgoing.buttons)?;
        if outgoing.text.len() > 12000 {
            bail!("channel message exceeds 12000 bytes");
        }
        let message = self
            .targets
            .lock()
            .map_err(|_| anyhow!("channel targets unavailable"))?
            .values()
            .find(|t| t.info.id == outgoing.target)
            .map(|t| t.message.clone())
            .context("unknown or disconnected channel target; list targets again")?;
        let channel = self
            .adapters
            .lock()
            .map_err(|_| anyhow!("channel adapters unavailable"))?
            .get(&message.route.kind)
            .cloned()
            .context("channel disconnected")?;
        let account = self
            .account(&message.route.kind)
            .await
            .context("channel unbound")?;
        if !account.enabled || account.id != message.route.account {
            bail!("channel target account changed");
        }
        let cancel = self
            .delivery_cancels
            .lock()
            .map_err(|_| anyhow!("channel cancellation unavailable"))?
            .get(&message.route.kind)
            .cloned();
        let send = async {
            let text = self.redact(&outgoing.text).await;
            if let Some(data) = outgoing.data {
                if !outgoing.buttons.is_empty()
                    || text.len() > 1000
                    || data.len() > media::MAX_FILE_BYTES.div_ceil(3) * 4
                    || !matches!(outgoing.kind.as_str(), "image" | "audio" | "video" | "file")
                {
                    bail!("invalid media delivery");
                }
                let bytes = STANDARD.decode(data)?;
                if bytes.is_empty() || bytes.len() > media::MAX_FILE_BYTES {
                    bail!("invalid media size");
                }
                channel
                    .send_media(
                        &message,
                        &media::safe_name(&outgoing.name),
                        &outgoing.kind,
                        bytes,
                        &text,
                    )
                    .await?;
                Ok(json!({"status":"submitted", "target":outgoing.target,"kind":outgoing.kind}))
            } else {
                if !matches!(outgoing.kind.as_str(), "text" | "markdown") || text.trim().is_empty()
                {
                    bail!("invalid text delivery");
                }
                let receipt = channel
                    .send_formatted(&message, &text, &outgoing.kind, &outgoing.buttons)
                    .await?;
                Ok(json!({"status":"submitted", "target":outgoing.target,"receipt":receipt}))
            }
        };
        if let Some(mut cancel) = cancel {
            tokio::select! {biased;_=cancelled(&mut cancel)=>bail!("channel stopped during delivery; result may be partial; do not automatically retry"), result=send=>result}
        } else {
            send.await
        }
    }
}
/// Resolve a runtime-known target using this configuration's existing gateway.
pub(crate) async fn delivery(
    path: &Path,
    target: &str,
) -> Result<Arc<dyn crate::agent::FileDelivery>> {
    let targets = list(path).await?;
    let info = targets
        .into_iter()
        .find(|t| t.id == target)
        .context("unknown channel target; call list_channel_targets first")?;
    Ok(Arc::new(TargetDelivery {
        path: path.to_owned(),
        info,
    }))
}
/// List destinations from an existing same-configuration gateway, locally or via same-UID IPC.
pub async fn list(path: &Path) -> Result<Vec<ChannelTarget>> {
    if let Some(manager) = ChannelManager::lookup(path) {
        manager.targets().await
    } else {
        control::targets(path).await
    }
}
struct TargetDelivery {
    path: PathBuf,
    info: ChannelTarget,
}
impl TargetDelivery {
    async fn send(&self, outgoing: Outbound) -> Result<Value> {
        if let Some(manager) = ChannelManager::lookup(&self.path) {
            manager.deliver(outgoing).await
        } else {
            control::deliver(&self.path, outgoing).await
        }
    }
}
#[async_trait]
impl crate::agent::FileDelivery for TargetDelivery {
    fn approval_destination(&self) -> String {
        format!(
            "{} / {} / {} (sender {})",
            self.info.kind, self.info.account, self.info.peer, self.info.sender
        )
    }
    fn supports_buttons(&self) -> bool {
        self.info.buttons
    }
    fn destination(&self) -> String {
        format!(
            "{} / {} / {} (sender {}, target {})",
            self.info.kind, self.info.account, self.info.peer, self.info.sender, self.info.id
        )
    }
    async fn send_message(
        &self,
        text: &str,
        format: &str,
        buttons: &[MessageButton],
    ) -> Result<Value> {
        self.send(Outbound {
            target: self.info.id.clone(),
            kind: format.into(),
            text: text.into(),
            name: String::new(),
            data: None,
            buttons: buttons.to_vec(),
        })
        .await
    }
    async fn send_file(&self, name: &str, kind: &str, bytes: Vec<u8>, caption: &str) -> Result<()> {
        self.send(Outbound {
            target: self.info.id.clone(),
            kind: kind.into(),
            text: caption.into(),
            name: name.into(),
            data: Some(STANDARD.encode(bytes)),
            buttons: Vec::new(),
        })
        .await?;
        Ok(())
    }
}
