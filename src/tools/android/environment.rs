//! Bounded, read-only Android userspace inventory.

use crate::shell::{CommandExecutor, ExecutionResult};
use anyhow::{Context, Result};
use serde_json::{json, Map, Value};

const COMMANDS: &[&str] = &[
    "toybox", "awk", "sed", "grep", "find", "tar", "gzip", "unzip", "zip", "sqlite3", "nc", "ping",
    "bash", "curl", "wget", "jq", "openssl", "git", "python3", "busybox", "tcpdump",
];

/// Reads a small fixed set of Android facts and command availability without writing to the device.
pub async fn inspect_environment(executor: &dyn CommandExecutor) -> Result<String> {
    let properties = probe(executor, "for p in ro.build.version.release ro.build.version.sdk ro.product.cpu.abi ro.product.cpu.abilist; do printf '%s=%s\\n' \"$p\" \"$(getprop \"$p\")\"; done").await;
    let uid = probe(executor, "id -u").await;
    let selinux = probe(executor, "getenforce").await;
    let memory = probe(
        executor,
        "grep -E '^(MemTotal|MemAvailable):' /proc/meminfo",
    )
    .await;
    let storage = probe(executor, "df -k /data").await;
    let names = COMMANDS.join(" ");
    let command = format!("for t in {names}; do p=$(command -v \"$t\" 2>/dev/null); if [ -n \"$p\" ]; then printf '%s=%s\\n' \"$t\" \"$p\"; fi; done");
    let available = probe(executor, &command).await;

    let mut commands = Map::new();
    for name in COMMANDS {
        let path = available
            .as_ref()
            .and_then(|result| field(&result.stdout, name));
        commands.insert((*name).into(), json!(path));
    }
    let device_abi = properties
        .as_ref()
        .and_then(|result| field(&result.stdout, "ro.product.cpu.abi"));
    let api_level = properties
        .as_ref()
        .and_then(|result| field(&result.stdout, "ro.build.version.sdk"));
    let data = storage.as_ref().and_then(|result| data_kib(&result.stdout));
    let complete = device_abi.is_some()
        && api_level.is_some()
        && data.is_some()
        && [
            properties.as_ref(),
            uid.as_ref(),
            selinux.as_ref(),
            memory.as_ref(),
            storage.as_ref(),
            available.as_ref(),
        ]
        .iter()
        .all(|result| result.is_some());
    let value = json!({
        "kind": "android_environment",
        "status": if complete { "complete" } else { "partial" },
        "android_release": properties.as_ref().and_then(|result| field(&result.stdout, "ro.build.version.release")),
        "api_level": api_level,
        "device_abi": device_abi,
        "supported_abis": properties.as_ref().and_then(|result| field(&result.stdout, "ro.product.cpu.abilist")),
        "uid": uid.as_ref().and_then(|result| result.stdout.trim().parse::<u32>().ok()),
        "selinux": selinux.as_ref().map(|result| result.stdout.trim()).filter(|value| !value.is_empty()),
        "memory_kib": {
            "total": memory.as_ref().and_then(|result| kilobytes(&result.stdout, "MemTotal:")),
            "available": memory.as_ref().and_then(|result| kilobytes(&result.stdout, "MemAvailable:")),
        },
        "data_kib": data,
        "commands": commands,
    });
    serde_json::to_string_pretty(&value).context("cannot encode Android environment")
}

async fn probe(executor: &dyn CommandExecutor, command: &str) -> Option<ExecutionResult> {
    let result = executor.execute_quiet(command, false, false).await.ok()?;
    (result.exit_code == Some(0) && !result.timed_out && !result.interrupted).then_some(result)
}

fn field(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let value = line.strip_prefix(name)?.strip_prefix('=')?.trim();
        (!value.is_empty() && value.len() <= 256).then(|| value.to_owned())
    })
}

fn kilobytes(text: &str, name: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        line.strip_prefix(name)?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()
    })
}

fn data_kib(text: &str) -> Option<Value> {
    let row = text.lines().last()?.split_whitespace().collect::<Vec<_>>();
    if row.len() < 6 {
        return None;
    }
    Some(json!({
        "total": row.get(1)?.parse::<u64>().ok()?,
        "used": row.get(2)?.parse::<u64>().ok()?,
        "available": row.get(3)?.parse::<u64>().ok()?,
        "mountpoint": row.last()?,
    }))
}

#[cfg(test)]
mod tests {
    use super::{data_kib, field, inspect_environment, kilobytes};
    use crate::shell::{CommandExecutor, ExecutionResult};
    use anyhow::{bail, Result};
    use async_trait::async_trait;
    use serde_json::json;

    struct MockExecutor;

    #[async_trait]
    impl CommandExecutor for MockExecutor {
        async fn execute(
            &self,
            _command: &str,
            _needs_root: bool,
            _interactive: bool,
        ) -> Result<ExecutionResult> {
            bail!("environment probes must use quiet execution")
        }

        async fn execute_quiet(
            &self,
            command: &str,
            needs_root: bool,
            interactive: bool,
        ) -> Result<ExecutionResult> {
            if needs_root || interactive {
                bail!("environment probes must stay non-root and noninteractive")
            }
            let output = if command.starts_with("for p in") {
                "ro.build.version.release=14\nro.build.version.sdk=34\nro.product.cpu.abi=armeabi-v7a\nro.product.cpu.abilist=armeabi-v7a,armeabi\n"
            } else if command == "id -u" {
                "0\n"
            } else if command == "getenforce" {
                "Permissive\n"
            } else if command.starts_with("grep -E") {
                "MemTotal: 10000 kB\nMemAvailable: 4000 kB\n"
            } else if command == "df -k /data" {
                "Filesystem 1K-blocks Used Available Use% Mounted on\n/dev/x 100 20 80 20% /data\n"
            } else if command.starts_with("for t in") {
                "toybox=/system/bin/toybox\nunzip=/system/bin/unzip\n"
            } else {
                bail!("unexpected environment probe")
            };
            Ok(ExecutionResult {
                stdout: output.into(),
                stderr: String::new(),
                exit_code: Some(0),
                timed_out: false,
                interrupted: false,
            })
        }
    }

    #[test]
    fn parses_device_abi_and_bounded_capacity_fields() {
        assert_eq!(
            field("ro.product.cpu.abi=armeabi-v7a\n", "ro.product.cpu.abi"),
            Some("armeabi-v7a".into())
        );
        assert_eq!(
            kilobytes("MemAvailable:  12345 kB\n", "MemAvailable:"),
            Some(12345)
        );
        assert_eq!(
            data_kib(
                "Filesystem 1K-blocks Used Available Use% Mounted on\n/dev/x 100 20 80 20% /data\n"
            ),
            Some(json!({"total":100,"used":20,"available":80,"mountpoint":"/data"}))
        );
        assert_eq!(data_kib("no filesystem data"), None);
    }

    #[tokio::test]
    async fn inventory_uses_device_abi_and_reports_missing_commands() -> Result<()> {
        let output = inspect_environment(&MockExecutor).await?;
        let value: serde_json::Value = serde_json::from_str(&output)?;
        assert_eq!(value["status"], "complete");
        assert_eq!(value["device_abi"], "armeabi-v7a");
        assert_eq!(value["commands"]["unzip"], "/system/bin/unzip");
        assert!(value["commands"]["curl"].is_null());
        assert_eq!(value["memory_kib"]["available"], 4000);
        Ok(())
    }
}
