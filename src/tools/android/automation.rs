//! Semantic Android UI actions backed by the platform shell and current UI tree.

use crate::{
    shell::CommandExecutor,
    tools::{
        android::companion,
        ui::domain::{inspect_android_ui, InspectAndroidUiArgs},
    },
};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

/// Direction the screen content should move.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    Down,
    Up,
}

/// A validated one-stroke gesture in display coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GestureSpec {
    pub x: u32,
    pub y: u32,
    pub end_x: u32,
    pub end_y: u32,
    pub duration_ms: u32,
}

impl GestureSpec {
    pub fn shell_command(self) -> String {
        format!(
            "input swipe {} {} {} {} {}",
            self.x, self.y, self.end_x, self.end_y, self.duration_ms
        )
    }
}

/// Arguments for one semantic Android UI operation.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiArgs {
    /// Absolute screenshot destination.
    #[serde(default)]
    pub path: Option<String>,
    /// Package name for launch or stop.
    #[serde(default)]
    pub package: Option<String>,
    /// Visible text or content description to match.
    #[serde(default)]
    pub text: Option<String>,
    /// Exact current node bounds for a node tap.
    #[serde(default)]
    pub bounds: Option<String>,
    /// Start or tap horizontal coordinate.
    #[serde(default)]
    pub x: Option<u32>,
    /// Start or tap vertical coordinate.
    #[serde(default)]
    pub y: Option<u32>,
    /// Swipe end horizontal coordinate.
    #[serde(default)]
    pub end_x: Option<u32>,
    /// Swipe end vertical coordinate.
    #[serde(default)]
    pub end_y: Option<u32>,
    /// Swipe duration in milliseconds.
    #[serde(default)]
    pub duration_ms: Option<u32>,
    /// Content scroll direction when coordinates are omitted.
    #[serde(default)]
    pub direction: Option<ScrollDirection>,
    /// Maximum wait time in milliseconds.
    #[serde(default)]
    pub timeout_ms: Option<u32>,
}

/// Validate arguments and build a shell command or current node evidence.
pub async fn prepare(name: &str, args: &UiArgs, executor: &dyn CommandExecutor) -> Result<String> {
    match name {
        "android.launch_app" | "android.stop_app" => {
            let package = args.package.as_deref().context("package is required")?;
            validate_package(package)?;
            if name == "android.launch_app" {
                Ok(format!(
                    "am start -a android.intent.action.MAIN -c android.intent.category.LAUNCHER -p {package}"
                ))
            } else {
                Ok(format!("am force-stop {package}"))
            }
        }
        "android.press_back" => Ok("input keyevent 4".into()),
        "android.press_home" => Ok("input keyevent 3".into()),
        "android.press_enter" => Ok("input keyevent 66".into()),
        "android.input_text" => {
            let value = required_text(args)?;
            shell_input_text_command(value)
        }
        "android.tap" => {
            let (x, y) = coordinates(args)?;
            Ok(format!("input tap {x} {y}"))
        }
        "android.swipe" => Ok(gesture_spec(name, args, executor).await?.shell_command()),
        "android.scroll" => Ok(gesture_spec(name, args, executor).await?.shell_command()),
        "android.tap_text" => Ok(prepare_tap(name, args, executor).await?.0),
        "android.tap_node" => Ok(prepare_tap(name, args, executor).await?.0),
        _ => bail!("unsupported Android UI action {name}"),
    }
}

