//! Structured Android automation, device summaries, media, and clipboard tools.

use crate::shell::{CommandExecutor, ExecutionResult};
use anyhow::{bail, Context, Result};
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_ROWS: usize = 200;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AndroidInputArgs {
    pub action: String,
    pub bounds: String,
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
    #[serde(default)]
    pub end_x: Option<i32>,
    #[serde(default)]
    pub end_y: Option<i32>,
    #[serde(default)]
    pub duration_ms: Option<u32>,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackageLimitArgs {
    #[serde(default)]
    pub package: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClipboardArgs {
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaControlArgs {
    pub action: String,
    #[serde(default)]
    pub level: Option<u8>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaQueryArgs {
    #[serde(default)]
    pub media_type: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub created_after_epoch_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectivityArgs {
    #[serde(default)]
    pub host: Option<String>,
}

/// Re-reads the current UI hierarchy and returns a generated input command only when the
/// supplied point(s) remain inside an exactly matching, enabled control bounds rectangle.
pub async fn prepare_input(
    executor: &dyn CommandExecutor,
    args: &AndroidInputArgs,
) -> Result<String> {
    let bounds = parse_bounds(&args.bounds)?;
    let xml = executor
        .execute_quiet("p=/data/local/tmp/.nl2sh-input-$$.xml; trap 'rm -f \"$p\"' EXIT HUP INT TERM; uiautomator dump \"$p\" >/dev/null && cat \"$p\"", false, false)
        .await
        .context("cannot refresh Android UI hierarchy")?;
    if xml.exit_code != Some(0) {
        bail!("cannot validate input target against the current UI hierarchy")
    }
    let bounds_attribute = format!("bounds=\"{}\"", args.bounds);
    let node = Regex::new(r"<node\s+[^>]+>").context("cannot build UI node validator")?;
    if !node.find_iter(&xml.stdout).any(|value| {
        let value = value.as_str();
        value.contains(&bounds_attribute)
            && (value.contains("enabled=\"true\"") || value.contains("clickable=\"true\""))
    }) {
        bail!("the requested bounds do not identify an enabled current UI control")
    }
    let (x, y) = point(args.x, args.y, bounds)?;
    let duration = args.duration_ms.unwrap_or(700).clamp(100, 10_000);
    match args.action.as_str() {
        "tap" => Ok(format!("input tap {x} {y}")),
        "long_press" => Ok(format!("input swipe {x} {y} {x} {y} {duration}")),
        "swipe" => {
            let end = point(args.end_x, args.end_y, bounds)?;
            Ok(format!(
                "input swipe {x} {y} {} {} {duration}",
                end.0, end.1
            ))
        }
        "text" => {
            let text = args.text.as_deref().context("text action requires text")?;
            if text.len() > 1024 || text.contains(['\n', '\r', '\0']) {
                bail!("invalid input text")
            }
            Ok(format!(
                "input tap {x} {y} && input text {}",
                shell_quote(text)
            ))
        }
        _ => bail!("input action must be tap, long_press, swipe, or text"),
    }
}

pub async fn notification(
    executor: &dyn CommandExecutor,
    args: &PackageLimitArgs,
) -> Result<String> {
    optional_package(args.package.as_deref())?;
    let result = readonly(executor, "dumpsys notification --noredact").await?;
    encode_filtered("notification", result, args.package.as_deref(), args.limit)
}

pub async fn crash_report(
    executor: &dyn CommandExecutor,
    args: &PackageLimitArgs,
) -> Result<String> {
    optional_package(args.package.as_deref())?;
    let limit = args.limit.unwrap_or(20).clamp(1, 100);
    let command = format!("dumpsys dropbox --print system_app_crash system_app_anr data_app_crash data_app_anr | tail -n {}", limit.saturating_mul(80));
    let result = readonly(executor, &command).await?;
    encode_filtered(
        "crash_report",
        result,
        args.package.as_deref(),
        Some(limit.saturating_mul(80)),
    )
}

pub async fn thermal_power(executor: &dyn CommandExecutor) -> Result<String> {
    aggregate(
        executor,
        "thermal_power",
        &[
            ("battery", "dumpsys battery"),
            ("thermal", "dumpsys thermalservice"),
            ("power", "dumpsys power"),
            ("deviceidle", "dumpsys deviceidle"),
        ],
    )
    .await
}

pub async fn netstats(executor: &dyn CommandExecutor, args: &PackageLimitArgs) -> Result<String> {
    optional_package(args.package.as_deref())?;
    let result = readonly(executor, "dumpsys netstats detail").await?;
    encode_filtered("netstats", result, args.package.as_deref(), args.limit)
}

pub async fn storage(executor: &dyn CommandExecutor, args: &PackageLimitArgs) -> Result<String> {
    optional_package(args.package.as_deref())?;
    let limit = args.limit.unwrap_or(20).clamp(1, 100);
    let command = if let Some(package) = args.package.as_deref() {
        format!(
            "df -k; du -sk /data/user/0/{} /data/data/{} 2>/dev/null",
            package, package
        )
    } else {
        format!("df -k; du -sk /data/user/0/* 2>/dev/null | sort -nr | head -n {limit}")
    };
    encode("storage", readonly(executor, &command).await?)
}

pub async fn wifi_eth(executor: &dyn CommandExecutor) -> Result<String> {
    aggregate(
        executor,
        "wifi_eth",
        &[
            ("wifi", "dumpsys wifi"),
            ("ethernet", "dumpsys ethernet"),
            ("interfaces", "ip address show"),
            ("routes", "ip route show"),
        ],
    )
    .await
}

pub async fn doze(executor: &dyn CommandExecutor) -> Result<String> {
    encode("doze", readonly(executor, "dumpsys deviceidle").await?)
}

pub async fn permission_audit(
    executor: &dyn CommandExecutor,
    args: &PackageLimitArgs,
) -> Result<String> {
    optional_package(args.package.as_deref())?;
    let limit = args.limit.unwrap_or(50).clamp(1, 200);
    let command = if let Some(package) = args.package.as_deref() {
        format!("dumpsys package {package}; cmd appops get {package}")
    } else {
        format!("for p in $(pm list packages -3 | cut -d: -f2 | head -n {limit}); do printf '\\n=== %s ===\\n' \"$p\"; dumpsys package \"$p\" | grep -E 'requested permissions:|install permissions:|runtime permissions:|android.permission.|granted='; cmd appops get \"$p\"; done")
    };
    encode("permission_audit", readonly(executor, &command).await?)
}

pub async fn clipboard_read(executor: &dyn CommandExecutor) -> Result<String> {
    encode("clipboard", readonly(executor, "cmd clipboard get").await?)
}

pub fn clipboard_write_command(args: &ClipboardArgs) -> Result<String> {
    let text = args
        .text
        .as_deref()
        .context("clipboard write requires text")?;
    if text.len() > 16 * 1024 || text.contains(['\0', '\r', '\n']) {
        bail!("invalid clipboard text")
    }
    Ok(format!("cmd clipboard set {}", shell_quote(text)))
}

pub async fn media_status(executor: &dyn CommandExecutor) -> Result<String> {
    aggregate(
        executor,
        "media_status",
        &[
            ("sessions", "dumpsys media_session"),
            ("audio", "dumpsys audio"),
        ],
    )
    .await
}

pub fn media_control_command(args: &MediaControlArgs) -> Result<String> {
    let command = match args.action.as_str() {
        "play" => "input keyevent KEYCODE_MEDIA_PLAY".into(),
        "pause" => "input keyevent KEYCODE_MEDIA_PAUSE".into(),
        "play_pause" => "input keyevent KEYCODE_MEDIA_PLAY_PAUSE".into(),
        "next" => "input keyevent KEYCODE_MEDIA_NEXT".into(),
        "previous" => "input keyevent KEYCODE_MEDIA_PREVIOUS".into(),
        "stop" => "input keyevent KEYCODE_MEDIA_STOP".into(),
        "volume_up" => "input keyevent KEYCODE_VOLUME_UP".into(),
        "volume_down" => "input keyevent KEYCODE_VOLUME_DOWN".into(),
        "mute" => "input keyevent KEYCODE_VOLUME_MUTE".into(),
        "set_volume" => format!(
            "cmd media_session volume --set {}",
            args.level.context("set_volume requires level")?.min(100)
        ),
        _ => bail!("unsupported media action"),
    };
    Ok(command)
}

pub async fn media_query(executor: &dyn CommandExecutor, args: &MediaQueryArgs) -> Result<String> {
    let (uri, preferred_projection, fallback_projection) =
        match args.media_type.as_deref().unwrap_or("images") {
            "images" => (
                "content://media/external/images/media",
                "_id:_display_name:width:height:datetaken:date_added:_size",
                "_id:_display_name:date_added:_size",
            ),
            "video" => (
                "content://media/external/video/media",
                "_id:_display_name:duration:width:height:datetaken:date_added:_size",
                "_id:_display_name:duration:date_added:_size",
            ),
            "audio" => (
                "content://media/external/audio/media",
                "_id:_display_name:duration:artist:album:date_added:_size",
                "_id:_display_name:duration:date_added:_size",
            ),
            _ => bail!("media_type must be images, video, or audio"),
        };
    let limit = args.limit.unwrap_or(50).clamp(1, MAX_ROWS);
    let where_clause = args
        .created_after_epoch_secs
        .map(|value| format!(" --where 'date_added>{value}'"))
        .unwrap_or_default();
    let command = |projection: &str| {
        format!("content query --uri {uri} --projection {projection}{where_clause} --sort 'date_added DESC' | head -n {limit}")
    };
    let preferred = readonly(executor, &command(preferred_projection)).await?;
    let (result, projection_mode) = if content_protocol_failed(&preferred) {
        let fallback = readonly(executor, &command(fallback_projection)).await?;
        if content_protocol_failed(&fallback) {
            bail!(
                "MediaStore query failed with preferred and fallback projections: {}",
                fallback.stdout.trim()
            )
        }
        (fallback, "fallback")
    } else {
        (preferred, "preferred")
    };
    serde_json::to_string_pretty(&json!({
        "kind":"media_query",
        "projection_mode":projection_mode,
        "result":result_value(result),
    }))
    .context("cannot encode MediaStore query result")
}

fn content_protocol_failed(result: &ExecutionResult) -> bool {
    let output = format!("{}\n{}", result.stdout, result.stderr);
    output.contains("Error while accessing provider")
        || output.contains("IllegalArgumentException")
        || output.contains("SecurityException")
}

pub async fn connectivity(
    executor: &dyn CommandExecutor,
    args: &ConnectivityArgs,
) -> Result<String> {
    let host = args
        .host
        .as_deref()
        .unwrap_or("connectivitycheck.gstatic.com");
    if host.is_empty()
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
    {
        bail!("invalid connectivity host")
    }
    let dns = readonly(executor, &format!("ping -c 1 -W 2 {host}")).await?;
    let ping = readonly(executor, &format!("ping -c 3 -W 2 {host}")).await?;
    let routes = readonly(executor, "ip route show").await?;
    let connectivity = readonly(executor, "dumpsys connectivity").await?;
    serde_json::to_string_pretty(&json!({
        "kind": "connectivity",
        "host": host,
        "https_checked": false,
        "sections": {
            "dns": compact_result(&dns, 800),
            "ping": compact_result(&ping, 1200),
            "routes": compact_result(&routes, 1200),
            "connectivity": connectivity_summary(&connectivity),
        },
    }))
    .context("cannot encode Android connectivity result")
}

fn compact_result(result: &ExecutionResult, max_chars: usize) -> Value {
    let limit = |text: &str| -> String {
        let mut value = text.chars().take(max_chars).collect::<String>();
        if text.chars().count() > max_chars {
            value.push_str("\n[TRUNCATED]");
        }
        value
    };
    json!({
        "status": status(result),
        "exit_code": result.exit_code,
        "stdout": limit(&result.stdout),
        "stderr": limit(&result.stderr),
    })
}

fn connectivity_summary(result: &ExecutionResult) -> Value {
    let active = result
        .stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("Active default network: "))
        .and_then(|value| value.split_whitespace().next());
    let network = active
        .and_then(|id| {
            result
                .stdout
                .lines()
                .find(|line| line.contains(&format!("NetworkAgentInfo{{network{{{id}}}")))
        })
        .or_else(|| {
            result
                .stdout
                .lines()
                .find(|line| line.contains("NetworkAgentInfo{network{"))
        });
    let extract = |start: &str, end: &str| {
        network
            .and_then(|line| line.split_once(start))
            .and_then(|(_, tail)| tail.split_once(end))
            .map(|(value, _)| value.chars().take(160).collect::<String>())
    };
    json!({
        "status": status(result),
        "exit_code": result.exit_code,
        "active_default_network": active,
        "transport": extract("Transports: ", " Capabilities:"),
        "interface": extract("InterfaceName: ", " "),
        "link_addresses": extract("LinkAddresses: [", "]"),
        "dns_addresses": extract("DnsAddresses: [", "]"),
        "stderr": result.stderr.chars().take(300).collect::<String>(),
    })
}

async fn aggregate(
    executor: &dyn CommandExecutor,
    kind: &str,
    commands: &[(&str, &str)],
) -> Result<String> {
    let mut values = serde_json::Map::new();
    for (name, command) in commands {
        values.insert(
            (*name).into(),
            result_value(readonly(executor, command).await?),
        );
    }
    serde_json::to_string_pretty(&json!({"kind":kind,"sections":values}))
        .context("cannot encode Android tool result")
}

async fn readonly(executor: &dyn CommandExecutor, command: &str) -> Result<ExecutionResult> {
    executor.execute_quiet(command, false, false).await
}

fn encode(kind: &str, result: ExecutionResult) -> Result<String> {
    serde_json::to_string_pretty(&json!({"kind":kind,"result":result_value(result)}))
        .context("cannot encode Android tool result")
}

fn encode_filtered(
    kind: &str,
    result: ExecutionResult,
    needle: Option<&str>,
    limit: Option<usize>,
) -> Result<String> {
    let limit = limit.unwrap_or(MAX_ROWS).clamp(1, MAX_ROWS);
    let rows = result
        .stdout
        .lines()
        .filter(|line| needle.is_none_or(|value| line.contains(value)))
        .take(limit)
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(
        &json!({"kind":kind,"status":status(&result),"rows":rows,"stderr":result.stderr}),
    )
    .context("cannot encode Android tool result")
}

fn result_value(result: ExecutionResult) -> Value {
    json!({"status":status(&result),"exit_code":result.exit_code,"stdout":result.stdout,"stderr":result.stderr})
}
fn status(result: &ExecutionResult) -> &'static str {
    if result.timed_out || result.interrupted {
        "timed_out"
    } else if result.exit_code == Some(0) {
        "complete"
    } else if result.stdout.trim().is_empty() {
        "failed"
    } else {
        "partial"
    }
}
fn optional_package(value: Option<&str>) -> Result<()> {
    if let Some(value) = value {
        if value.is_empty()
            || !value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            bail!("invalid package")
        }
    }
    Ok(())
}
fn parse_bounds(value: &str) -> Result<(i32, i32, i32, i32)> {
    let re = Regex::new(r"^\[(\d+),(\d+)\]\[(\d+),(\d+)\]$")?;
    let c = re
        .captures(value)
        .context("bounds must use [left,top][right,bottom]")?;
    let n = |i| {
        c.get(i)
            .and_then(|v| v.as_str().parse::<i32>().ok())
            .context("invalid bounds coordinate")
    };
    let out = (n(1)?, n(2)?, n(3)?, n(4)?);
    if out.0 >= out.2 || out.1 >= out.3 {
        bail!("invalid empty bounds")
    }
    Ok(out)
}
fn point(x: Option<i32>, y: Option<i32>, b: (i32, i32, i32, i32)) -> Result<(i32, i32)> {
    let p = (x.unwrap_or((b.0 + b.2) / 2), y.unwrap_or((b.1 + b.3) / 2));
    if p.0 < b.0 || p.0 >= b.2 || p.1 < b.1 || p.1 >= b.3 {
        bail!("input point is outside the validated control bounds")
    }
    Ok(p)
}
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_bounds_and_points() {
        let b = parse_bounds("[1,2][30,40]").unwrap_or_default();
        assert_eq!(point(Some(5), Some(6), b).ok(), Some((5, 6)));
        assert!(point(Some(0), Some(6), b).is_err());
        assert!(parse_bounds("1,2,3,4").is_err());
    }
    #[test]
    fn rejects_injected_arguments() {
        assert!(permission_package("bad;id").is_err());
        assert!(clipboard_write_command(&ClipboardArgs {
            text: Some("a\nb".into())
        })
        .is_err());
    }
    fn permission_package(value: &str) -> Result<()> {
        optional_package(Some(value))
    }

    #[test]
    fn connectivity_summary_keeps_active_network_without_raw_dump() {
        let result = ExecutionResult {
            stdout: format!("Active default network: 100\nNetworkAgentInfo{{network{{100}} lp{{{{InterfaceName: eth0 LinkAddresses: [192.0.2.2/24] DnsAddresses: [/192.0.2.1]}}}} nc{{[ Transports: ETHERNET Capabilities: INTERNET]}}}}\n{}", "unrelated\n".repeat(10_000)),
            stderr: String::new(),
            exit_code: Some(0),
            timed_out: false,
            interrupted: false,
        };
        let summary = connectivity_summary(&result);
        assert_eq!(summary["active_default_network"], "100");
        assert_eq!(summary["transport"], "ETHERNET");
        assert_eq!(summary["interface"], "eth0");
        assert_eq!(summary["link_addresses"], "192.0.2.2/24");
        assert!(summary.to_string().len() < 1_000);
    }
}
