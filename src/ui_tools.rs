use crate::shell::CommandExecutor;
use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};

const MAX_UI_NODES: usize = 300;

#[derive(Debug, Clone, Deserialize)]
pub struct CaptureAndroidScreenArgs {
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
struct UiNode {
    class: Option<String>,
    resource_id: Option<String>,
    text: Option<String>,
    content_description: Option<String>,
    bounds: Option<String>,
    clickable: bool,
    focused: bool,
}

pub async fn inspect_android_ui(executor: &dyn CommandExecutor) -> Result<String> {
    let command = "p=/data/local/tmp/.nl2sh-ui-$$.xml; trap 'rm -f \"$p\"' EXIT HUP INT TERM; uiautomator dump \"$p\" >/dev/null && cat \"$p\"; printf '\n---NL2SH_WINDOW---\n'; dumpsys window | grep -E 'mCurrentFocus|mFocusedApp' | head -n 8; printf '\n---NL2SH_DISPLAY---\n'; wm size; wm density";
    let result = executor.execute(command, false, false).await?;
    let (xml, remainder) = result
        .stdout
        .split_once("---NL2SH_WINDOW---")
        .unwrap_or((&result.stdout, ""));
    let (window, display) = remainder
        .split_once("---NL2SH_DISPLAY---")
        .unwrap_or((remainder, ""));
    let nodes = parse_nodes(xml)?;
    serde_json::to_string_pretty(&serde_json::json!({
        "status": if result.exit_code == Some(0) { "complete" } else if nodes.is_empty() { "failed" } else { "partial" },
        "node_count": nodes.len(),
        "nodes": nodes,
        "window": window.trim(),
        "display": display.trim(),
        "stderr": result.stderr,
    }))
    .context("cannot encode Android UI state")
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

fn parse_nodes(xml: &str) -> Result<Vec<UiNode>> {
    let node = Regex::new(r"<node\s+([^>]+)/?>").context("invalid UI node parser")?;
    let attribute =
        Regex::new(r#"([a-zA-Z0-9_-]+)="([^"]*)""#).context("invalid UI attribute parser")?;
    let mut nodes = Vec::new();
    for capture in node.captures_iter(xml).take(MAX_UI_NODES) {
        let Some(attributes) = capture.get(1) else {
            continue;
        };
        let mut value = UiNode {
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
    Ok(nodes)
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::{capture_command, parse_nodes};

    #[test]
    fn parses_bounded_accessibility_nodes() -> anyhow::Result<()> {
        let nodes = parse_nodes(
            r#"<hierarchy><node text="OK" resource-id="button" class="android.widget.Button" content-desc="confirm" clickable="true" focused="false" bounds="[1,2][3,4]" /></hierarchy>"#,
        )?;
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].text.as_deref(), Some("OK"));
        assert!(nodes[0].clickable);
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
}
