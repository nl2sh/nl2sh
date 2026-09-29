//! Android Binder adapter for the optional Accessibility companion.

use crate::shell::CommandExecutor;
use crate::tools::android::automation::GestureSpec;
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

const URI: &str = "content://com.nl2sh.bridge.ops";
const MAX_REPLY: usize = 64 * 1024;

/// How one Unicode write treats the text already in the focused control.
///
/// Also the model-visible `android.input_text` mode: clearing a field needs the companion input
/// method, because the shell and accessibility paths can only insert characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextWriteMode {
    /// Insert the text at the current cursor position.
    #[default]
    Append,
    /// Clear the control first, then write the text.
    Replace,
}

impl TextWriteMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Append => "append",
            Self::Replace => "replace",
        }
    }
}

/// Check whether the enabled companion is available to the current shell UID.
pub async fn probe(executor: &dyn CommandExecutor) -> Result<()> {
    let response = request(executor, "ping", &[]).await?;
    if response["status"] != "ready" {
        bail!("Accessibility companion is not ready")
    }
    Ok(())
}

/// Append Unicode text to the focused editable accessibility node.
pub async fn input_text(
    executor: &dyn CommandExecutor,
    text: &str,
    target: &Value,
) -> Result<Value> {
    if text.is_empty() || text.len() > 1024 || text.contains('\0') {
        bail!("invalid Android Accessibility input text")
    }
    request(
        executor,
        "input_text",
        &[
            ("text", text),
            ("package", target["package"].as_str().unwrap_or("")),
            ("class", target["class"].as_str().unwrap_or("")),
            ("resource_id", target["resource_id"].as_str().unwrap_or("")),
            ("bounds", target["bounds"].as_str().unwrap_or("")),
        ],
    )
    .await
}

/// Read the focused editable node identity before local confirmation.
pub async fn focused_input(executor: &dyn CommandExecutor) -> Result<Value> {
    request(executor, "focused_input", &[]).await
}

/// Read the input-focused node identity even when it does not report `isEditable`.
///
/// Apps such as Toutiao hide that flag on their search box; the node is still the right paste
/// target, and the write itself goes through `paste_text`'s own action checks.
pub async fn focused_target(executor: &dyn CommandExecutor) -> Result<Value> {
    request(executor, "focused_target", &[]).await
}

/// Paste Unicode text into one identity-matched node through the system clipboard.
///
/// Used when the field hides `isEditable`, where `input_text`'s `ACTION_SET_TEXT` path cannot
/// verify its target. The companion sets the clipboard, then pastes into the matched node or its
/// editable ancestor.
pub async fn paste_text(executor: &dyn CommandExecutor, text: &str, node: &Value) -> Result<Value> {
    let bounds = node["bounds"]
        .as_str()
        .context("Android UI target has no bounds")?;
    if text.is_empty() || text.len() > 4096 || text.contains('\0') || bounds.len() > 128 {
        bail!("invalid Android Accessibility paste target")
    }
    request(
        executor,
        "paste_text",
        &[
            ("text", text),
            ("package", node["package"].as_str().unwrap_or("")),
            ("bounds", bounds),
            ("class", node["class"].as_str().unwrap_or("")),
            ("resource_id", node["resource_id"].as_str().unwrap_or("")),
            ("node_text", node["text"].as_str().unwrap_or("")),
            ("text_hash", node["text_hash"].as_str().unwrap_or("")),
            (
                "description",
                node["content_description"].as_str().unwrap_or(""),
            ),
            (
                "description_hash",
                node["description_hash"].as_str().unwrap_or(""),
            ),
        ],
    )
    .await
}

/// Read the companion input method's current editor identity.
///
/// Unlike the accessibility probes this does not need the Accessibility service: the companion's
/// input method is loaded as soon as the user selects it as the on-screen keyboard.
pub async fn ime_status(executor: &dyn CommandExecutor) -> Result<Value> {
    request(executor, "ime_status", &[]).await
}

