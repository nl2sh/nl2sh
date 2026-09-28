use crate::shell::CommandExecutor;
use anyhow::{Context, Result};
use base64::Engine;
use image::{imageops::FilterType, GenericImageView};
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const MAX_UI_NODES: usize = 300;
const MAX_IMAGE_SOURCE_BYTES: usize = 32 * 1024 * 1024;
const MAX_IMAGE_ATTACHMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 80_000_000;
const MAX_IMAGE_EDGE: u32 = 1600;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptureAndroidScreenArgs {
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewScreenshotArgs {
    pub path: String,
}

/// Model-ready image attachment and a bounded transformation summary.
pub struct ViewedImage {
    /// Inline attachment sent to the model provider.
    pub attachment: crate::llm::ToolAttachment,
    /// Source and transmitted dimensions, format, and byte counts.
    pub summary: String,
}

pub fn view_screenshot(args: &ViewScreenshotArgs) -> Result<ViewedImage> {
    let path = std::path::Path::new(&args.path);
    if !path.is_absolute() || path.file_name().is_none() {
        anyhow::bail!("view_screenshot requires an absolute image path")
    }
    let bytes =
        std::fs::read(path).with_context(|| format!("cannot read image {}", path.display()))?;
    if bytes.len() > MAX_IMAGE_SOURCE_BYTES {
        anyhow::bail!("image exceeds the 32 MiB source limit")
    }
    let format = image::guess_format(&bytes).context("unsupported or malformed image")?;
    if !matches!(
        format,
        image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP
    ) {
        anyhow::bail!("view_screenshot supports PNG, JPEG, and WebP images")
    }
    let image = image::load_from_memory_with_format(&bytes, format)
        .context("cannot decode image for bounded multimodal input")?;
    let (width, height) = image.dimensions();
    if u64::from(width).saturating_mul(u64::from(height)) > MAX_IMAGE_PIXELS {
        anyhow::bail!("decoded image exceeds the 80 megapixel safety limit")
    }
    let (payload, media_type, sent_width, sent_height, transformed) = if bytes.len()
        <= MAX_IMAGE_ATTACHMENT_BYTES
        && matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg)
    {
        (
            bytes,
            match format {
                image::ImageFormat::Jpeg => "image/jpeg",
                _ => "image/png",
            },
            width,
            height,
            false,
        )
    } else {
        let resized = image.resize(MAX_IMAGE_EDGE, MAX_IMAGE_EDGE, FilterType::Triangle);
        let (sent_width, sent_height) = resized.dimensions();
        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 82)
            .encode_image(&resized.to_rgb8())
            .context("cannot encode bounded JPEG attachment")?;
        if encoded.len() > MAX_IMAGE_ATTACHMENT_BYTES {
            anyhow::bail!("resized image still exceeds the 2 MiB attachment limit")
        }
        (encoded, "image/jpeg", sent_width, sent_height, true)
    };
    let payload_len = payload.len();
    Ok(ViewedImage {
        attachment: crate::llm::ToolAttachment {
            media_type: media_type.into(),
            base64_data: base64::engine::general_purpose::STANDARD.encode(payload),
        },
        summary: format!(
            "Image attached for multimodal inspection: source={}x{}, sent={}x{}, bytes={}, transformed={transformed}",
            width, height, sent_width, sent_height, payload_len
        ),
    })
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectAndroidUiArgs {
    #[serde(default)]
    pub full: bool,
}

#[derive(Debug, Clone, Serialize)]
struct UiNode {
    package: Option<String>,
    class: Option<String>,
    resource_id: Option<String>,
    text: Option<String>,
    content_description: Option<String>,
    bounds: Option<String>,
    clickable: bool,
    focused: bool,
}

pub async fn inspect_android_ui(
    executor: &dyn CommandExecutor,
    args: &InspectAndroidUiArgs,
) -> Result<String> {
    let command = "p=/data/local/tmp/.nl2sh-ui-$$.xml; trap 'rm -f \"$p\"' EXIT HUP INT TERM; uiautomator dump \"$p\" >/dev/null && cat \"$p\"; printf '\n---NL2SH_WINDOW---\n'; dumpsys window | grep -E 'mCurrentFocus|mFocusedApp' | head -n 8; printf '\n---NL2SH_DISPLAY---\n'; wm size; wm density";
    let result = executor.execute_quiet(command, false, false).await?;
    let (xml, remainder) = result
        .stdout
        .split_once("---NL2SH_WINDOW---")
        .unwrap_or((&result.stdout, ""));
    let (window, display) = remainder
        .split_once("---NL2SH_DISPLAY---")
        .unwrap_or((remainder, ""));
    let (all_nodes, truncated) = parse_nodes(xml)?;
    let total_node_count = all_nodes.len();
    let nodes = if args.full {
        all_nodes
    } else {
        all_nodes
            .into_iter()
            .filter(UiNode::is_meaningful)
            .collect()
    };
    serde_json::to_string_pretty(&serde_json::json!({
        "status": if result.exit_code == Some(0) && !truncated { "complete" } else if nodes.is_empty() { "failed" } else { "partial" },
        "node_count": nodes.len(),
        "total_node_count": total_node_count,
        "truncated": truncated,
        "mode": if truncated { "partial" } else if args.full { "full" } else { "compact" },
        "nodes": nodes,
        "window": window.trim(),
        "display": display.trim(),
        "stderr": result.stderr,
    }))
    .context("cannot encode Android UI state")
}

impl UiNode {
    fn is_meaningful(&self) -> bool {
        self.clickable || self.focused || self.text.is_some() || self.content_description.is_some()
    }
}

