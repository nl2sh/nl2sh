//! Model and direct-runtime adapters for semantic Android UI operations.

use super::{
    automation::{self, GestureSpec, UiArgs},
    companion::{self, TextWriteMode},
};
use crate::tools::ui::domain::{self as ui, InspectAndroidUiArgs};
use crate::tools::{
    definition, parse_args, PreparedExecution, PreparedToolCall, Tool, ToolCategory, ToolContext,
    ToolMetadata, ToolOutput, ToolRisk,
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

/// Guidance shown when only the companion input method could still accept Unicode text.
const IME_HINT: &str =
    "select the nl2sh Keyboard in the system input method settings to enable IME text input";

macro_rules! meta {
    ($name:literal, $description:literal, $risk:ident) => {
        ToolMetadata {
            name: $name,
            description: $description,
            category: ToolCategory::Android,
            risk: ToolRisk::$risk,
            requires: &[],
            group: None,
            default_enabled: true,
            platform: crate::tools::ToolPlatform::AndroidShell,
            runtime: crate::tools::RuntimeRequirement::None,
            concurrency: crate::tools::ToolConcurrency::AndroidUi,
            lifetime: crate::tools::ToolLifetime::Call,
            schema: android_schema,
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
        "Append text to the focused control. Unicode requires the enabled Android Accessibility companion or the nl2sh keyboard.",
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
        "android.tap_text" => (&["text"], &["text"]),
        "android.input_text" => (&["text", "mode"], &["text"]),
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

/// Record every `#/$defs/<name>` reference inside one JSON schema fragment.
fn collect_definition_refs(value: &Value, referenced: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            if let Some(name) = text.strip_prefix("#/$defs/") {
                referenced.push(name.to_owned());
            }
        }
        Value::Array(items) => items
            .iter()
            .for_each(|item| collect_definition_refs(item, referenced)),
        Value::Object(entries) => entries
            .values()
            .for_each(|entry| collect_definition_refs(entry, referenced)),
        _ => {}
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

    async fn prepare(&self, ctx: &ToolContext<'_>, arguments: Value) -> Result<PreparedToolCall> {
        validate_argument_fields(self.0.name, &arguments)?;
        let args: UiArgs = parse_args(self.0.name, arguments)?;
        let name = self.0.name;
        let executor = ctx.executor.context("Android executor unavailable")?;
        let mut accessibility = false;
        let mut target = None;
        let mut gesture = None;
        let mut backend = TextBackend::SetText;
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
                            backend,
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
                // 字段报 isEditable 时走 ACTION_SET_TEXT；隐藏该标记但能粘贴时走剪贴板；
                // 两者都不可用时用 companion 输入法通道提交，不静默改用别的目标。
                let (preview, chosen, prepared) =
                    plan_unicode_input(executor, value, text_mode(&args)).await?;
                accessibility = true;
                backend = chosen;
                target = Some(prepared);
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
                backend,
            }),
        ))
    }
}

/// Backend chosen for one Unicode write, fixed before confirmation and re-checked after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextBackend {
    /// `ACTION_SET_TEXT` on the focused editable node.
    SetText,
    /// Clipboard plus `ACTION_PASTE` on the identity-matched node.
    Paste,
    /// Companion input method `commitText` on the focused editor.
    Ime,
}

fn text_mode(args: &UiArgs) -> TextWriteMode {
    args.mode.unwrap_or_default()
}

