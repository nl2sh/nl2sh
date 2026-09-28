//! Model and direct-runtime adapters for semantic Android UI operations.

use super::{
    automation::{self, GestureSpec, UiArgs},
    companion,
};
use crate::tools::ui::domain::{self as ui, InspectAndroidUiArgs};
use crate::tools::{
    definition, parse_args, PreparedExecution, PreparedToolCall, Tool, ToolCategory, ToolContext,
    ToolMetadata, ToolOutput, ToolRisk,
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

macro_rules! meta {
    ($name:literal, $description:literal, $risk:ident) => {
        ToolMetadata {
            name: $name,
            description: $description,
            category: ToolCategory::Android,
            risk: ToolRisk::$risk,
            requires: &[],
            parallel_safe: false,
        }
    };
}

static METADATA: &[ToolMetadata] = &[
    meta!(
        "android.launch_app",
        "Launch a validated Android package.",
        Mutating
    ),
    meta!(
        "android.stop_app",
        "Force-stop a validated Android package.",
        Mutating
    ),
    meta!(
        "android.screen_dump",
        "Read the current Android UI hierarchy.",
        ReadOnly
    ),
    meta!(
        "android.screenshot",
        "Capture and return a bounded display image; optionally save to an absolute PNG path after confirmation.",
        ReadOnly
    ),
    meta!(
        "android.read_screen",
        "Capture and return a bounded display image without retaining a file.",
        ReadOnly
    ),
    meta!(
        "android.find_node",
        "Find one current UI node by exact text or bounds.",
        ReadOnly
    ),
    meta!(
        "android.wait_text",
        "Wait for exact visible UI text, at most ten seconds.",
        ReadOnly
    ),
    meta!(
        "android.tap",
        "Tap a coordinate on the current Android display.",
        Dangerous
    ),
    meta!(
        "android.tap_text",
        "Tap a unique visible node with exact text.",
        Dangerous
    ),
    meta!(
        "android.tap_node",
        "Tap a unique node with exact current bounds.",
        Dangerous
    ),
    meta!(
        "android.input_text",
        "Append text to the focused control. Unicode requires the enabled Android Accessibility companion.",
        Dangerous
    ),
    meta!(
        "android.swipe",
        "Swipe between Android display coordinates.",
        Dangerous
    ),
    meta!(
        "android.scroll",
        "Scroll by swiping between Android display coordinates.",
        Dangerous
    ),
    meta!("android.press_back", "Press Android Back.", Dangerous),
    meta!("android.press_home", "Press Android Home.", Dangerous),
    meta!("android.press_enter", "Press Android Enter.", Dangerous),
];

pub(crate) fn builtin_tools() -> Vec<Box<dyn Tool>> {
    METADATA
        .iter()
        .map(|metadata| Box::new(AndroidUiTool(metadata)) as Box<dyn Tool>)
        .collect()
}

struct AndroidUiTool(&'static ToolMetadata);

fn argument_fields(name: &str) -> (&'static [&'static str], &'static [&'static str]) {
    match name {
        "android.launch_app" | "android.stop_app" => (&["package"], &["package"]),
        "android.screen_dump"
        | "android.read_screen"
        | "android.press_back"
        | "android.press_home"
        | "android.press_enter" => (&[], &[]),
        "android.screenshot" => (&["path"], &[]),
        "android.find_node" => (&["text", "bounds"], &[]),
        "android.wait_text" => (&["text", "timeout_ms"], &["text"]),
        "android.tap" => (&["x", "y"], &["x", "y"]),
        "android.tap_text" | "android.input_text" => (&["text"], &["text"]),
        "android.tap_node" => (&["bounds"], &["bounds"]),
        "android.swipe" => (
            &["x", "y", "end_x", "end_y", "duration_ms"],
            &["x", "y", "end_x", "end_y"],
        ),
        "android.scroll" => (
            &["x", "y", "end_x", "end_y", "duration_ms", "direction"],
            &[],
        ),
        _ => (&[], &[]),
    }
}

