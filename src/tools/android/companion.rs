//! Android Binder adapter for the optional Accessibility companion.

use crate::shell::CommandExecutor;
use crate::tools::android::automation::GestureSpec;
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;

const URI: &str = "content://com.nl2sh.bridge.ops";
const MAX_REPLY: usize = 64 * 1024;

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
    use super::{decode_reply, encode_extra_value, shell_quote};
    use anyhow::Result;
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
