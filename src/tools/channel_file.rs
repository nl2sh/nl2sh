//! Approved, route-bound file delivery with an immutable pre-approval snapshot.
use super::*;
use crate::agent::FileDelivery;
use schemars::JsonSchema;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{io::Read, os::unix::fs::OpenOptionsExt, path::PathBuf, sync::Arc};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SendArgs {
    /// Opaque ID from list_channel_targets; omit only in a channel conversation.
    #[serde(default)]
    pub(super) target: Option<String>,
    /// Existing local file. Provide exactly one of path or attachment_id; no URL is accepted.
    #[serde(default)]
    pub(super) path: Option<String>,
    /// Opaque ID returned by a tool in this channel task, such as android.screenshot without a path.
    #[serde(default)]
    pub(super) attachment_id: Option<String>,
    /// auto, image, audio, video, or file; file preserves arbitrary binary formats.
    #[serde(default = "auto")]
    pub(super) kind: String,
    /// Optional accompanying text (maximum 1000 UTF-8 bytes).
    #[serde(default)]
    pub(super) caption: String,
}
fn auto() -> String {
    "auto".into()
}
const META: ToolMetadata = ToolMetadata {
    name: "send_channel_file",
    description: "Send a local image, audio, video or arbitrary file, or a tool attachment by its current-task attachment_id, to the originating message-channel conversation, after approval of its destination, filename, size and SHA-256. Outside channel tasks, specify a target from list_channel_targets. Does not execute the file or follow remote URLs.",
    category: ToolCategory::File, risk: ToolRisk::Mutating,
    requires: &[], group: None, default_enabled: true,
    platform: ToolPlatform::Any, runtime: RuntimeRequirement::None,
    concurrency: ToolConcurrency::Sequential, lifetime: ToolLifetime::Call,
    schema: descriptor_schema::<SendArgs>,
};
struct SendOperation {
    delivery: Arc<dyn FileDelivery>,
    name: String,
    kind: String,
    bytes: Vec<u8>,
    caption: String,
}
#[async_trait]
impl PreparedExecution for SendOperation {
    fn media_approval_scope(&self) -> Option<crate::agent::MediaApprovalScope> {
        Some(crate::agent::MediaApprovalScope {
            destination: self.delivery.approval_destination(),
            kind: self.kind.clone(),
        })
    }
    async fn execute(self: Box<Self>, _: &mut ToolContext<'_>) -> Result<ToolOutput> {
        self.delivery
            .send_file(&self.name, &self.kind, self.bytes, &self.caption)
            .await?;
        Ok(ToolOutput::success(serde_json::json!({"status":"submitted","destination":self.delivery.destination(),"kind":self.kind,"name":self.name}).to_string()))
    }
}
pub(super) async fn prepare(ctx: &ToolContext<'_>, args: SendArgs) -> Result<PreparedToolCall> {
    let delivery = super::channel_message::delivery(ctx, args.target.as_deref()).await?;
    let snapshot_delivery = ctx
        .confirmer
        .and_then(|c| c.file_delivery())
        .unwrap_or_else(|| delivery.clone());
    let zh = ctx
        .config
        .is_none_or(|c| c.ui_language == crate::config::UiLanguage::ZhCn);
    if args.caption.len() > 1000
        || args.path.is_some() == args.attachment_id.is_some()
        || args
            .path
            .as_ref()
            .is_some_and(|p| p.len() > 4096 || p.chars().any(char::is_control))
        || args
            .attachment_id
            .as_ref()
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_err())
        || !matches!(
            args.kind.as_str(),
            "auto" | "image" | "audio" | "video" | "file"
        )
    {
        anyhow::bail!("invalid file kind, path or caption length");
    }
    let (source, name, bytes, digest) = tokio::task::spawn_blocking(move || -> Result<_> {
        if let Some(id) = args.attachment_id {
            let (name, bytes) = snapshot_delivery.attachment(&id)?;
            let digest = format!("{:x}", Sha256::digest(&bytes));
            return Ok((
                format!(
                    "{}: {id}",
                    if zh {
                        "附件（仅当前任务）"
                    } else {
                        "Attachment (current task only)"
                    }
                ),
                name,
                bytes,
                digest,
            ));
        }
        let path = std::fs::canonicalize(PathBuf::from(args.path.context("file path missing")?))?;
        if path.to_string_lossy().chars().any(char::is_control) {
            anyhow::bail!("file path contains control characters");
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
            .open(&path)?;
        if !file.metadata()?.is_file() {
            anyhow::bail!("only regular files can be sent");
        }
        let mut bytes = Vec::new();
        file.take(crate::channels::media::MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.is_empty() || bytes.len() > crate::channels::media::MAX_FILE_BYTES {
            anyhow::bail!("file must contain 1–20 MiB");
        }
        let name = crate::channels::media::safe_name(
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("document.bin"),
        );
        let digest = format!("{:x}", Sha256::digest(&bytes));
        Ok((
            format!("{}: {}", if zh { "路径" } else { "Path" }, path.display()),
            name,
            bytes,
            digest,
        ))
    })
    .await
    .context("file snapshot worker failed")??;
    let kind = if args.kind == "auto" {
        crate::channels::media::kind_for_name(&name)
    } else {
        args.kind.as_str()
    }
    .to_owned();
    if bytes.is_empty() || bytes.len() > crate::channels::media::MAX_FILE_BYTES {
        anyhow::bail!("file must contain 1–20 MiB");
    }
    let labels = if zh {
        ["发送文件", "目标", "文件名", "类型", "字节数", "说明"]
    } else {
        [
            "Send file",
            "Destination",
            "Filename",
            "Type",
            "Size",
            "Caption",
        ]
    };
    let preview = format!(
        "{}\n{}: {}\n{}\n{}: {}\n{}: {}\n{}: {}\nSHA-256: {}\n{}: {}",
        labels[0],
        labels[1],
        delivery.destination(),
        source,
        labels[2],
        name,
        labels[3],
        kind,
        labels[4],
        bytes.len(),
        digest,
        labels[5],
        args.caption
    );
    Ok(PreparedToolCall::operation(
        preview,
        Box::new(SendOperation {
            delivery,
            name,
            kind,
            bytes,
            caption: args.caption,
        }),
    ))
}
define_tool!(SendChannelFileTool, SendArgs, META, prepare);
