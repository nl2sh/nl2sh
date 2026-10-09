//! Typed access to the active nl2sh configuration, with approval-bound writes.

use super::{
    PreparedExecution, PreparedToolCall, ToolCategory, ToolContext, ToolMetadata, ToolOutput,
    ToolRisk,
};
use crate::config::{parse_unvalidated, Config};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use toml_edit::{DocumentMut, Item};

const MAX_CONFIG_BYTES: usize = 256 * 1024;
const META: ToolMetadata = ToolMetadata {
    name: "nl2sh_config",
    description: "Manage nl2sh's active configuration with list/get/set/reset. Read persisted settings and the current task snapshot, defaults and write policy before changing a key. Use native JSON values; dotted keys are supported for tool_groups and tool_overrides. reset removes a persisted override. All writes require approval; security, privilege, tool availability, network and audit changes require strong approval. Credentials are redacted and can only be edited by the user in settings. Writes preserve other fields and comments and do not hot-reload the current task. New Web/protocol tasks reload; restart TUI to apply. Prefer this tool over editing config with shell or apply_patch.",
    category: ToolCategory::Configuration,
    risk: ToolRisk::ReadOnly,
    requires: &[],
    group: None, default_enabled: true,
            platform: crate::tools::ToolPlatform::Any, runtime: crate::tools::RuntimeRequirement::None,
            concurrency: crate::tools::ToolConcurrency::Sequential, lifetime: crate::tools::ToolLifetime::Call,
            schema: crate::tools::descriptor_schema::<ConfigArgs>,
};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Action {
    List,
    Get,
    Set,
    Reset,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ConfigArgs {
    /// list/get are read-only; set/reset require confirmation.
    action: Action,
    /// Exact configuration key; required except for list. Supports tool_groups.jadx and tool_overrides.tailcat_check.
    #[serde(default)]
    key: Option<String>,
    /// Native JSON value for set (boolean, number, string, array or object). Use reset to remove an override; null is not a set value.
    #[serde(default)]
    value: Option<Value>,
}

define_tool!(ConfigTool, ConfigArgs, META, prepare);

fn credentials() -> &'static [&'static str] {
    &[
        "api_key",
        "ima_client_id",
        "ima_api_key",
        "jev_api_key",
        "proxy_username",
        "proxy_password",
    ]
}

fn write_risk(key: &str) -> ToolRisk {
    match key {
        "model"
        | "model_context_window"
        | "model_max_output_tokens"
        | "api_type"
        | "ima_enabled"
        | "ima_knowledge_base_id"
        | "jev_model"
        | "skipped_update_version"
        | "max_context_turns"
        | "max_agent_steps"
        | "agent_mode"
        | "max_tool_calls"
        | "max_task_execution_time_secs"
        | "replan_after_stalled_steps"
        | "abort_after_stalled_steps"
        | "max_same_action_retries"
        | "hard_max_agent_steps"
        | "llm_retry_count"
        | "llm_retry_base_delay_ms"
        | "llm_request_timeout_secs"
        | "execute_timeout_secs"
        | "interactive_execute_timeout_secs"
        | "enable_pty"
        | "ascii_symbols"
        | "show_buddha_ascii_art"
        | "show_train_ascii_art"
        | "ui_language"
        | "ui_live_output_max_bytes"
        | "tool_output_max_bytes"
        | "model_tool_output_max_bytes" => ToolRisk::Mutating,
        // New fields default to strong approval until their risk is reviewed locally.
        _ => ToolRisk::Dangerous,
    }
}

fn config_json(config: &Config) -> Result<Value> {
    let mut value = serde_json::to_value(config)?;
    // `auto` is omitted by Config's TOML serializer but is still a supported key.
    value["api_type"] = serde_json::to_value(config.api_type)?;
    Ok(value)
}

fn check_key(key: &str, defaults: &Value) -> Result<()> {
    if let Some((group, leaf)) = key.split_once('.') {
        match group {
            "tool_groups"
                if super::optional_groups()
                    .iter()
                    .any(|known| known.id() == leaf) =>
            {
                return Ok(())
            }
            "tool_overrides" if super::optional_tool_names().contains(&leaf) => return Ok(()),
            _ => bail!("unknown configuration key; use nl2sh_config list"),
        }
    }
    if !defaults
        .as_object()
        .is_some_and(|fields| fields.contains_key(key))
    {
        bail!("unknown configuration key; use nl2sh_config list")
    }
    Ok(())
}