/// Resolve a swipe or scroll into one bounded gesture on the current display.
pub(crate) async fn gesture_spec(
    name: &str,
    args: &UiArgs,
    executor: &dyn CommandExecutor,
) -> Result<GestureSpec> {
    let duration_ms = args.duration_ms.unwrap_or(500).clamp(100, 10_000);
    let (x, y, end_x, end_y) = match name {
        "android.swipe" => {
            let (x, y) = coordinates(args)?;
            let end_x = args.end_x.context("end_x is required")?;
            let end_y = args.end_y.context("end_y is required")?;
            (x, y, end_x, end_y)
        }
        "android.scroll" => {
            if let (Some(x), Some(y), Some(end_x), Some(end_y)) =
                (args.x, args.y, args.end_x, args.end_y)
            {
                (x, y, end_x, end_y)
            } else {
                if args.x.is_some()
                    || args.y.is_some()
                    || args.end_x.is_some()
                    || args.end_y.is_some()
                {
                    bail!("scroll coordinates must be supplied together")
                }
                let display = executor
                    .execute_quiet("wm size", false, false)
                    .await
                    .context("cannot read Android display size")?;
                if display.exit_code != Some(0) {
                    bail!("cannot determine Android display size")
                }
                let (width, height) = parse_display_size(&display.stdout)?;
                let x = width / 2;
                let (y, end_y) = match args.direction.unwrap_or(ScrollDirection::Down) {
                    ScrollDirection::Down => (height * 3 / 4, height / 4),
                    ScrollDirection::Up => (height / 4, height * 3 / 4),
                };
                (x, y, x, end_y)
            }
        }
        _ => bail!("unsupported Android gesture {name}"),
    };
    check_coordinate(x, y)?;
    check_coordinate(end_x, end_y)?;
    Ok(GestureSpec {
        x,
        y,
        end_x,
        end_y,
        duration_ms,
    })
}

/// Resolve a semantic tap to both a shell command and the node it would affect.
pub(crate) async fn prepare_tap(
    name: &str,
    args: &UiArgs,
    executor: &dyn CommandExecutor,
) -> Result<(String, Value)> {
    let node = match name {
        "android.tap_text" => find_unique_node(executor, Some(required_text(args)?), None).await?,
        "android.tap_node" => {
            let bounds = args.bounds.as_deref().context("bounds is required")?;
            find_unique_node(executor, None, Some(bounds)).await?
        }
        _ => bail!("unsupported semantic Android tap {name}"),
    };
    let bounds = node["bounds"]
        .as_str()
        .context("matched node has no bounds")?;
    let (x, y) = bounds_center(bounds)?;
    Ok((format!("input tap {x} {y}"), node))
}

/// Return the current matching node, rejecting ambiguous matches.
pub async fn find_unique_node(
    executor: &dyn CommandExecutor,
    text: Option<&str>,
    bounds: Option<&str>,
) -> Result<Value> {
    let value = match companion::screen_dump(executor).await {
        Ok(snapshot) => snapshot,
        Err(_) => {
            let snapshot =
                inspect_android_ui(executor, &InspectAndroidUiArgs { full: true }).await?;
            serde_json::from_str(&snapshot).context("invalid Android UI snapshot")?
        }
    };
    select_unique_node(&value, text, bounds)
}

fn select_unique_node(value: &Value, text: Option<&str>, bounds: Option<&str>) -> Result<Value> {
    if value["status"] != "complete" || value["truncated"] == true || value["mode"] == "partial" {
        bail!("Android UI snapshot is incomplete; semantic target cannot be verified")
    }
    let nodes = value["nodes"]
        .as_array()
        .context("Android UI snapshot has no nodes")?;
    let text_digest = text.map(full_text_hash);
    let mut matches = nodes.iter().filter(|node| {
        let text_matches = text.is_none_or(|needle| {
            node_matches_text(node, needle, text_digest.as_deref().unwrap_or(""))
        });
        let bounds_matches =
            bounds.is_none_or(|expected| node["bounds"].as_str() == Some(expected));
        text_matches && bounds_matches
    });
    let first = matches.next().context("Android UI node not found")?;
    if matches.next().is_some() {
        bail!("Android UI node match is ambiguous")
    }
    Ok(first.clone())
}