/// Choose the Unicode write backend and build the approval preview.
///
/// The order keeps the already verified accessibility paths first and only falls back to the
/// input method when the focused node advertises no text action at all, so a confirmed action
/// never switches backend afterwards.
async fn plan_unicode_input(
    executor: &dyn crate::shell::CommandExecutor,
    text: &str,
    mode: TextWriteMode,
) -> Result<(String, TextBackend, Value)> {
    if let Ok(focused) = companion::focused_input(executor).await {
        require_ime_for_replace(mode)?;
        return Ok((
            format!(
                "Accessibility append text {text:?} to {} {} at {}",
                node_field(&focused, "class", "unknown control"),
                node_field(&focused, "resource_id", ""),
                node_field(&focused, "bounds", "unknown bounds"),
            ),
            TextBackend::SetText,
            focused,
        ));
    }
    if let Ok(node) = companion::focused_target(executor).await {
        // An older companion omits `can_write`; only an explicit false rules the clipboard out.
        if node.get("can_write") != Some(&Value::Bool(false)) {
            require_ime_for_replace(mode)?;
            return Ok((
                format!(
                    "Accessibility paste text {text:?} into {} {} at {}",
                    node_field(&node, "class", "unknown control"),
                    node_field(&node, "resource_id", ""),
                    node_field(&node, "bounds", "unknown bounds"),
                ),
                TextBackend::Paste,
                node,
            ));
        }
        let editor = ime_editor(executor, Some(&node)).await?;
        return Ok((
            ime_preview(text, mode, Some(&node), &editor),
            TextBackend::Ime,
            ime_target(Some(&node), &editor),
        ));
    }
    let editor = ime_editor(executor, None).await?;
    Ok((
        ime_preview(text, mode, None, &editor),
        TextBackend::Ime,
        ime_target(None, &editor),
    ))
}

/// The accessibility write paths only insert text; clearing a field needs the input method.
fn require_ime_for_replace(mode: TextWriteMode) -> Result<()> {
    if mode == TextWriteMode::Replace {
        bail!("clearing a field requires the nl2sh keyboard input method; {IME_HINT}")
    }
    Ok(())
}

/// Read the focused editor identity the input method would commit into.
async fn ime_editor(
    executor: &dyn crate::shell::CommandExecutor,
    node: Option<&Value>,
) -> Result<Value> {
    let editor = companion::ime_status(executor)
        .await
        .with_context(|| IME_HINT.to_string())?;
    if editor["attached"] != true {
        bail!("nl2sh keyboard has no focused text field; {IME_HINT}")
    }
    if editor["text_field"] != true {
        bail!("focused control is not a text field")
    }
    if editor["password"] == true {
        bail!("focused control is a password field")
    }
    let package = editor["package"].as_str().unwrap_or("");
    if package.is_empty() {
        bail!("nl2sh keyboard reports no current editor")
    }
    if let Some(node) = node {
        if node["package"].as_str().unwrap_or("") != package {
            bail!("focused text field does not belong to the focused Android UI node")
        }
    }
    Ok(editor)
}

fn ime_preview(text: &str, mode: TextWriteMode, node: Option<&Value>, editor: &Value) -> String {
    let action = match mode {
        TextWriteMode::Append => "commit",
        TextWriteMode::Replace => "replace",
    };
    let detail = node.map_or_else(String::new, |node| {
        format!(
            " ({} {} at {})",
            node_field(node, "class", "unknown control"),
            node_field(node, "resource_id", ""),
            node_field(node, "bounds", "unknown bounds"),
        )
    });
    format!(
        "IME {action} text {text:?} into {} field {}{detail}",
        editor["package"].as_str().unwrap_or(""),
        companion::scalar_text(&editor["field_id"]),
    )
}

/// Approved IME identity: the editor plus the accessibility node it was confirmed against.
fn ime_target(node: Option<&Value>, editor: &Value) -> Value {
    json!({
        "package": editor["package"].clone(),
        "field_id": editor["field_id"].clone(),
        "node": node.cloned().unwrap_or(Value::Null),
    })
}

fn node_field<'a>(node: &'a Value, field: &str, fallback: &'a str) -> &'a str {
    node[field].as_str().unwrap_or(fallback)
}