fn validate_argument_fields(name: &str, arguments: &Value) -> Result<()> {
    let (allowed, required) = argument_fields(name);
    let fields = arguments
        .as_object()
        .with_context(|| format!("{name} arguments must be an object"))?;
    for field in fields.keys() {
        if !allowed.contains(&field.as_str()) {
            bail!("{name} does not accept {field}")
        }
    }
    for field in required {
        if fields.get(*field).is_none_or(Value::is_null) {
            bail!("{field} is required for {name}")
        }
    }
    Ok(())
}

#[async_trait]
impl Tool for AndroidUiTool {
    fn metadata(&self) -> &'static ToolMetadata {
        self.0
    }

    fn definition(&self) -> crate::llm::ToolDefinition {
        let mut tool = definition::<UiArgs>(self.0.name, self.0.description);
        let (allowed, required) = argument_fields(self.0.name);
        if let Some(properties) = tool.parameters["properties"].as_object_mut() {
            properties.retain(|field, _| allowed.contains(&field.as_str()));
            for field in required {
                if let Some(property) = properties.get_mut(*field).and_then(Value::as_object_mut) {
                    property.remove("default");
                    let type_name = property
                        .get("type")
                        .and_then(Value::as_array)
                        .and_then(|types| types.iter().find(|value| *value != "null"))
                        .cloned();
                    if let Some(type_name) = type_name {
                        property.insert("type".into(), type_name);
                    }
                }
            }
        }
        tool.parameters["required"] = json!(required);
        if !allowed.contains(&"direction") {
            tool.parameters
                .as_object_mut()
                .map(|object| object.remove("$defs"));
        }
        tool
    }

    async fn prepare(&self, ctx: &ToolContext<'_>, arguments: Value) -> Result<PreparedToolCall> {
        validate_argument_fields(self.0.name, &arguments)?;
        let args: UiArgs = parse_args(self.0.name, arguments)?;
        let name = self.0.name;
        let executor = ctx.executor.context("Android executor unavailable")?;
        let mut accessibility = false;
        let mut target = None;
        let mut gesture = None;
        let preview = match name {
            "android.screen_dump" | "android.find_node" | "android.wait_text" => String::new(),
            "android.screenshot" => {
                if let Some(path) = args.path.as_deref() {
                    let preview = ui::capture_command(path)?;
                    return Ok(PreparedToolCall::operation_with_risk(
                        preview.clone(),
                        Box::new(AndroidUiAction {
                            name,
                            args,
                            preview,
                            accessibility: false,
                            target: None,
                            gesture: None,
                        }),
                        ToolRisk::Mutating,
                    ));
                }
                String::new()
            }
            "android.read_screen" => {
                if args.path.is_some() {
                    bail!("android.read_screen does not accept a path")
                }
                String::new()
            }
            "android.input_text" if args.text.as_deref().is_some_and(|value| !value.is_ascii()) => {
                let value = args.text.as_deref().context("text is required")?;
                if value.is_empty() || value.len() > 1024 {
                    bail!("invalid Android UI text")
                }
                let focused = companion::focused_input(executor).await?;
                let preview = format!(
                    "Accessibility append text {value:?} to {} {} at {}",
                    focused["class"].as_str().unwrap_or("unknown control"),
                    focused["resource_id"].as_str().unwrap_or(""),
                    focused["bounds"].as_str().unwrap_or("unknown bounds"),
                );
                accessibility = true;
                target = Some(focused);
                preview
            }
            "android.tap_text" if companion::probe(executor).await.is_ok() => {
                let value = args.text.as_deref().context("text is required")?;
                let node = automation::find_unique_node(executor, Some(value), None).await?;
                let bounds = node["bounds"]
                    .as_str()
                    .context("matched node has no bounds")?;
                let preview = format!("Accessibility tap text {value:?} at {bounds}");
                accessibility = true;
                target = Some(node);
                preview
            }
            "android.tap_node" if companion::probe(executor).await.is_ok() => {
                let bounds = args.bounds.as_deref().context("bounds is required")?;
                let node = automation::find_unique_node(executor, None, Some(bounds)).await?;
                accessibility = true;
                target = Some(node);
                format!("Accessibility tap node at {bounds}")
            }
            "android.tap_text" | "android.tap_node" => {
                let (command, node) = automation::prepare_tap(name, &args, executor).await?;
                target = Some(node);
                command
            }
            "android.swipe" | "android.scroll" => {
                let spec = automation::gesture_spec(name, &args, executor).await?;
                gesture = Some(spec);
                if companion::probe(executor).await.is_ok() {
                    accessibility = true;
                    format!(
                        "Accessibility gesture from ({}, {}) to ({}, {}) in {} ms",
                        spec.x, spec.y, spec.end_x, spec.end_y, spec.duration_ms
                    )
                } else {
                    spec.shell_command()
                }
            }
            _ => automation::prepare(name, &args, executor).await?,
        };
        Ok(PreparedToolCall::operation(
            preview.clone(),
            Box::new(AndroidUiAction {
                name,
                args,
                preview,
                accessibility,
                target,
                gesture,
            }),
        ))
    }
}