/// Commit Unicode text through the companion input method's `InputConnection`.
///
/// The text is base64 encoded because `content call` transports it as a single argv string, and
/// the editor `package`/`field_id` are re-checked inside the companion before the commit. This
/// reaches fields that report neither `isEditable` nor `ACTION_PASTE`, and never touches the
/// system clipboard the clipboard path needs.
pub async fn ime_input_text(
    executor: &dyn CommandExecutor,
    text: &str,
    mode: TextWriteMode,
    target: &Value,
) -> Result<Value> {
    let package = target["package"].as_str().unwrap_or("");
    let field_id = scalar_text(&target["field_id"]);
    if package.is_empty() || field_id.is_empty() || text.len() > 4096 || text.contains('\0') {
        bail!("invalid Android input method target")
    }
    request(
        executor,
        "ime_input_text",
        &[
            ("text_b64", &base64_text(text)),
            ("mode", mode.as_str()),
            ("package", package),
            ("field_id", &field_id),
        ],
    )
    .await
}

/// Read one scalar extra as the plain text the companion expects.
pub(crate) fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

fn base64_text(text: &str) -> String {
    URL_SAFE_NO_PAD.encode(text.as_bytes())
}

/// Click a unique current accessibility node whose approved identity still matches.
pub async fn tap_text(executor: &dyn CommandExecutor, text: &str, node: &Value) -> Result<Value> {
    let bounds = node["bounds"]
        .as_str()
        .context("Android UI target has no bounds")?;
    if text.is_empty() || text.len() > 1024 || text.contains('\0') || bounds.len() > 128 {
        bail!("invalid Android Accessibility target")
    }
    request(
        executor,
        "tap_text",
        &[
            ("text", text),
            ("package", node["package"].as_str().unwrap_or("")),
            ("bounds", bounds),
            ("class", node["class"].as_str().unwrap_or("")),
            ("resource_id", node["resource_id"].as_str().unwrap_or("")),
            ("node_text", node["text"].as_str().unwrap_or("")),
            ("text_hash", node["text_hash"].as_str().unwrap_or("")),
            (
                "description",
                node["content_description"].as_str().unwrap_or(""),
            ),
            (
                "description_hash",
                node["description_hash"].as_str().unwrap_or(""),
            ),
        ],
    )
    .await
}

/// Click a unique current accessibility node selected by its approved bounds.
pub async fn tap_node(executor: &dyn CommandExecutor, node: &Value) -> Result<Value> {
    let bounds = node["bounds"]
        .as_str()
        .context("Android UI target has no bounds")?;
    if bounds.len() > 128 {
        bail!("invalid Android Accessibility target")
    }
    request(
        executor,
        "tap_node",
        &[
            ("package", node["package"].as_str().unwrap_or("")),
            ("bounds", bounds),
            ("class", node["class"].as_str().unwrap_or("")),
            ("resource_id", node["resource_id"].as_str().unwrap_or("")),
            ("node_text", node["text"].as_str().unwrap_or("")),
            ("text_hash", node["text_hash"].as_str().unwrap_or("")),
            (
                "description",
                node["content_description"].as_str().unwrap_or(""),
            ),
            (
                "description_hash",
                node["description_hash"].as_str().unwrap_or(""),
            ),
        ],
    )
    .await
}

/// Read a bounded accessibility node tree from the companion.
pub async fn screen_dump(executor: &dyn CommandExecutor) -> Result<Value> {
    request(executor, "screen_dump", &[]).await
}

/// Dispatch one bounded Accessibility gesture and wait for its completion.
pub(crate) async fn gesture(executor: &dyn CommandExecutor, spec: GestureSpec) -> Result<Value> {
    let x = spec.x.to_string();
    let y = spec.y.to_string();
    let end_x = spec.end_x.to_string();
    let end_y = spec.end_y.to_string();
    let duration = spec.duration_ms.to_string();
    request(
        executor,
        "gesture",
        &[
            ("x", &x),
            ("y", &y),
            ("end_x", &end_x),
            ("end_y", &end_y),
            ("duration_ms", &duration),
        ],
    )
    .await
}