fn node_matches_text(node: &Value, needle: &str, digest: &str) -> bool {
    [
        ("text", "text_hash"),
        ("content_description", "description_hash"),
    ]
    .iter()
    .any(|(field, hash_field)| {
        node[*field].as_str().is_some_and(|displayed| {
            if let Some(full_hash) = node[*hash_field].as_str() {
                needle.starts_with(displayed) && full_hash == digest
            } else {
                displayed == needle
            }
        })
    })
}

fn full_text_hash(value: &str) -> String {
    let mut digest = Sha256::new();
    for unit in value.encode_utf16() {
        digest.update(unit.to_be_bytes());
    }
    URL_SAFE_NO_PAD.encode(digest.finalize())
}

/// Poll the current UI tree until exact visible text appears or the timeout expires.
pub async fn wait_text(executor: &dyn CommandExecutor, args: &UiArgs) -> Result<Value> {
    let text = required_text(args)?;
    let deadline = Instant::now()
        + Duration::from_millis(args.timeout_ms.unwrap_or(5000).clamp(100, 10_000) as u64);
    loop {
        match find_unique_node(executor, Some(text), None).await {
            Ok(node) => return Ok(node),
            Err(error) if !ui_node_is_absent(&error) => return Err(error),
            Err(_) if Instant::now() >= deadline => {
                bail!("Android UI text did not appear before timeout")
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(300)).await,
        }
    }
}

fn ui_node_is_absent(error: &anyhow::Error) -> bool {
    error.to_string() == "Android UI node not found"
}

fn required_text(args: &UiArgs) -> Result<&str> {
    let text = args.text.as_deref().context("text is required")?;
    if text.is_empty() || text.len() > 1024 {
        bail!("invalid Android UI text")
    }
    Ok(text)
}

fn shell_input_text_command(value: &str) -> Result<String> {
    if !value.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
        bail!("Android shell input supports printable ASCII only; Unicode text requires an Accessibility adapter")
    }
    // Android's input command decodes %s to a space, including literal input.
    // Split a literal %s across invocations so it cannot be decoded.
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut previous_percent = false;
    for character in value.chars() {
        if previous_percent && character == 's' {
            parts.push(current);
            current = String::new();
        }
        current.push(character);
        previous_percent = character == '%';
    }
    parts.push(current);
    Ok(parts
        .into_iter()
        .map(|part| {
            let encoded = part.replace(' ', "%s");
            format!("input text '{}'", encoded.replace('\'', "'\\''"))
        })
        .collect::<Vec<_>>()
        .join(" && "))
}