struct AndroidUiAction {
    name: &'static str,
    args: UiArgs,
    preview: String,
    accessibility: bool,
    target: Option<Value>,
    gesture: Option<GestureSpec>,
}

#[async_trait]
impl PreparedExecution for AndroidUiAction {
    async fn execute(self: Box<Self>, ctx: &mut ToolContext<'_>) -> Result<ToolOutput> {
        let executor = ctx.executor.context("Android executor unavailable")?;
        let content = match self.name {
            "android.screen_dump" => {
                let snapshot = match companion::screen_dump(executor).await {
                    Ok(snapshot) => snapshot,
                    Err(_) => {
                        let output =
                            ui::inspect_android_ui(executor, &InspectAndroidUiArgs { full: true })
                                .await?;
                        serde_json::from_str(&output).context("invalid Android UI snapshot")?
                    }
                };
                checked_screen_dump(&snapshot)?
            }
            "android.find_node" => {
                let text = self.args.text.as_deref();
                let bounds = self.args.bounds.as_deref();
                if text.is_none() && bounds.is_none() {
                    bail!("text or bounds is required")
                }
                serde_json::to_string(&automation::find_unique_node(executor, text, bounds).await?)?
            }
            "android.wait_text" => {
                serde_json::to_string(&automation::wait_text(executor, &self.args).await?)?
            }
            "android.screenshot" | "android.read_screen" => {
                if let Some(path) = self.args.path.as_deref() {
                    let result = executor.execute(&self.preview, false, false).await?;
                    check_result(&result)?;
                    format!("Screenshot saved to {path}")
                } else {
                    return capture_screen_attachment(executor).await;
                }
            }
            "android.input_text" if self.accessibility => {
                let value = self.args.text.as_deref().context("text is required")?;
                let focused = companion::focused_input(executor).await?;
                verify_same_target(self.target.as_ref(), &focused)?;
                serde_json::to_string(&companion::input_text(executor, value, &focused).await?)?
            }
            "android.tap_text" if self.accessibility => {
                let value = self.args.text.as_deref().context("text is required")?;
                let node = automation::find_unique_node(executor, Some(value), None).await?;
                let bounds = node["bounds"]
                    .as_str()
                    .context("matched node has no bounds")?;
                let current_preview = format!("Accessibility tap text {value:?} at {bounds}");
                if current_preview != self.preview {
                    bail!("Android UI target changed after confirmation")
                }
                verify_same_target(self.target.as_ref(), &node)?;
                serde_json::to_string(&companion::tap_text(executor, value, &node).await?)?
            }
            "android.tap_node" if self.accessibility => {
                let bounds = self.args.bounds.as_deref().context("bounds is required")?;
                let node = automation::find_unique_node(executor, None, Some(bounds)).await?;
                verify_same_target(self.target.as_ref(), &node)?;
                serde_json::to_string(&companion::tap_node(executor, &node).await?)?
            }
            "android.swipe" | "android.scroll" if self.accessibility => {
                let current = automation::gesture_spec(self.name, &self.args, executor).await?;
                if self.gesture != Some(current) {
                    bail!("Android display or gesture changed after confirmation")
                }
                serde_json::to_string(&companion::gesture(executor, current).await?)?
            }
            _ => {
                // Re-read semantic targets after confirmation. Never execute if the target moved.
                let command = if matches!(self.name, "android.tap_text" | "android.tap_node") {
                    let (command, node) =
                        automation::prepare_tap(self.name, &self.args, executor).await?;
                    verify_same_target(self.target.as_ref(), &node)?;
                    command
                } else {
                    automation::prepare(self.name, &self.args, executor).await?
                };
                if command != self.preview {
                    bail!("Android UI target changed after confirmation")
                }
                let result = executor.execute(&command, false, false).await?;
                check_result(&result)?;
                serde_json::to_string(
                    &serde_json::json!({"status":"complete", "command": command, "stdout": result.stdout, "stderr": result.stderr}),
                )?
            }
        };
        Ok(ToolOutput::success(content))
    }
}