struct AndroidUiAction {
    name: &'static str,
    args: UiArgs,
    preview: String,
    accessibility: bool,
    target: Option<Value>,
    gesture: Option<GestureSpec>,
    backend: TextBackend,
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
                match self.backend {
                    TextBackend::SetText => {
                        let focused = companion::focused_input(executor).await?;
                        verify_same_target(self.target.as_ref(), &focused)?;
                        serde_json::to_string(
                            &companion::input_text(executor, value, &focused).await?,
                        )?
                    }
                    TextBackend::Paste => {
                        // 目标字段不报 isEditable：粘贴进已确认的那个节点（身份由 companion 复核）。
                        let target = self
                            .target
                            .as_ref()
                            .context("Android UI paste target is missing")?;
                        serde_json::to_string(
                            &companion::paste_text(executor, value, target).await?,
                        )?
                    }
                    TextBackend::Ime => {
                        // The input method cannot read node bounds: re-read both the editor
                        // identity and the node the confirmation preview named.
                        let target = self
                            .target
                            .as_ref()
                            .context("Android input method target is missing")?;
                        let editor = companion::ime_status(executor).await?;
                        verify_ime_target(executor, target, &editor).await?;
                        serde_json::to_string(
                            &companion::ime_input_text(
                                executor,
                                value,
                                text_mode(&self.args),
                                target,
                            )
                            .await?,
                        )?
                    }
                }
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

/// Re-check the approved input method target before committing through the IME.
///
/// The input method only knows the editor, so the accessibility node the preview named is read
/// again here and must still be the same control in the same package.
async fn verify_ime_target(
    executor: &dyn crate::shell::CommandExecutor,
    expected: &Value,
    editor: &Value,
) -> Result<()> {
    let package = expected["package"].as_str().unwrap_or("");
    if package.is_empty() {
        bail!("prepared Android input method target has no package identity")
    }
    if expected["package"] != editor["package"]
        || companion::scalar_text(&expected["field_id"])
            != companion::scalar_text(&editor["field_id"])
    {
        bail!("Android input target changed after confirmation")
    }
    if let Some(node) = expected["node"].as_object() {
        let approved = Value::Object(node.clone());
        let current = companion::focused_target(executor).await?;
        verify_same_target(Some(&approved), &current)?;
    }
    Ok(())
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
    let result = executor.execute_readonly(&command).await?;
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
        builtin_tools, check_result, checked_screen_dump, plan_unicode_input,
        validate_argument_fields, verify_ime_target, verify_same_target, TextBackend,
    };
    use crate::tools::android::automation::UiArgs;
    use crate::tools::android::companion::TextWriteMode;
    use crate::{
        agent::{ConfirmationDecision, Confirmer},
        config::Config,
        security::SecurityAssessment,
        shell::{CommandExecutor, ExecutionResult},
        tools::runtime::invoke,
    };
    use anyhow::{Context, Result};
    use async_trait::async_trait;
    use base64::Engine;
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
        fn is_android(&self) -> bool {
            true
        }
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