fn field(value: &Value, key: &str) -> Value {
    if let Some((group, leaf)) = key.split_once('.') {
        value[group].get(leaf).cloned().unwrap_or(Value::Null)
    } else {
        value.get(key).cloned().unwrap_or(Value::Null)
    }
}

fn read_snapshot(path: &Path) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            bail!("configuration must be a regular file, not a symlink or directory")
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("cannot inspect active configuration"),
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .context("cannot read active configuration")?;
    if !file.metadata()?.is_file() {
        bail!("configuration must be a regular file")
    }
    let mut bytes = Vec::new();
    file.take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CONFIG_BYTES {
        bail!("configuration exceeds 256 KiB")
    }
    Ok(Some(
        String::from_utf8(bytes).context("configuration is not UTF-8")?,
    ))
}

fn parse(text: &str, path: &Path) -> Result<Config> {
    // TOML/type errors can quote credentials or entire source lines. Keep these out of tool results.
    parse_unvalidated(text, path)
        .map_err(|_| anyhow::anyhow!("invalid configuration; repair it in user settings"))
}

fn redact(value: &mut Value, secrets: &[String]) {
    match value {
        Value::String(text) => {
            for secret in secrets {
                if !secret.is_empty() {
                    *text = text.replace(secret, "[REDACTED]");
                }
            }
            if let Ok(mut url) = url::Url::parse(text) {
                if matches!(url.scheme(), "http" | "https") {
                    if !url.username().is_empty() {
                        let _ = url.set_username("REDACTED");
                    }
                    if url.password().is_some() {
                        let _ = url.set_password(Some("REDACTED"));
                    }
                    if url.query().is_some() {
                        url.set_query(Some("REDACTED"));
                    }
                    if url.fragment().is_some() {
                        url.set_fragment(Some("REDACTED"));
                    }
                    *text = url.to_string();
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact(item, secrets);
            }
        }
        Value::Object(fields) => {
            for item in fields.values_mut() {
                redact(item, secrets);
            }
        }
        _ => {}
    }
}

fn describe(
    key: &str,
    persisted: &Value,
    resolved: &Value,
    active: &Value,
    defaults: &Value,
) -> Value {
    let secret = credentials().contains(&key);
    if secret {
        return json!({"key":key, "sensitive":true, "writable":false,
            "persisted_configured":field(persisted,key).as_str().is_some_and(|s| !s.is_empty()),
            "current_task_configured":field(active,key).as_str().is_some_and(|s| !s.is_empty())});
    }
    let mut result = json!({"key":key,"persisted":field(persisted,key),"resolved":field(resolved,key),
        "current_task":field(active,key),"default":field(defaults,key),"writable":true,
        "write_risk": if write_risk(key)==ToolRisk::Dangerous {"dangerous"} else {"mutating"}});
    let choices: &[&str] = match key {
        "api_type" => &["auto", "responses", "chat_completions"],
        "agent_mode" => &["fast", "normal", "deep"],
        "security_level" => &["strict", "balanced", "unsafe"],
        "execute_confirm_policy" => &["always", "risk_only", "never"],
        "execute_user_mode" => &["auto", "normal", "root"],
        "ui_language" => &["zh_cn", "en"],
        "proxy_type" => &["http", "socks5", "socks5h"],
        _ => &[],
    };
    if !choices.is_empty() {
        result["allowed_values"] = json!(choices);
    }
    if let Some((group, leaf)) = key.split_once('.') {
        if group == "tool_groups" {
            result["default"] = json!(false);
            result["resolved"] = json!(resolved[group][leaf].as_bool().unwrap_or(false));
            result["current_task"] = json!(active[group][leaf].as_bool().unwrap_or(false));
        } else if let Some(owner) = super::optional_group(leaf) {
            for (label, config) in [
                ("enabled_after_reload", resolved),
                ("enabled_current_task", active),
            ] {
                result[label] = json!(config[group][leaf]
                    .as_bool()
                    .unwrap_or_else(|| config["tool_groups"][owner].as_bool().unwrap_or(false)));
            }
        }
    }
    result
}

struct ConfigOperation {
    path: PathBuf,
    before: Option<String>,
    after: Option<String>,
    result: Value,
}

#[async_trait]
impl PreparedExecution for ConfigOperation {
    async fn execute(self: Box<Self>, _: &mut ToolContext<'_>) -> Result<ToolOutput> {
        let operation = *self;
        tokio::task::spawn_blocking(move || {
            if let Some(after) = operation.after {
                let parent = operation
                    .path
                    .parent()
                    .context("configuration has no parent")?;
                // A stable sidecar lock serializes tool writers across processes despite atomic rename.
                let mut options = OpenOptions::new();
                options.read(true).write(true).create(true).truncate(false);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options
                        .mode(0o600)
                        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
                }
                let lock = options
                    .open(parent.join(".nl2sh-config.lock"))
                    .context("cannot lock configuration")?;
                if !lock.metadata()?.is_file() {
                    bail!("configuration lock must be a regular file")
                }
                lock.lock().context("cannot acquire configuration lock")?;
                if read_snapshot(&operation.path)? != operation.before {
                    bail!("configuration changed since approval preview; request a new call")
                }
                parse(&after, &operation.path)?
                    .validate_runtime()
                    .map_err(|_| {
                        anyhow::anyhow!("configuration no longer validates; request a new call")
                    })?;
                let mut temporary = tempfile::NamedTempFile::new_in(parent)
                    .context("cannot create private configuration temporary file")?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    temporary
                        .as_file()
                        .set_permissions(fs::Permissions::from_mode(0o600))?;
                }
                temporary
                    .write_all(after.as_bytes())
                    .context("cannot write configuration")?;
                temporary
                    .as_file()
                    .sync_all()
                    .context("cannot sync configuration")?;
                if read_snapshot(&operation.path)? != operation.before {
                    bail!("configuration changed during save; request a new call")
                }
                if operation.before.is_none() {
                    temporary
                        .persist_noclobber(&operation.path)
                        .map_err(|e| e.error)
                        .context("cannot create configuration")?;
                } else {
                    temporary
                        .persist(&operation.path)
                        .map_err(|e| e.error)
                        .context("cannot replace configuration")?;
                }
                File::open(parent)?
                    .sync_all()
                    .context("cannot sync configuration directory")?;
            }
            Ok(ToolOutput::success(serde_json::to_string(
                &operation.result,
            )?))
        })
        .await
        .context("configuration worker failed")?
    }
}