fn validate_package(value: &str) -> Result<()> {
    if value.len() > 255
        || value.split('.').count() < 2
        || value.split('.').any(|part| {
            part.is_empty() || !part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
    {
        bail!("invalid Android package name")
    }
    Ok(())
}

fn coordinates(args: &UiArgs) -> Result<(u32, u32)> {
    let x = args.x.context("x is required")?;
    let y = args.y.context("y is required")?;
    check_coordinate(x, y)?;
    Ok((x, y))
}

fn check_coordinate(x: u32, y: u32) -> Result<()> {
    if x > 20_000 || y > 20_000 {
        bail!("Android coordinate exceeds limit")
    }
    Ok(())
}

fn parse_display_size(output: &str) -> Result<(u32, u32)> {
    for line in output.lines().rev() {
        let Some(size) = line.split_whitespace().last() else {
            continue;
        };
        let Some((width, height)) = size.split_once('x') else {
            continue;
        };
        let (Ok(width), Ok(height)) = (width.parse::<u32>(), height.parse::<u32>()) else {
            continue;
        };
        if width >= 100 && height >= 100 && width <= 20_000 && height <= 20_000 {
            return Ok((width, height));
        }
    }
    bail!("invalid Android display size")
}

fn bounds_center(bounds: &str) -> Result<(u32, u32)> {
    let numbers = bounds
        .trim_matches(|c| c == '[' || c == ']')
        .split(['[', ']', ','])
        .filter(|part| !part.is_empty())
        .map(str::parse::<u32>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("invalid Android node bounds")?;
    if numbers.len() != 4 || numbers[0] >= numbers[2] || numbers[1] >= numbers[3] {
        bail!("invalid Android node bounds")
    }
    check_coordinate(numbers[2], numbers[3])?;
    Ok(((numbers[0] + numbers[2]) / 2, (numbers[1] + numbers[3]) / 2))
}

#[cfg(test)]
mod tests {
    use super::{
        bounds_center, full_text_hash, parse_display_size, prepare, select_unique_node,
        shell_input_text_command, ui_node_is_absent, validate_package, UiArgs,
    };
    use crate::{config::Config, shell::ShellExecutor};
    use serde_json::json;

    #[tokio::test]
    async fn launch_app_uses_package_scoped_launcher_intent() -> anyhow::Result<()> {
        let executor = ShellExecutor::new(Config::default());
        let args = UiArgs {
            package: Some("com.example.app".into()),
            ..UiArgs::default()
        };
        let command = prepare("android.launch_app", &args, &executor).await?;
        assert_eq!(
            command,
            "am start -a android.intent.action.MAIN -c android.intent.category.LAUNCHER -p com.example.app"
        );
        Ok(())
    }
    #[test]
    fn validates_semantic_ui_targets() {
        assert_eq!(bounds_center("[42,263][1031,398]").ok(), Some((536, 330)));
        assert!(bounds_center("[0,0][0,1]").is_err());
        assert!(validate_package("com.example.app").is_ok());
        assert!(validate_package("com.example;id").is_err());
    }

    #[test]
    fn parses_android_display_override_for_semantic_scroll() {
        assert_eq!(
            parse_display_size("Physical size: 1080x2400\nOverride size: 720x1600").ok(),
            Some((720, 1600))
        );
        assert!(parse_display_size("Override size: bad;id").is_err());
    }

    #[test]
    fn shell_text_preserves_percent_s_and_rejects_unicode() {
        assert_eq!(
            shell_input_text_command("100%s safe").ok().as_deref(),
            Some("input text '100%' && input text 's%ssafe'")
        );
        assert!(shell_input_text_command("技术评论").is_err());
        assert!(shell_input_text_command("line\nnext").is_err());
    }

    #[test]
    fn partial_ui_tree_cannot_prove_unique_semantic_target() {
        let snapshot = json!({
            "status": "complete", "mode": "partial", "truncated": true,
            "nodes": [{"text": "Publish", "bounds": "[0,0][100,100]"}],
        });
        assert!(select_unique_node(&snapshot, Some("Publish"), None).is_err());
        let complete = json!({"status":"complete", "mode":"full", "nodes":[
            {"text":"Publish", "bounds":"[0,0][100,100]"},
        ]});
        assert!(select_unique_node(&complete, Some("Publish"), None).is_ok());
    }

    #[test]
    fn long_text_matches_only_its_complete_accessibility_value() -> anyhow::Result<()> {
        let full = format!("{}B", "A".repeat(256));
        let changed = format!("{}C", "A".repeat(256));
        let snapshot = json!({"status":"complete", "mode":"full", "nodes":[{
            "text":"A".repeat(256), "text_hash":full_text_hash(&full),
            "bounds":"[0,0][100,100]",
        }]});
        assert!(select_unique_node(&snapshot, Some(&full), None).is_ok());
        assert!(select_unique_node(&snapshot, Some(&"A".repeat(256)), None).is_err());
        assert!(select_unique_node(&snapshot, Some(&changed), None).is_err());
        Ok(())
    }

    #[test]
    fn wait_retries_only_when_node_is_absent() -> anyhow::Result<()> {
        let absent = json!({"status":"complete", "nodes":[]});
        let failed = json!({"status":"failed", "nodes":[]});
        assert!(ui_node_is_absent(
            &select_unique_node(&absent, Some("OK"), None).unwrap_err()
        ));
        assert!(!ui_node_is_absent(
            &select_unique_node(&failed, Some("OK"), None).unwrap_err()
        ));
        Ok(())
    }
}