async fn request(
    executor: &dyn CommandExecutor,
    method: &str,
    extras: &[(&str, &str)],
) -> Result<Value> {
    let mut command = format!("content call --uri {URI} --method {method}");
    for (name, value) in extras {
        command.push_str(" --extra ");
        command.push_str(&shell_quote(&format!(
            "{name}:s:{}",
            encode_extra_value(value)
        )));
    }
    let result = executor
        .execute_machine(&command, false)
        .await
        .context("Android Accessibility provider call failed")?;
    if result.exit_code != Some(0) || result.timed_out || result.interrupted {
        bail!(
            "Android Accessibility provider is unavailable: {}",
            result.stderr
        )
    }
    if result
        .stdout
        .trim_start()
        .starts_with("usage: adb shell content")
    {
        bail!("Android Accessibility provider rejected the call arguments")
    }
    decode_reply(&result.stdout)
}

/// Percent-encode `%` and `:` inside one extra value.
///
/// `content call --extra` parses `<name>:<type>:<value>` and rejects a value that contains a
/// colon by printing its usage text **with exit code 0**; `resource_id` always contains one
/// (`package:id/name`), so every identity-bound companion action failed as an "invalid reply".
/// The companion percent-decodes these two characters before using them.
fn encode_extra_value(value: &str) -> String {
    value.replace('%', "%25").replace(':', "%3A")
}

