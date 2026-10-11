//! Channel discovery and message sending use the same assessed runtime in every entry point.
use super::*;
use crate::{agent::FileDelivery, channels::outbound::MessageButton};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub(super) async fn delivery(
    ctx: &ToolContext<'_>,
    target: Option<&str>,
) -> Result<Arc<dyn FileDelivery>> {
    if let Some(target) = target {
        let path = ctx
            .config
            .and_then(|c| c.source.as_deref())
            .context("active configuration has no source path")?;
        crate::channels::outbound::delivery(path, target).await
    } else {
        ctx.confirmer
            .and_then(|c| c.file_delivery())
            .context("target is required outside a channel conversation; call list_channel_targets")
    }
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListArgs {}
const LIST: ToolMetadata = ToolMetadata {
    name:"list_channel_targets", description:"List runtime-known WeChat/QQ destinations and Markdown, media and button capabilities for the active configuration. IDs expire on channel stop/restart; no arbitrary peer or context token can be supplied. Read-only; the gateway must already be running.",
    category:ToolCategory::File,risk:ToolRisk::ReadOnly,requires:&[],group:None,default_enabled:true,platform:ToolPlatform::Any,runtime:RuntimeRequirement::None,concurrency:ToolConcurrency::Sequential,lifetime:ToolLifetime::Call,schema:descriptor_schema::<ListArgs>,
};
async fn list(ctx: &ToolContext<'_>, _: ListArgs) -> Result<PreparedToolCall> {
    let path = ctx
        .config
        .and_then(|c| c.source.as_deref())
        .context("active configuration has no source path")?;
    let targets = crate::channels::outbound::list(path).await?;
    Ok(PreparedToolCall::operation(
        String::new(),
        Box::new(ListResult(serde_json::to_string(&targets)?)),
    ))
}
struct ListResult(String);
#[async_trait]
impl PreparedExecution for ListResult {
    async fn execute(self: Box<Self>, _: &mut ToolContext<'_>) -> Result<ToolOutput> {
        Ok(ToolOutput::success(self.0))
    }
}
define_tool!(ListChannelTargetsTool, ListArgs, LIST, list);

#[derive(Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Text,
    Markdown,
    Image,
    Audio,
    Video,
    File,
    Auto,
}
impl Kind {
    fn name(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Markdown => "markdown",
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::File => "file",
            Self::Auto => "auto",
        }
    }
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MessageArgs {
    /// Opaque ID from list_channel_targets; omit only to reply to the originating channel.
    #[serde(default)]
    target: Option<String>,
    /// text/markdown use text; image/audio/video/file/auto use exactly one path or attachment_id.
    kind: Kind,
    /// Message body, or optional media caption (maximum 1000 bytes for media).
    #[serde(default)]
    text: String,
    /// Local regular file; remote URLs are not accepted.
    #[serde(default)]
    path: Option<String>,
    /// Reference produced in this channel Agent task; never reusable across MCP calls or tasks.
    #[serde(default)]
    attachment_id: Option<String>,
    /// QQ native command buttons; not available in WeChat or media messages.
    #[serde(default)]
    buttons: Vec<MessageButton>,
}
const SEND:ToolMetadata=ToolMetadata {
    name:"send_channel_message",description:"Send text, native QQ Markdown, compatible WeChat Markdown, image, audio, video or arbitrary file to a known channel target, or the originating conversation. Always assessed and approved before transport; file bytes and SHA-256 are snapshotted before approval. QQ text/Markdown may include command buttons. Errors after partial delivery must not be automatically retried. MCP/A2A retain their own approval policy.",category:ToolCategory::File,risk:ToolRisk::Mutating,requires:&[],group:None,default_enabled:true,platform:ToolPlatform::Any,runtime:RuntimeRequirement::None,concurrency:ToolConcurrency::Sequential,lifetime:ToolLifetime::Call,schema:descriptor_schema::<MessageArgs>,
};
struct SendText {
    delivery: Arc<dyn FileDelivery>,
    text: String,
    format: String,
    buttons: Vec<MessageButton>,
}
#[async_trait]
impl PreparedExecution for SendText {
    async fn execute(self: Box<Self>, _: &mut ToolContext<'_>) -> Result<ToolOutput> {
        let receipt = self
            .delivery
            .send_message(&self.text, &self.format, &self.buttons)
            .await?;
        Ok(ToolOutput::success(serde_json::to_string(&receipt)?))
    }
}
async fn prepare(ctx: &ToolContext<'_>, args: MessageArgs) -> Result<PreparedToolCall> {
    crate::channels::outbound::validate_buttons(&args.buttons)?;
    if !matches!(args.kind, Kind::Text | Kind::Markdown) {
        if !args.buttons.is_empty() {
            anyhow::bail!("media messages do not support buttons");
        }
        return channel_file::prepare(
            ctx,
            channel_file::SendArgs {
                target: args.target,
                path: args.path,
                attachment_id: args.attachment_id,
                kind: args.kind.name().into(),
                caption: args.text,
            },
        )
        .await;
    }
    if args.path.is_some()
        || args.attachment_id.is_some()
        || args.text.trim().is_empty()
        || args.text.len() > 12000
    {
        anyhow::bail!("text/markdown require a nonempty body up to 12000 bytes and no attachment");
    }
    let delivery = delivery(ctx, args.target.as_deref()).await?;
    if !args.buttons.is_empty() && !delivery.supports_buttons() {
        anyhow::bail!("this destination does not support buttons");
    }
    let zh = ctx
        .config
        .is_none_or(|c| c.ui_language == crate::config::UiLanguage::ZhCn);
    let labels = if zh {
        ["发送消息", "目标", "格式", "正文", "按钮"]
    } else {
        ["Send message", "Destination", "Format", "Body", "Buttons"]
    };
    let preview = format!(
        "{}\n{}: {}\n{}: {}\n{}:\n{}\n{}: {}",
        labels[0],
        labels[1],
        delivery.destination(),
        labels[2],
        args.kind.name(),
        labels[3],
        args.text,
        labels[4],
        serde_json::to_string(&args.buttons)?
    );
    Ok(PreparedToolCall::operation(
        preview,
        Box::new(SendText {
            delivery,
            text: args.text,
            format: args.kind.name().into(),
            buttons: args.buttons,
        }),
    ))
}
define_tool!(SendChannelMessageTool, MessageArgs, SEND, prepare);