fn checked_screen_dump(snapshot: &Value) -> Result<String> {
    if snapshot["status"] == "failed" {
        let detail = snapshot["stderr"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("UI hierarchy unavailable");
        bail!(
            "Android UI snapshot failed: {}",
            crate::limits::truncate_text(detail, 256)
        )
    }
    serde_json::to_string(snapshot).context("cannot encode Android UI snapshot")
}

fn verify_same_target(expected: Option<&Value>, actual: &Value) -> Result<()> {
    let expected = expected.context("prepared Android UI target is missing")?;
    if expected["package"]
        .as_str()
        .is_none_or(|package| package.is_empty())
    {
        bail!("prepared Android UI target has no package identity")
    }
    for field in [
        "package",
        "class",
        "resource_id",
        "text",
        "content_description",
        "text_hash",
        "description_hash",
        "bounds",
    ] {
        if expected[field] != actual[field] {
            bail!("Android UI target changed after confirmation")
        }
    }
    Ok(())
}

fn check_result(result: &crate::shell::ExecutionResult) -> Result<()> {
    if result.exit_code != Some(0) || result.timed_out || result.interrupted {
        bail!(
            "Android UI action failed: exit={:?} stderr={}",
            result.exit_code,
            result.stderr
        )
    }
    if result
        .stdout
        .lines()
        .any(|line| line.trim_start().starts_with("Error:"))
    {
        bail!(
            "Android UI action failed: {}",
            crate::limits::truncate_text(&result.stdout, 512)
        )
    }
    Ok(())
}

async fn capture_screen_attachment(
    executor: &dyn crate::shell::CommandExecutor,
) -> Result<ToolOutput> {
    let base = screenshot_temp_base()?;
    let directory = tempfile::Builder::new()
        .prefix(".nl2sh-screen-")
        .tempdir_in(base)
        .context("cannot create private screenshot directory")?;
    let path = directory.path().join("screen.png");
    let path_text = path.to_string_lossy().into_owned();
    let command = ui::capture_command(&path_text)?;
    let result = executor.execute_quiet(&command, false, false).await?;
    check_result(&result)?;
    let viewed = tokio::task::spawn_blocking(move || {
        ui::view_screenshot(&ui::ViewScreenshotArgs { path: path_text })
    })
    .await
    .context("screenshot processing worker failed")??;
    let mut output = ToolOutput::success(viewed.summary);
    output.attachments.push(viewed.attachment);
    Ok(output)
}

fn screenshot_temp_base() -> Result<std::path::PathBuf> {
    #[cfg(target_os = "android")]
    {
        if crate::runtime::is_termux() {
            return std::env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(std::path::PathBuf::from)
                .context("Termux HOME is unavailable for screenshot capture");
        }
        Ok(std::path::PathBuf::from("/data/local/tmp"))
    }
    #[cfg(not(target_os = "android"))]
    {
        std::env::current_dir().context("cannot determine screenshot directory")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        builtin_tools, check_result, checked_screen_dump, validate_argument_fields,
        verify_same_target,
    };
    use crate::{
        agent::{ConfirmationDecision, Confirmer},
        config::Config,
        security::SecurityAssessment,
        shell::{CommandExecutor, ExecutionResult},
        tools::runtime::invoke,
    };
    use anyhow::{Context, Result};
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    #[test]
    fn android_shell_error_in_stdout_is_failure_even_with_zero_exit() {
        let result = ExecutionResult {
            stdout:
                "Starting: Intent {...}\nError: Activity not started, unable to resolve Intent\n"
                    .into(),
            stderr: String::new(),
            exit_code: Some(0),
            timed_out: false,
            interrupted: false,
        };
        assert!(check_result(&result).is_err());
    }

    #[test]
    fn android_tool_schemas_expose_only_relevant_arguments() -> Result<()> {
        let definitions = builtin_tools()
            .into_iter()
            .map(|tool| tool.definition())
            .collect::<Vec<_>>();
        let find = |name: &str| {
            definitions
                .iter()
                .find(|tool| tool.name == name)
                .map(|tool| &tool.parameters)
                .with_context(|| format!("missing {name} schema"))
        };
        assert_eq!(find("android.press_home")?["properties"], json!({}));
        assert_eq!(find("android.tap_text")?["required"], json!(["text"]));
        assert_eq!(
            find("android.tap_text")?["properties"]["text"]["type"],
            "string"
        );
        assert_eq!(
            find("android.tap_text")?["properties"]
                .as_object()
                .map(|value| value.len()),
            Some(1)
        );
        assert_eq!(
            find("android.swipe")?["required"],
            json!(["x", "y", "end_x", "end_y"])
        );
        assert!(find("android.scroll")?["properties"]
            .get("direction")
            .is_some());
        assert!(find("android.screenshot")?["properties"]
            .get("path")
            .is_some());
        assert!(find("android.read_screen")?["properties"]
            .get("path")
            .is_none());
        Ok(())
    }

    #[test]
    fn android_tool_rejects_irrelevant_and_missing_arguments() {
        assert!(
            validate_argument_fields("android.press_home", &json!({"text":"ignored"})).is_err()
        );
        assert!(validate_argument_fields("android.tap_text", &json!({})).is_err());
        assert!(validate_argument_fields("android.tap_text", &json!({"text":null})).is_err());
        assert!(validate_argument_fields("android.tap_text", &json!({"text":"Search"})).is_ok());
        assert!(validate_argument_fields("android.scroll", &json!({"direction":"down"})).is_ok());
    }

    #[test]
    fn semantic_tap_rejects_replacement_at_same_bounds() {
        let approved = json!({
            "package": "com.example.article", "class": "android.widget.Button", "resource_id": "app:id/comment",
            "text": "Comment", "content_description": null,
            "bounds": "[20,40][200,100]",
        });
        let replacement = json!({
            "package": "com.example.article", "class": "android.widget.Button", "resource_id": "app:id/publish",
            "text": "Publish", "content_description": null,
            "bounds": "[20,40][200,100]",
        });
        assert!(verify_same_target(Some(&approved), &approved).is_ok());
        assert!(verify_same_target(Some(&approved), &replacement).is_err());
    }

    #[test]
    fn semantic_target_rejects_same_control_in_another_package() {
        let approved = json!({
            "package": "com.example.article", "class": "android.widget.Button",
            "resource_id": null, "text": "OK", "content_description": null,
            "bounds": "[20,40][200,100]",
        });
        let mut replacement = approved.clone();
        replacement["package"] = json!("com.example.other");
        assert!(verify_same_target(Some(&approved), &replacement).is_err());
        replacement["package"] = Value::Null;
        assert!(verify_same_target(Some(&replacement), &approved).is_err());
    }

    #[test]
    fn semantic_tap_rejects_changed_full_text_with_same_visible_prefix() {
        let prefix = "A".repeat(256);
        let approved = json!({
            "package": "com.example.article", "class": "android.widget.TextView", "resource_id": "app:id/article",
            "text": prefix, "text_hash": "approved-full-text-digest",
            "content_description": null, "description_hash": "empty-digest",
            "bounds": "[20,40][200,100]",
        });
        let changed = json!({
            "package": "com.example.article", "class": "android.widget.TextView", "resource_id": "app:id/article",
            "text": prefix, "text_hash": "changed-full-text-digest",
            "content_description": null, "description_hash": "empty-digest",
            "bounds": "[20,40][200,100]",
        });
        assert!(verify_same_target(Some(&approved), &approved).is_ok());
        assert!(verify_same_target(Some(&approved), &changed).is_err());
    }

    #[test]
    fn failed_ui_snapshot_is_not_reported_as_success() {
        let failed = json!({"status":"failed", "nodes":[], "stderr":"permission denied"});
        assert!(checked_screen_dump(&failed).is_err());
        let partial = json!({"status":"partial", "nodes":[], "truncated":true});
        assert!(checked_screen_dump(&partial).is_ok());
    }

    #[derive(Default)]
    struct FakeScreen {
        captured_path: Arc<Mutex<Option<PathBuf>>>,
    }

    #[async_trait]
    impl CommandExecutor for FakeScreen {
        async fn execute(&self, command: &str, _: bool, _: bool) -> Result<ExecutionResult> {
            let words = shell_words::split(command)?;
            if words.len() != 3 || words[0] != "screencap" || words[1] != "-p" {
                anyhow::bail!("unexpected screenshot command")
            }
            let path = PathBuf::from(&words[2]);
            image::RgbImage::new(1, 1).save(&path)?;
            *self
                .captured_path
                .lock()
                .map_err(|_| anyhow::anyhow!("screen lock poisoned"))? = Some(path);
            Ok(ExecutionResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: Some(0),
                timed_out: false,
                interrupted: false,
            })
        }
    }

    struct Reject;
    #[async_trait]
    impl Confirmer for Reject {
        async fn confirm(&self, _: &str, _: &SecurityAssessment) -> Result<ConfirmationDecision> {
            Ok(ConfirmationDecision::Reject)
        }
    }

    #[tokio::test]
    async fn screenshot_returns_image_and_removes_temporary_file() -> Result<()> {
        let config = Config::default();
        let executor = FakeScreen::default();
        let result = invoke(&config, &executor, &Reject, "android.screenshot", json!({})).await?;
        assert!(result.success);
        assert_eq!(result.attachments.len(), 1);
        assert_eq!(result.attachments[0].media_type, "image/png");
        let path = executor
            .captured_path
            .lock()
            .map_err(|_| anyhow::anyhow!("screen lock poisoned"))?
            .clone()
            .context("screenshot command was not called")?;
        assert!(!path.exists());
        Ok(())
    }

    #[tokio::test]
    async fn persistent_screenshot_needs_confirmation() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("saved.png");
        let config = Config::default();
        let executor = FakeScreen::default();
        let result = invoke(
            &config,
            &executor,
            &Reject,
            "android.screenshot",
            json!({"path": target.to_string_lossy()}),
        )
        .await?;
        assert!(!result.success);
        assert!(!target.exists());
        assert!(executor
            .captured_path
            .lock()
            .map_err(|_| anyhow::anyhow!("screen lock poisoned"))?
            .is_none());
        Ok(())
    }
}