pub fn capture_command(path: &str) -> Result<String> {
    let path = std::path::Path::new(path);
    if !path.is_absolute() || path.file_name().is_none() {
        anyhow::bail!("screenshot path must be an absolute file path")
    }
    let rendered = path.to_string_lossy();
    if rendered.contains(['\n', '\r', '\0']) {
        anyhow::bail!("invalid screenshot path")
    }
    Ok(format!("screencap -p {}", shell_quote(&rendered)))
}

fn parse_nodes(xml: &str) -> Result<(Vec<UiNode>, bool)> {
    let node = Regex::new(r"<node\s+([^>]+)/?>").context("invalid UI node parser")?;
    let attribute =
        Regex::new(r#"([a-zA-Z0-9_-]+)="([^"]*)""#).context("invalid UI attribute parser")?;
    let mut nodes = Vec::new();
    let mut truncated = false;
    for (index, capture) in node.captures_iter(xml).take(MAX_UI_NODES + 1).enumerate() {
        if index == MAX_UI_NODES {
            truncated = true;
            break;
        }
        let Some(attributes) = capture.get(1) else {
            continue;
        };
        let mut value = UiNode {
            package: None,
            class: None,
            resource_id: None,
            text: None,
            content_description: None,
            bounds: None,
            clickable: false,
            focused: false,
        };
        for pair in attribute.captures_iter(attributes.as_str()) {
            let Some(name) = pair.get(1).map(|item| item.as_str()) else {
                continue;
            };
            let Some(content) = pair.get(2).map(|item| item.as_str()) else {
                continue;
            };
            match name {
                "package" => value.package = non_empty(content),
                "class" => value.class = non_empty(content),
                "resource-id" => value.resource_id = non_empty(content),
                "text" => value.text = non_empty(content),
                "content-desc" => value.content_description = non_empty(content),
                "bounds" => value.bounds = non_empty(content),
                "clickable" => value.clickable = content == "true",
                "focused" => value.focused = content == "true",
                _ => {}
            }
        }
        nodes.push(value);
    }
    Ok((nodes, truncated))
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| decode_xml_entities(value))
}

fn decode_xml_entities(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find('&') {
        decoded.push_str(&remaining[..start]);
        remaining = &remaining[start..];
        // XML character references are short; bounding the scan also keeps
        // app-controlled text with many ampersands linear to decode.
        let Some(end) = remaining
            .as_bytes()
            .iter()
            .take(13)
            .position(|byte| *byte == b';')
        else {
            decoded.push('&');
            remaining = &remaining[1..];
            continue;
        };
        let entity = &remaining[1..end];
        let character = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|number| u32::from_str_radix(number, 16).ok())
                .or_else(|| {
                    entity
                        .strip_prefix('#')
                        .and_then(|number| number.parse().ok())
                })
                .and_then(char::from_u32)
                .filter(|character| {
                    !character.is_control() || matches!(character, '\t' | '\n' | '\r')
                }),
        };
        if let Some(character) = character {
            decoded.push(character);
            remaining = &remaining[end + 1..];
        } else {
            decoded.push('&');
            remaining = &remaining[1..];
        }
    }
    decoded.push_str(remaining);
    decoded
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::{capture_command, parse_nodes, view_screenshot, ViewScreenshotArgs};

    #[test]
    fn parses_bounded_accessibility_nodes() -> anyhow::Result<()> {
        let (nodes, truncated) = parse_nodes(
            r#"<hierarchy><node package="com.example.app" text="OK" resource-id="button" class="android.widget.Button" content-desc="confirm" clickable="true" focused="false" bounds="[1,2][3,4]" /></hierarchy>"#,
        )?;
        assert_eq!(nodes.len(), 1);
        assert!(!truncated);
        assert_eq!(nodes[0].text.as_deref(), Some("OK"));
        assert_eq!(nodes[0].package.as_deref(), Some("com.example.app"));
        assert!(nodes[0].clickable);
        Ok(())
    }

    #[test]
    fn reports_truncated_ui_tree() -> anyhow::Result<()> {
        let xml = "<node text=\"item\" />".repeat(super::MAX_UI_NODES + 1);
        let (nodes, truncated) = parse_nodes(&xml)?;
        assert_eq!(nodes.len(), super::MAX_UI_NODES);
        assert!(truncated);
        Ok(())
    }

    #[test]
    fn decodes_uiautomator_attribute_entities_once() -> anyhow::Result<()> {
        let (nodes, truncated) = parse_nodes(
            r#"<node text="A &amp; B &quot;中文&quot; &#x1F642;" content-desc="&lt;tag&gt; &apos; &#25991; &amp;lt; &unknown;" bounds="[1,2][3,4]" />"#,
        )?;
        assert!(!truncated);
        assert_eq!(nodes[0].text.as_deref(), Some("A & B \"中文\" 🙂"));
        assert_eq!(
            nodes[0].content_description.as_deref(),
            Some("<tag> ' 文 &lt; &unknown;")
        );
        Ok(())
    }

    #[test]
    fn screenshot_requires_an_absolute_path() {
        assert!(capture_command("screen.png").is_err());
        assert_eq!(
            capture_command("/sdcard/screen.png").ok().as_deref(),
            Some("screencap -p '/sdcard/screen.png'")
        );
    }

    #[test]
    fn accepts_small_jpeg_images_without_requiring_png() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("photo.jpg");
        image::DynamicImage::new_rgb8(16, 12).save_with_format(&path, image::ImageFormat::Jpeg)?;
        let viewed = view_screenshot(&ViewScreenshotArgs {
            path: path.to_string_lossy().into_owned(),
        })?;
        assert_eq!(viewed.attachment.media_type, "image/jpeg");
        assert!(viewed.summary.contains("source=16x12"));
        Ok(())
    }
}