fn decode_reply(output: &str) -> Result<Value> {
    let encoded = output
        .trim()
        .strip_prefix("Result: Bundle[{data=")
        .and_then(|value| value.strip_suffix("}]"))
        .context("Android Accessibility provider returned an invalid reply")?;
    if encoded.len() > MAX_REPLY * 2 {
        bail!("Android Accessibility provider reply exceeds size limit")
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .context("invalid Android Accessibility reply encoding")?;
    if bytes.is_empty() || bytes.len() > MAX_REPLY {
        bail!("Android Accessibility provider reply exceeds size limit")
    }
    let response: Value =
        serde_json::from_slice(&bytes).context("invalid Android Accessibility reply JSON")?;
    if response["ok"] != true {
        bail!(
            "Android Accessibility action failed: {}",
            response["error"].as_str().unwrap_or("unknown error")
        )
    }
    Ok(response)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::{
        decode_reply, encode_extra_value, ime_input_text, paste_text, scalar_text, shell_quote,
        TextWriteMode,
    };
    use crate::shell::{CommandExecutor, ExecutionResult};
    use anyhow::{bail, Result};
    use async_trait::async_trait;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use serde_json::json;

    #[test]
    fn extra_values_survive_the_colon_delimited_binding_format() -> Result<()> {
        // `content call --extra name:type:value` rejects a value containing ':' (exit code 0,
        // usage text on stdout), and resource_id always contains one.
        assert_eq!(
            encode_extra_value("com.zhihu.android:id/input_text"),
            "com.zhihu.android%3Aid/input_text"
        );
        assert!(!encode_extra_value("com.zhihu.android:id/input_text").contains(':'));
        assert_eq!(encode_extra_value("100%"), "100%25");
        assert_eq!(encode_extra_value("plain text 中文"), "plain text 中文");
        Ok(())
    }

    /// Records the machine-protocol command and answers like a ready companion.
    #[derive(Default)]
    struct RecordingExecutor {
        captured: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait]
    impl CommandExecutor for RecordingExecutor {
        async fn execute(
            &self,
            _command: &str,
            _needs_root: bool,
            _interactive: bool,
        ) -> Result<ExecutionResult> {
            bail!("the companion only speaks the machine protocol")
        }

        async fn execute_machine(
            &self,
            command: &str,
            _needs_root: bool,
        ) -> Result<ExecutionResult> {
            self.captured
                .lock()
                .map_err(|_| anyhow::anyhow!("recording lock poisoned"))?
                .push(command.to_owned());
            let reply = URL_SAFE_NO_PAD.encode(serde_json::to_vec(
                &json!({"ok": true, "status": "complete"}),
            )?);
            Ok(ExecutionResult {
                stdout: format!("Result: Bundle[{{data={reply}}}]"),
                stderr: String::new(),
                exit_code: Some(0),
                timed_out: false,
                interrupted: false,
            })
        }
    }

    #[tokio::test]
    async fn paste_request_carries_node_identity_without_bare_colons() -> Result<()> {
        let executor = RecordingExecutor::default();
        let node = json!({
            "package": "com.example.app",
            "class": "android.widget.EditText",
            "resource_id": "com.example.app:id/query",
            "bounds": "[0,0][10,10]",
            "text": "hint",
            "text_hash": "AAA",
            "content_description": "",
            "description_hash": "",
        });
        paste_text(&executor, "中文", &node).await?;
        let command = executor
            .captured
            .lock()
            .map_err(|_| anyhow::anyhow!("recording lock poisoned"))?
            .pop()
            .ok_or_else(|| anyhow::anyhow!("no command recorded"))?;
        assert!(command.contains("--method paste_text"));
        assert!(command.contains("resource_id:s:com.example.app%3Aid/query"));
        assert!(!command.contains("com.example.app:id/query"));
        Ok(())
    }

    #[tokio::test]
    async fn paste_target_without_bounds_is_rejected() {
        let executor = RecordingExecutor::default();
        assert!(
            paste_text(&executor, "中文", &json!({"package": "com.example.app"}))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn ime_commit_binds_the_editor_and_encodes_the_text() -> Result<()> {
        let executor = RecordingExecutor::default();
        let target = json!({"package": "com.example.app", "field_id": 4242});
        ime_input_text(&executor, "中文", TextWriteMode::Replace, &target).await?;
        let command = executor
            .captured
            .lock()
            .map_err(|_| anyhow::anyhow!("recording lock poisoned"))?
            .pop()
            .ok_or_else(|| anyhow::anyhow!("no command recorded"))?;
        assert!(command.contains("--method ime_input_text"));
        assert!(command.contains("mode:s:replace"));
        assert!(command.contains("package:s:com.example.app"));
        assert!(command.contains("field_id:s:4242"));
        // `content call` carries the extra as one argv string, so the text travels base64 encoded.
        let encoded = URL_SAFE_NO_PAD.encode("中文".as_bytes());
        assert!(command.contains(&format!("text_b64:s:{encoded}")));
        Ok(())
    }

    #[tokio::test]
    async fn ime_commit_without_a_bound_editor_is_rejected() {
        let executor = RecordingExecutor::default();
        for target in [
            json!({"package": "com.example.app"}),
            json!({"field_id": 1}),
            json!({"package": "", "field_id": 1}),
        ] {
            assert!(
                ime_input_text(&executor, "中文", TextWriteMode::Append, &target)
                    .await
                    .is_err()
            );
        }
    }

    #[test]
    fn editor_identity_extras_read_as_plain_text() {
        assert_eq!(scalar_text(&json!(4242)), "4242");
        assert_eq!(scalar_text(&json!("4242")), "4242");
        assert_eq!(scalar_text(&json!(null)), "");
    }

    #[test]
    fn provider_reply_decodes_bounded_success_and_rejection() -> Result<()> {
        let success =
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({"ok":true,"status":"ready"}))?);
        assert_eq!(
            decode_reply(&format!("Result: Bundle[{{data={success}}}]"))?["status"],
            "ready"
        );
        let failed =
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({"ok":false,"error":"denied"}))?);
        assert!(decode_reply(&format!("Result: Bundle[{{data={failed}}}]")).is_err());
        assert!(decode_reply("Result: null").is_err());
        assert_eq!(shell_quote("text:s:it's safe"), "'text:s:it'\\''s safe'");
        Ok(())
    }
}