async fn prepare(ctx: &ToolContext<'_>, args: ConfigArgs) -> Result<PreparedToolCall> {
    let active = ctx
        .config
        .context("active configuration unavailable")?
        .clone();
    let source = active
        .source
        .clone()
        .context("active configuration has no source path; cannot guess a configuration file")?;
    let path = if source.is_absolute() {
        source
    } else {
        std::env::current_dir()?.join(source)
    };
    tokio::task::spawn_blocking(move || prepare_sync(active, path, args))
        .await
        .context("configuration preparation worker failed")?
}

fn prepare_sync(active: Config, path: PathBuf, args: ConfigArgs) -> Result<PreparedToolCall> {
    let defaults = config_json(&Config::default())?;
    match args.action {
        Action::List if args.key.is_some() || args.value.is_some() => {
            bail!("list accepts neither key nor value")
        }
        Action::Get | Action::Reset if args.key.is_none() || args.value.is_some() => {
            bail!("get/reset require key and no value")
        }
        Action::Set if args.key.is_none() || args.value.is_none() => {
            bail!("set requires key and a non-null JSON value")
        }
        _ => {}
    }
    if let Some(key) = &args.key {
        check_key(key, &defaults)?;
    }
    let before = read_snapshot(&path)?;
    let text = before.as_deref().unwrap_or("");
    let mut document: DocumentMut = text
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid TOML configuration; repair it in user settings"))?;
    let persisted: Value = serde_json::to_value(
        toml::from_str::<toml::Value>(text)
            .map_err(|_| anyhow::anyhow!("invalid TOML configuration"))?,
    )?;
    let resolved_config = parse(text, &path)?;
    let resolved = config_json(&resolved_config)?;
    let active_json = config_json(&active)?;
    let mut secrets = Vec::new();
    for key in credentials() {
        for values in [&persisted, &resolved, &active_json] {
            if let Some(secret) = values[*key].as_str().filter(|s| !s.is_empty()) {
                secrets.push(secret.to_owned());
            }
        }
    }
    secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let notice = "Writes affect the file only. Current task/clients keep their snapshot. New Web and protocol tasks reload; restart TUI to apply. Environment and CLI overrides may take precedence.";
    if matches!(args.action, Action::List | Action::Get) {
        let keys: Vec<String> = if let Some(key) = args.key {
            vec![key]
        } else {
            let mut keys: Vec<String> = defaults
                .as_object()
                .context("configuration defaults unavailable")?
                .keys()
                .cloned()
                .collect();
            keys.extend(["jadx", "tailcat"].map(|s| format!("tool_groups.{s}")));
            keys.extend(
                super::optional_tool_names()
                    .iter()
                    .map(|s| format!("tool_overrides.{s}")),
            );
            keys
        };
        let mut result = json!({"config_path":path,"file_exists":before.is_some(),"notice":notice,
            "settings":keys.iter().map(|key| describe(key,&persisted,&resolved,&active_json,&defaults)).collect::<Vec<_>>()});
        redact(&mut result, &secrets);
        return Ok(PreparedToolCall::operation(
            String::new(),
            Box::new(ConfigOperation {
                path,
                before,
                after: None,
                result,
            }),
        ));
    }
    let key = args.key.context("configuration key missing")?;
    if credentials().contains(&key.as_str()) {
        bail!("credential fields cannot be set/reset by the model; use /config or Web settings")
    }
    if let Some(value) = &args.value {
        if serde_json::to_vec(value)?.len() > 16 * 1024 {
            bail!("configuration value exceeds 16 KiB")
        }
        if matches!(key.as_str(), "endpoint" | "jev_endpoint") {
            let url = value
                .as_str()
                .and_then(|s| url::Url::parse(s).ok())
                .context("endpoint requires a valid URL string")?;
            if !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                bail!("endpoint values with credentials, query or fragment require user settings")
            }
        }
        // Serialize a single field through TOML to preserve native value types and reject null.
        let wrapper = json!({"value":value});
        let encoded = toml::to_string(&wrapper).map_err(|_| {
            anyhow::anyhow!("value cannot be represented in TOML; use native JSON types")
        })?;
        let mut parsed: DocumentMut = encoded
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid configuration value"))?;
        let item = parsed
            .remove("value")
            .context("configuration value missing")?;
        if let Some((group, leaf)) = key.split_once('.') {
            document[group][leaf] = item;
        } else {
            document[&key] = item;
        }
    } else if let Some((group, leaf)) = key.split_once('.') {
        if let Some(table) = document.get_mut(group).and_then(Item::as_table_like_mut) {
            table.remove(leaf);
        }
    } else {
        document.remove(&key);
    }
    let after = document.to_string();
    if after.len() > MAX_CONFIG_BYTES {
        bail!("updated configuration exceeds 256 KiB")
    }
    let updated = parse(&after, &path)?;
    updated.validate_runtime().map_err(|_| {
        anyhow::anyhow!(
            "updated configuration fails runtime validation; check types, enums and limits"
        )
    })?;
    let updated_json = config_json(&updated)?;
    let persisted_after = serde_json::to_value(
        toml::from_str::<toml::Value>(&after)
            .map_err(|_| anyhow::anyhow!("updated configuration is invalid TOML"))?,
    )?;
    let mut result = json!({"config_path":path,"key":key,"saved":true,"restart_required":true,
        "persisted_before":field(&persisted,&key),"persisted_after":field(&persisted_after,&key),
        "previous":field(&resolved,&key),"resolved_after_reload":field(&updated_json,&key),
        "current_task":field(&active_json,&key),"notice":notice});
    redact(&mut result, &secrets);
    let mut approval = result.clone();
    approval
        .as_object_mut()
        .context("approval preview unavailable")?
        .remove("saved");
    let preview = format!(
        "nl2sh configuration {}\n{}\n{}",
        if matches!(args.action, Action::Set) {
            "set"
        } else {
            "reset"
        },
        "active configuration file",
        serde_json::to_string_pretty(&approval)?
    );
    Ok(PreparedToolCall::operation_with_risk(
        preview,
        Box::new(ConfigOperation {
            path,
            before,
            after: Some(after),
            result,
        }),
        write_risk(&key),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::{ConfirmationDecision, Confirmer},
        security::SecurityAssessment,
        shell::ShellExecutor,
        tools::{file::domain::FileToolExecutor, runtime::invoke},
    };

    struct Decision(bool);
    #[async_trait]
    impl Confirmer for Decision {
        async fn confirm(&self, _: &str, _: &SecurityAssessment) -> Result<ConfirmationDecision> {
            Ok(if self.0 {
                ConfirmationDecision::Approve
            } else {
                ConfirmationDecision::Reject
            })
        }
    }

    fn setup(text: &str) -> Result<(tempfile::TempDir, Config)> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("selected.toml");
        fs::write(&source, text)?;
        let config = parse(text, &source)?;
        Ok((dir, config))
    }

    async fn call(
        config: &Config,
        approved: bool,
        args: Value,
    ) -> Result<super::super::runtime::DirectToolResult> {
        let executor = ShellExecutor::new(config.clone());
        invoke(config, &executor, &Decision(approved), "nl2sh_config", args).await
    }

    #[tokio::test]
    async fn reads_show_persisted_and_current_snapshot_without_leaking_secrets() -> Result<()> {
        let (dir,mut config) = setup("api_key = 'disk-secret'\nproxy_password = 'proxy-secret'\nmodel = 'disk-secret-model'\nendpoint = 'https://user:pass@example.com/v1?token=hidden'\n")?;
        config.api_key = "active-secret".into();
        config.model = "current-model".into();
        let before = fs::read(dir.path().join("selected.toml"))?;
        let result = call(&config, false, json!({"action":"list"})).await?;
        assert!(result.success);
        for secret in [
            "disk-secret",
            "active-secret",
            "proxy-secret",
            "hidden",
            "user:pass",
        ] {
            assert!(!result.output.contains(secret), "leaked {secret}");
        }
        assert!(result.output.contains("current-model"));
        assert!(result.output.contains("persisted_configured"));
        assert_eq!(fs::read(dir.path().join("selected.toml"))?, before);
        assert!(!dir.path().join(".nl2sh-config.lock").exists());
        let result = call(&config, false, json!({"action":"get","key":"api_type"})).await?;
        assert!(result.output.contains("auto"));
        Ok(())
    }

    #[tokio::test]
    async fn writes_require_approval_and_preserve_comments_credentials_and_snapshot() -> Result<()>
    {
        let text = "# Keep this comment\napi_key = 'disk-secret' # credential\nmodel = 'old'\n\n[tool_groups]\njadx = false # keep\n";
        let (dir, config) = setup(text)?;
        let args = json!({"action":"set","key":"model","value":"new-model"});
        let refused = call(&config, false, args.clone()).await?;
        assert!(!refused.success);
        let path = dir.path().join("selected.toml");
        assert_eq!(fs::read_to_string(&path)?, text);
        let accepted = call(&config, true, args).await?;
        assert!(accepted.success);
        assert!(!accepted.output.contains("disk-secret"));
        let saved = fs::read_to_string(&path)?;
        assert!(saved.contains("# Keep this comment"));
        assert!(saved.contains("api_key = 'disk-secret' # credential"));
        assert!(saved.contains("jadx = false # keep"));
        assert_eq!(config.model, "old");
        assert_eq!(parse(&saved, &path)?.model, "new-model");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
        }
        Ok(())
    }

    #[tokio::test]
    async fn invalid_keys_types_limits_actions_and_secret_writes_never_change_file() -> Result<()> {
        let (dir, config) = setup("model = 'old'\n")?;
        for args in [
            json!({"action":"read"}),
            json!({"action":"get"}),
            json!({"action":"list","key":"model"}),
            json!({"action":"list","value":true}),
            json!({"action":"get","key":"source"}),
            json!({"action":"get","key":"unknown"}),
            json!({"action":"set","key":"model","value":false}),
            json!({"action":"set","key":"model","value":null}),
            json!({"action":"set","key":"execute_timeout_secs","value":0}),
            json!({"action":"set","key":"api_type","value":"invalid"}),
            json!({"action":"set","key":"api_key","value":"never-secret"}),
            json!({"action":"reset","key":"proxy_password"}),
            json!({"action":"set","key":"tool_groups.unknown","value":true}),
            json!({"action":"set","key":"tool_overrides.nl2sh_config","value":false}),
            json!({"action":"set","key":"model","value":"x".repeat(17000)}),
            json!({"action":"get","key":"model","path":"other.toml"}),
        ] {
            assert!(
                call(&config, true, args.clone()).await.is_err(),
                "accepted {args}"
            );
            assert_eq!(
                fs::read_to_string(dir.path().join("selected.toml"))?,
                "model = 'old'\n"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn reset_and_dotted_keys_follow_loader_presets_and_tool_inheritance() -> Result<()> {
        let (dir,config) = setup("agent_mode = 'deep'\nmax_agent_steps = 12\n[tool_groups]\ntailcat = true\n[tool_overrides]\ntailcat_check = false\n")?;
        assert!(
            call(
                &config,
                true,
                json!({"action":"reset","key":"max_agent_steps"})
            )
            .await?
            .success
        );
        assert!(
            call(
                &config,
                true,
                json!({"action":"reset","key":"tool_overrides.tailcat_check"})
            )
            .await?
            .success
        );
        assert!(
            call(
                &config,
                true,
                json!({"action":"set","key":"tool_groups.jadx","value":true})
            )
            .await?
            .success
        );
        let saved = parse(
            &fs::read_to_string(dir.path().join("selected.toml"))?,
            &dir.path().join("selected.toml"),
        )?;
        assert_eq!(saved.max_agent_steps, 100);
        assert!(super::super::tool_enabled(&saved, "tailcat_check"));
        assert!(super::super::tool_enabled(&saved, "inspect_apk"));
        Ok(())
    }

    #[tokio::test]
    async fn stale_approval_and_missing_source_are_rejected() -> Result<()> {
        let (dir, config) = setup("model = 'old'\n")?;
        let path = dir.path().join("selected.toml");
        let file_tools = FileToolExecutor::new(dir.path())?;
        let mut ctx = ToolContext {
            file_tools: &file_tools,
            ima: None,
            config: Some(&config),
            executor: None,
            llm: None,
            confirmer: None,
            audio_tools: None,
            runtime: None,
            audio_cache: None,
        };
        let prepared = prepare(
            &ctx,
            serde_json::from_value(json!({"action":"set","key":"model","value":"new"}))?,
        )
        .await?;
        fs::write(&path, "model = 'changed-by-user'\n")?;
        if let super::super::PreparedAction::Operation(operation) = prepared.action {
            assert!(operation.execute(&mut ctx).await.is_err());
        } else {
            bail!("unexpected prepared action")
        }
        assert_eq!(fs::read_to_string(&path)?, "model = 'changed-by-user'\n");
        assert!(call(&Config::default(), true, json!({"action":"list"}))
            .await
            .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn risk_floor_is_enforced_for_set_and_reset() -> Result<()> {
        let (dir, config) = setup("")?;
        let file_tools = FileToolExecutor::new(dir.path())?;
        let ctx = ToolContext {
            file_tools: &file_tools,
            ima: None,
            config: Some(&config),
            executor: None,
            llm: None,
            confirmer: None,
            audio_tools: None,
            runtime: None,
            audio_cache: None,
        };
        for key in [
            "security_level",
            "execute_confirm_policy",
            "protocol_start_with_service",
            "protocol_service_port",
            "protocol_auto_approve",
            "execute_user_mode",
            "security_rules",
            "tool_groups",
            "tool_overrides",
            "endpoint",
            "proxy_enabled",
            "history_log_file",
        ] {
            let prepared = prepare(
                &ctx,
                serde_json::from_value(json!({"action":"reset","key":key}))?,
            )
            .await?;
            assert_eq!(prepared.risk, Some(ToolRisk::Dangerous), "{key}");
            assert!(
                META.assessment_for(prepared.risk.context("risk missing")?)
                    .context("assessment missing")?
                    .requires_double_confirmation
            );
        }
        let prepared = prepare(
            &ctx,
            serde_json::from_value(json!({"action":"set","key":"max_agent_steps","value":40}))?,
        )
        .await?;
        assert_eq!(prepared.risk, Some(ToolRisk::Mutating));
        assert!(!prepared.preview.contains("\"saved\": true"));
        assert!(!dir.path().join(".nl2sh-config.lock").exists());
        Ok(())
    }

    #[tokio::test]
    async fn missing_files_can_only_be_created_after_approval() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("new.toml");
        let config = Config {
            source: Some(path.clone()),
            ..Config::default()
        };
        assert!(
            call(&config, false, json!({"action":"list"}))
                .await?
                .success
        );
        assert!(
            !call(
                &config,
                false,
                json!({"action":"set","key":"model","value":"new"})
            )
            .await?
            .success
        );
        assert!(!path.exists());
        assert!(
            call(
                &config,
                true,
                json!({"action":"set","key":"model","value":"new"})
            )
            .await?
            .success
        );
        assert_eq!(
            parse(&fs::read_to_string(path)?, &dir.path().join("new.toml"))?.model,
            "new"
        );
        Ok(())
    }

    #[tokio::test]
    async fn invalid_and_oversized_files_do_not_leak_source() -> Result<()> {
        let (dir, config) = setup("")?;
        let path = dir.path().join("selected.toml");
        fs::write(&path, "api_key = 'never-secret\n")?;
        let error = call(&config, false, json!({"action":"list"}))
            .await
            .err()
            .context("invalid source accepted")?;
        assert!(!format!("{error:#}").contains("never-secret"));
        fs::write(&path, "x".repeat(MAX_CONFIG_BYTES + 1))?;
        assert!(call(
            &config,
            true,
            json!({"action":"set","key":"model","value":"new"})
        )
        .await
        .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn simultaneous_approved_tool_writers_do_not_overwrite_each_other() -> Result<()> {
        let (dir, config) = setup("model = 'old'\n")?;
        let file_tools = FileToolExecutor::new(dir.path())?;
        let mut ctx_a = ToolContext {
            file_tools: &file_tools,
            ima: None,
            config: Some(&config),
            executor: None,
            llm: None,
            confirmer: None,
            audio_tools: None,
            runtime: None,
            audio_cache: None,
        };
        let mut ctx_b = ToolContext {
            file_tools: &file_tools,
            ima: None,
            config: Some(&config),
            executor: None,
            llm: None,
            confirmer: None,
            audio_tools: None,
            runtime: None,
            audio_cache: None,
        };
        let a = prepare(
            &ctx_a,
            serde_json::from_value(json!({"action":"set","key":"model","value":"first"}))?,
        )
        .await?;
        let b = prepare(
            &ctx_b,
            serde_json::from_value(json!({"action":"set","key":"model","value":"second"}))?,
        )
        .await?;
        let (
            super::super::PreparedAction::Operation(a),
            super::super::PreparedAction::Operation(b),
        ) = (a.action, b.action)
        else {
            bail!("unexpected prepared action")
        };
        let (a, b) = tokio::join!(a.execute(&mut ctx_a), b.execute(&mut ctx_b));
        assert_ne!(a.is_ok(), b.is_ok());
        let saved = parse(
            &fs::read_to_string(dir.path().join("selected.toml"))?,
            &dir.path().join("selected.toml"),
        )?;
        assert!(matches!(saved.model.as_str(), "first" | "second"));
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_configuration_is_rejected_without_replacing_link_or_target() -> Result<()> {
        let (dir, mut config) = setup("model = 'old'\n")?;
        let link = dir.path().join("linked.toml");
        std::os::unix::fs::symlink(dir.path().join("selected.toml"), &link)?;
        config.source = Some(link.clone());
        assert!(call(
            &config,
            true,
            json!({"action":"set","key":"model","value":"new"})
        )
        .await
        .is_err());
        assert!(fs::symlink_metadata(link)?.file_type().is_symlink());
        assert_eq!(
            fs::read_to_string(dir.path().join("selected.toml"))?,
            "model = 'old'\n"
        );
        Ok(())
    }
}