    struct Approve;
    #[async_trait]
    impl Confirmer for Approve {
        async fn confirm(&self, _: &str, _: &SecurityAssessment) -> Result<ConfirmationDecision> {
            Ok(ConfirmationDecision::Approve)
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

    /// Companion replay: answers each `content call --method <name>` from a fixed reply table and
    /// records every dispatched command.
    struct FakeCompanion {
        replies: Vec<(String, Value)>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl FakeCompanion {
        fn with(replies: &[(&str, Value)]) -> Self {
            Self {
                replies: replies
                    .iter()
                    .map(|(method, body)| ((*method).to_owned(), body.clone()))
                    .collect(),
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn executor(&self) -> Arc<Mutex<Vec<String>>> {
            Arc::clone(&self.calls)
        }
    }

    #[async_trait]
    impl CommandExecutor for FakeCompanion {
        fn is_android(&self) -> bool {
            true
        }
        async fn execute(
            &self,
            _command: &str,
            _needs_root: bool,
            _interactive: bool,
        ) -> Result<ExecutionResult> {
            anyhow::bail!("the companion only speaks the machine protocol")
        }

        async fn execute_machine(
            &self,
            command: &str,
            _needs_root: bool,
        ) -> Result<ExecutionResult> {
            self.calls
                .lock()
                .map_err(|_| anyhow::anyhow!("companion log poisoned"))?
                .push(command.to_owned());
            let method = command
                .split("--method ")
                .nth(1)
                .and_then(|rest| rest.split(' ').next())
                .unwrap_or_default();
            let body = self
                .replies
                .iter()
                .find(|(name, _)| name == method)
                .map(|(_, body)| body.clone())
                .unwrap_or_else(|| json!({"ok": false, "error": "unsupported action"}));
            let encoded =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&body)?);
            Ok(ExecutionResult {
                stdout: format!("Result: Bundle[{{data={encoded}}}]"),
                stderr: String::new(),
                exit_code: Some(0),
                timed_out: false,
                interrupted: false,
            })
        }
    }

    fn editable_node() -> Value {
        json!({
            "ok": true, "status": "ready", "package": "com.example.app",
            "class": "android.widget.EditText", "resource_id": "com.example.app:id/query",
            "bounds": "[0,100][1080,200]", "editable": true, "can_write": true,
        })
    }

    /// A Toutiao-style field: no `isEditable`, but it still accepts a clipboard paste.
    fn pasteable_node() -> Value {
        json!({
            "ok": true, "status": "ready", "package": "com.example.app",
            "class": "com.example.app.SearchBox", "resource_id": "com.example.app:id/search",
            "bounds": "[0,100][1080,200]", "editable": false, "can_write": true,
        })
    }

    /// A field that advertises no text action at all: only the input method can write it.
    fn closed_node() -> Value {
        json!({
            "ok": true, "status": "ready", "package": "com.example.app",
            "class": "com.example.app.SearchBox", "resource_id": "com.example.app:id/search",
            "bounds": "[0,100][1080,200]", "editable": false, "can_write": false,
        })
    }

    fn ready_editor() -> Value {
        json!({
            "ok": true, "status": "ready", "attached": true, "package": "com.example.app",
            "field_id": 4242, "text_field": true, "password": false,
        })
    }

    #[tokio::test]
    async fn unicode_input_prefers_the_verified_accessibility_actions() -> Result<()> {
        let executor = FakeCompanion::with(&[
            ("focused_input", editable_node()),
            ("focused_target", editable_node()),
        ]);
        let (preview, backend, target) =
            plan_unicode_input(&executor, "中文", TextWriteMode::Append).await?;
        assert_eq!(backend, TextBackend::SetText);
        assert!(preview.contains("Accessibility append text"), "{preview}");
        assert_eq!(target["class"], "android.widget.EditText");
        Ok(())
    }

    #[tokio::test]
    async fn unicode_input_pastes_when_the_field_hides_is_editable() -> Result<()> {
        let executor = FakeCompanion::with(&[("focused_target", pasteable_node())]);
        let (preview, backend, target) =
            plan_unicode_input(&executor, "中文", TextWriteMode::Append).await?;
        assert_eq!(backend, TextBackend::Paste);
        assert!(preview.contains("Accessibility paste text"), "{preview}");
        assert_eq!(target["can_write"], true);
        Ok(())
    }

    #[tokio::test]
    async fn a_legacy_companion_without_the_capability_probe_still_pastes() -> Result<()> {
        let executor = FakeCompanion::with(&[(
            "focused_target",
            json!({
                "ok": true, "status": "ready", "package": "com.example.app",
                "class": "com.example.app.SearchBox", "resource_id": "",
                "bounds": "[0,100][1080,200]", "editable": false,
            }),
        )]);
        let (preview, backend, _) =
            plan_unicode_input(&executor, "中文", TextWriteMode::Append).await?;
        assert_eq!(backend, TextBackend::Paste);
        assert!(preview.contains("Accessibility paste text"), "{preview}");
        Ok(())
    }

    #[tokio::test]
    async fn unicode_input_commits_through_the_keyboard_when_no_node_action_exists() -> Result<()> {
        let executor = FakeCompanion::with(&[
            ("focused_target", closed_node()),
            ("ime_status", ready_editor()),
        ]);
        let (preview, backend, target) =
            plan_unicode_input(&executor, "中文", TextWriteMode::Replace).await?;
        assert_eq!(backend, TextBackend::Ime);
        assert!(preview.contains("IME replace text"), "{preview}");
        assert!(preview.contains("com.example.app field 4242"), "{preview}");
        assert_eq!(target["package"], "com.example.app");
        assert_eq!(target["field_id"], 4242);
        assert_eq!(target["node"]["bounds"], "[0,100][1080,200]");
        Ok(())
    }

    #[tokio::test]
    async fn unicode_input_rejects_an_editor_from_another_package() {
        let executor = FakeCompanion::with(&[
            ("focused_target", closed_node()),
            (
                "ime_status",
                json!({
                    "ok": true, "status": "ready", "attached": true,
                    "package": "com.example.other", "field_id": 1,
                    "text_field": true, "password": false,
                }),
            ),
        ]);
        assert!(plan_unicode_input(&executor, "中文", TextWriteMode::Append)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn unicode_input_reports_how_to_enable_the_keyboard_backend() {
        // No accessibility node and no loaded input method: the failure must name the missing
        // setup step instead of reporting a generic provider error.
        let executor = FakeCompanion::with(&[]);
        let error = plan_unicode_input(&executor, "中文", TextWriteMode::Append)
            .await
            .expect_err("missing keyboard backend must fail");
        let message = format!("{error:#}");
        assert!(message.contains("input method settings"), "{message}");
    }

    #[tokio::test]
    async fn unicode_input_refuses_a_password_field() {
        let executor = FakeCompanion::with(&[
            ("focused_target", closed_node()),
            (
                "ime_status",
                json!({
                    "ok": true, "status": "ready", "attached": true,
                    "package": "com.example.app", "field_id": 7,
                    "text_field": true, "password": true,
                }),
            ),
        ]);
        assert!(
            plan_unicode_input(&executor, "hunter2", TextWriteMode::Append)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn clearing_a_field_is_rejected_before_the_accessibility_backends() -> Result<()> {
        let executor = FakeCompanion::with(&[("focused_input", editable_node())]);
        let error = plan_unicode_input(&executor, "中文", TextWriteMode::Replace)
            .await
            .expect_err("the accessibility append path cannot clear a field");
        assert!(error.to_string().contains("nl2sh keyboard"), "{error}");
        Ok(())
    }

    #[tokio::test]
    async fn ime_target_is_rechecked_after_confirmation() -> Result<()> {
        let executor = FakeCompanion::with(&[
            ("focused_target", closed_node()),
            ("ime_status", ready_editor()),
        ]);
        let (_, _, target) = plan_unicode_input(&executor, "中文", TextWriteMode::Append).await?;
        verify_ime_target(&executor, &target, &ready_editor()).await?;

        let moved_field = json!({
            "ok": true, "status": "ready", "attached": true, "package": "com.example.app",
            "field_id": 99, "text_field": true, "password": false,
        });
        assert!(verify_ime_target(&executor, &target, &moved_field)
            .await
            .is_err());

        let mut other_package = ready_editor();
        other_package["package"] = json!("com.example.other");
        assert!(verify_ime_target(&executor, &target, &other_package)
            .await
            .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn approved_unicode_replace_reaches_the_keyboard_backend() -> Result<()> {
        let executor = FakeCompanion::with(&[
            ("focused_target", closed_node()),
            ("ime_status", ready_editor()),
            (
                "ime_input_text",
                json!({"ok": true, "status": "complete", "backend": "ime", "mode": "replace"}),
            ),
        ]);
        let calls = executor.executor();
        let result = crate::tools::runtime::invoke(
            &Config::default(),
            &executor,
            &Approve,
            "android.input_text",
            json!({"text": "中文", "mode": "replace"}),
        )
        .await?;
        assert!(result.success, "{result:?}");
        let command = calls
            .lock()
            .map_err(|_| anyhow::anyhow!("companion log poisoned"))?
            .iter()
            .find(|command| command.contains("--method ime_input_text"))
            .cloned()
            .context("no input method commit was dispatched")?;
        assert!(command.contains("mode:s:replace"), "{command}");
        assert!(command.contains("package:s:com.example.app"), "{command}");
        assert!(command.contains("field_id:s:4242"), "{command}");
        Ok(())
    }

    #[tokio::test]
    async fn ime_target_rejects_a_node_replaced_at_the_same_editor() -> Result<()> {
        let executor = FakeCompanion::with(&[
            ("focused_target", closed_node()),
            ("ime_status", ready_editor()),
        ]);
        let (_, _, target) = plan_unicode_input(&executor, "中文", TextWriteMode::Append).await?;
        let mut replaced = closed_node();
        replaced["class"] = json!("com.example.other.SearchBox");
        let executor = FakeCompanion::with(&[("focused_target", replaced)]);
        assert!(verify_ime_target(&executor, &target, &ready_editor())
            .await
            .is_err());
        Ok(())
    }

    #[test]
    fn input_text_schema_exposes_the_append_or_replace_mode() -> Result<()> {
        let definitions = builtin_tools()
            .into_iter()
            .map(|tool| tool.definition())
            .collect::<Vec<_>>();
        let parameters = definitions
            .iter()
            .find(|tool| tool.name == "android.input_text")
            .context("missing android.input_text schema")?
            .parameters
            .clone();
        assert_eq!(parameters["required"], json!(["text"]));
        assert!(parameters["properties"]["mode"].is_object());
        // The mode enum must keep its definition: a filtered tool never leaves a dangling $ref.
        let modes = parameters["properties"]["mode"]["anyOf"][0]["$ref"]
            .as_str()
            .context("mode must reference its enum definition")?;
        assert_eq!(modes, "#/$defs/TextWriteMode");
        assert_eq!(
            parameters["$defs"]["TextWriteMode"]["oneOf"][0]["const"],
            json!("append")
        );
        assert_eq!(
            parameters["$defs"]["TextWriteMode"]["oneOf"][1]["const"],
            json!("replace")
        );
        assert!(parameters["$defs"].get("ScrollDirection").is_none());

        let tap_text = definitions
            .iter()
            .find(|tool| tool.name == "android.tap_text")
            .map(|tool| tool.parameters.clone())
            .context("missing android.tap_text schema")?;
        assert!(tap_text.get("$defs").is_none());
        assert!(validate_argument_fields(
            "android.input_text",
            &json!({"text": "中文", "mode": "replace"})
        )
        .is_ok());
        // An unknown mode is refused by the argument parser, not by field presence.
        let mode = |value: &str| {
            crate::tools::parse_args::<UiArgs>(
                "android.input_text",
                json!({"text": "中文", "mode": value}),
            )
        };
        assert!(mode("erase").is_err());
        assert_eq!(
            mode("replace")?.mode,
            Some(crate::tools::android::companion::TextWriteMode::Replace)
        );
        Ok(())
    }

    #[tokio::test]
    async fn shell_ascii_input_cannot_clear_a_field() {
        let executor = FakeCompanion::with(&[]);
        let args = UiArgs {
            text: Some("plain".into()),
            mode: Some(TextWriteMode::Replace),
            ..UiArgs::default()
        };
        assert!(
            crate::tools::android::automation::prepare("android.input_text", &args, &executor)
                .await
                .is_err()
        );
    }
}

fn android_schema(metadata: &ToolMetadata) -> serde_json::Value {
    let mut tool = definition::<UiArgs>(metadata.name, metadata.description);
    let (allowed, required) = argument_fields(metadata.name);
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
    let mut referenced = Vec::new();
    if let Some(properties) = tool.parameters["properties"].as_object() {
        properties
            .values()
            .for_each(|property| collect_definition_refs(property, &mut referenced));
    }
    if let Some(definitions) = tool
        .parameters
        .get_mut("$defs")
        .and_then(Value::as_object_mut)
    {
        // Keep only the enum definitions the retained properties still reference, so a
        // filtered tool never points at a `$defs` entry that was removed with it.
        definitions.retain(|name, _| referenced.contains(&name.as_str().to_owned()));
        if definitions.is_empty() {
            tool.parameters
                .as_object_mut()
                .map(|object| object.remove("$defs"));
        }
    }
    tool.parameters
}
