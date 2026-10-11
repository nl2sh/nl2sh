use crate::shell::{CommandExecutor, ExecutionResult};
use anyhow::{bail, Context, Result};
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

const DEFAULT_LOGCAT_LINES: usize = 200;
const MAX_LOGCAT_LINES: usize = 500;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectAndroidAppArgs {
    #[serde(default)]
    pub package: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListAndroidAppsArgs {
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TopAndroidAppsArgs {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AndroidDumpsysArgs {
    pub service: String,
    #[serde(default)]
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AndroidLogcatArgs {
    #[serde(default)]
    pub lines: Option<usize>,
    #[serde(default)]
    pub filter: Option<String>,
    /// Look back on the device clock, 1–3600 seconds; cap the returned matching lines.
    #[serde(default)]
    pub since_secs: Option<u32>,
    /// Exact main-process package; fails if no unique running PID can be resolved.
    #[serde(default)]
    pub package: Option<String>,
    /// Explicit process ID; mutually exclusive with package.
    #[serde(default)]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AndroidSettingsArgs {
    pub namespace: String,
    #[serde(default)]
    pub key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AndroidContentQueryArgs {
    pub uri: String,
    #[serde(default)]
    pub projection: Option<ProjectionArg>,
    #[serde(default)]
    pub where_clause: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
/// Backward-compatible content-provider projection accepted as text or columns.
pub enum ProjectionArg {
    /// Comma- or colon-delimited projection used by older callers.
    Text(String),
    /// Preferred structured list of projection columns.
    Columns(Vec<String>),
}

pub async fn inspect_android_app(
    executor: &dyn CommandExecutor,
    args: &InspectAndroidAppArgs,
) -> Result<String> {
    let activity = execute_readonly(executor, "dumpsys activity activities").await?;
    let component = foreground_component(&activity.stdout);
    let package = match args.package.as_deref().filter(|value| !value.is_empty()) {
        Some(package) => {
            validate_identifier("package", package)?;
            package.to_owned()
        }
        None => component
            .as_deref()
            .and_then(|value| value.split('/').next())
            .filter(|value| !value.is_empty())
            .context("cannot determine the foreground Android package")?
            .to_owned(),
    };
    let package_arg = shell_quote(&package);
    let memory = execute_readonly(executor, &format!("dumpsys meminfo {package_arg}")).await?;
    let package_info =
        execute_readonly(executor, &format!("dumpsys package {package_arg}")).await?;
    let process = execute_readonly(
        executor,
        &format!("pidof {package_arg}; top -b -n 1 | grep -F {package_arg} | head -n 5"),
    )
    .await?;
    let storage = execute_readonly(
        executor,
        &format!(
            "pm path {package_arg}; du -sk /data/user/0/{package} /data/data/{package} 2>/dev/null"
        ),
    )
    .await?;
    let summary = json!({
        "pid": first_capture(&process.stdout, r"(?m)^\s*(\d+)\s*$"),
        "version_name": first_capture(&package_info.stdout, r"(?m)versionName=([^\s]+)"),
        "version_code": first_capture(&package_info.stdout, r"(?m)versionCode=(\d+)"),
        "total_pss_kb": first_capture(&memory.stdout, r"(?m)^\s*TOTAL\s+(\d+)"),
        "total_rss_kb": first_capture(&memory.stdout, r"(?m)TOTAL RSS:\s*(\d+)"),
        "apk_paths": storage.stdout.lines().filter_map(|line| line.strip_prefix("package:")).collect::<Vec<_>>(),
        "storage_kb_lines": storage.stdout.lines().filter(|line| line.chars().next().is_some_and(|value| value.is_ascii_digit())).collect::<Vec<_>>(),
    });
    let value = json!({
        "status": "ok",
        "package": package,
        "foreground_component": component,
        "summary": summary,
        "activity": result_value(activity),
        "memory": result_value(memory),
        "package_info": result_value(package_info),
        "process": result_value(process),
        "storage": result_value(storage),
    });
    serde_json::to_string_pretty(&value).context("cannot encode Android app diagnostics")
}

pub async fn list_android_apps(
    executor: &dyn CommandExecutor,
    args: &ListAndroidAppsArgs,
) -> Result<String> {
    let scope = args.scope.as_deref().unwrap_or("all");
    let flag = match scope {
        "all" => "",
        "user" => " -3",
        "system" => " -s",
        _ => bail!("application scope must be all, user, or system"),
    };
    let limit = args.limit.unwrap_or(200).clamp(1, 500);
    let command = format!("pm list packages -f -U{flag} | head -n {limit}");
    let result = execute_readonly(executor, &command).await?;
    let packages = result
        .stdout
        .lines()
        .filter_map(parse_package_line)
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&json!({
        "status": result_status(&result),
        "scope": scope,
        "count": packages.len(),
        "packages": packages,
        "stderr": result.stderr,
    }))
    .context("cannot encode Android application list")
}

pub async fn top_android_apps(
    executor: &dyn CommandExecutor,
    args: &TopAndroidAppsArgs,
) -> Result<String> {
    let limit = args.limit.unwrap_or(10).clamp(1, 50);
    let command = format!("top -b -n 1 -m {limit} -o PID,USER,RES,%MEM,%CPU,CMDLINE -s 3");
    let result = execute_readonly(executor, &command).await?;
    let rows = result
        .stdout
        .lines()
        .filter(|line| {
            line.split_whitespace()
                .next()
                .is_some_and(|value| value.chars().all(|character| character.is_ascii_digit()))
        })
        .take(limit)
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&json!({
        "status": result_status(&result),
        "sort": "resident_memory",
        "count": rows.len(),
        "rows": rows,
        "raw_header": result.stdout.lines().filter(|line| !line.trim().is_empty()).take(5).collect::<Vec<_>>(),
        "stderr": result.stderr,
    }))
    .context("cannot encode top Android applications")
}

pub async fn android_dumpsys(
    executor: &dyn CommandExecutor,
    args: &AndroidDumpsysArgs,
) -> Result<String> {
    validate_identifier("dumpsys service", &args.service)?;
    validate_dumpsys_query(args)?;
    let mut command = format!("dumpsys {}", shell_quote(&args.service));
    if let Some(arguments) = args.arguments.as_deref().filter(|value| !value.is_empty()) {
        validate_words("dumpsys arguments", arguments)?;
        for argument in arguments.split_whitespace() {
            command.push(' ');
            command.push_str(&shell_quote(argument));
        }
    }
    encode_single("dumpsys", execute_readonly(executor, &command).await?)
}

pub async fn android_logcat(
    executor: &dyn CommandExecutor,
    args: &AndroidLogcatArgs,
) -> Result<String> {
    validate_logcat_args(args)?;
    let lines = args
        .lines
        .unwrap_or(DEFAULT_LOGCAT_LINES)
        .clamp(1, MAX_LOGCAT_LINES);
    let pid = if let Some(package) = &args.package {
        let result = execute_readonly(executor, &format!("pidof {}", shell_quote(package))).await?;
        if result.exit_code != Some(0) || result.timed_out || result.interrupted {
            bail!("target package has no verified running main process; no broad log query was issued")
        }
        let pids = result
            .stdout
            .split_whitespace()
            .map(str::parse::<u32>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if pids.len() != 1 || pids[0] <= 1 {
            bail!("target package has no unique running main process; specify pid")
        }
        Some(pids[0])
    } else {
        args.pid
    };
    let cutoff = if let Some(seconds) = args.since_secs {
        let clock = execute_readonly(executor, "date '+%s %z'").await?;
        if clock.exit_code != Some(0) || clock.timed_out || clock.interrupted {
            bail!("cannot read device clock")
        }
        Some(logcat_cutoff(&clock.stdout, seconds)?)
    } else {
        None
    };
    let command = logcat_command(args, lines, pid, cutoff.as_deref())?;
    let mut result = execute_readonly(executor, &command).await?;
    result.stdout = redact_logcat(&result.stdout)?;
    result.stderr = redact_logcat(&result.stderr)?;
    let capped = result.stdout.lines().count() >= lines;
    serde_json::to_string_pretty(&json!({
        "kind": "logcat", "result": result_value(result), "package": args.package, "pid": pid,
        "since_secs": args.since_secs, "device_cutoff": cutoff, "line_limit": lines, "limit_reached": capped,
        "scope": if args.package.is_some() { "current main process only; restarted and child processes may be omitted" } else { "requested PID or accessible buffers" },
        "redacted": true,
        "limitations": "Bounded snapshot; no matching logs does not prove no error. Credential patterns are best effort."
    })).context("cannot encode Android logcat snapshot")
}

/// Validate structured log filters before any device command is prepared or issued.
pub(crate) fn validate_logcat_args(args: &AndroidLogcatArgs) -> Result<()> {
    if let Some(filter) = args.filter.as_deref().filter(|value| !value.is_empty()) {
        validate_logcat_filter(filter)?;
    }
    if let Some(package) = &args.package {
        super::automation::validate_package(package)?;
    }
    if args.package.is_some() && args.pid.is_some() {
        bail!("logcat package and pid are mutually exclusive")
    }
    if args
        .pid
        .is_some_and(|pid| pid <= 1 || pid > i32::MAX as u32)
    {
        bail!("invalid logcat PID")
    }
    if args
        .since_secs
        .is_some_and(|seconds| !(1..=3600).contains(&seconds))
    {
        bail!("logcat since_secs must be 1–3600")
    }
    Ok(())
}
fn logcat_command(
    args: &AndroidLogcatArgs,
    lines: usize,
    pid: Option<u32>,
    cutoff: Option<&str>,
) -> Result<String> {
    let mut command = if let Some(cutoff) = cutoff {
        // -T starts at device-local time; -m caps matching entries without clearing buffers.
        format!(
            "logcat -d -v threadtime -T {} -m {lines}",
            shell_quote(cutoff)
        )
    } else {
        format!("logcat -d -v threadtime -t {lines}")
    };
    if let Some(pid) = pid {
        command.push_str(&format!(" --pid {pid}"));
    }
    if let Some(filter) = args.filter.as_deref().filter(|value| !value.is_empty()) {
        // Each filter spec is an argument. Quoting the whole list breaks Android parsing.
        for rule in filter.split_whitespace() {
            command.push(' ');
            command.push_str(&shell_quote(rule));
        }
    }
    Ok(command)
}
fn logcat_cutoff(clock: &str, seconds: u32) -> Result<String> {
    let fields: Vec<_> = clock.split_whitespace().collect();
    if fields.len() != 2 {
        bail!("invalid device clock reply")
    }
    let epoch: i64 = fields[0].parse().context("invalid device epoch")?;
    let zone = fields[1];
    if zone.len() != 5
        || !zone.is_ascii()
        || !matches!(zone.as_bytes()[0], b'+' | b'-')
        || !zone[1..].bytes().all(|b| b.is_ascii_digit())
    {
        bail!("invalid device timezone")
    }
    let hours: i32 = zone[1..3].parse()?;
    let minutes: i32 = zone[3..5].parse()?;
    if hours > 23 || minutes > 59 {
        bail!("invalid device timezone offset")
    }
    let offset = (hours * 3600 + minutes * 60) * if zone.starts_with('-') { -1 } else { 1 };
    let epoch = epoch
        .checked_sub(i64::from(seconds))
        .context("invalid device time range")?;
    let t = time::OffsetDateTime::from_unix_timestamp(epoch)?
        .to_offset(time::UtcOffset::from_whole_seconds(offset)?);
    Ok(format!(
        "{:02}-{:02} {:02}:{:02}:{:02}.000",
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    ))
}
fn redact_logcat(text: &str) -> Result<String> {
    let header = Regex::new(
        r"(?im)((?:authorization|(?:x-)?(?:api|secret|access)[-_ ]?key)\s*:\s*)[^\r\n]+",
    )?;
    let assignment = Regex::new(
        r#"(?i)(["']?(?:secret|token|password|authorization|api[-_]?key|private[-_]?key|encryption[-_]?key)["']?\s*[:=]\s*)("[^"]*"|'[^']*'|[^\s,;}]+)"#,
    )?;
    let headers = header.replace_all(text, "${1}[REDACTED]");
    Ok(assignment
        .replace_all(&headers, "${1}\"[REDACTED]\"")
        .into_owned())
}

pub async fn android_settings(
    executor: &dyn CommandExecutor,
    args: &AndroidSettingsArgs,
) -> Result<String> {
    if !matches!(args.namespace.as_str(), "system" | "secure" | "global") {
        bail!("settings namespace must be system, secure, or global")
    }
    let command = if let Some(key) = args.key.as_deref().filter(|value| !value.is_empty()) {
        validate_identifier("settings key", key)?;
        format!("settings get {} {}", args.namespace, shell_quote(key))
    } else {
        format!("settings list {}", args.namespace)
    };
    encode_single("settings", execute_readonly(executor, &command).await?)
}

pub async fn android_content_query(
    executor: &dyn CommandExecutor,
    args: &AndroidContentQueryArgs,
) -> Result<String> {
    validate_content_uri(&args.uri)?;
    let mut command = format!("content query --uri {}", shell_quote(&args.uri));
    if let Some(projection) = args.projection.as_ref() {
        let projection = normalize_projection(projection)?;
        command.push_str(" --projection ");
        command.push_str(&shell_quote(&projection));
    }
    if let Some(where_clause) = args
        .where_clause
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        validate_content_fragment("where clause", where_clause)?;
        command.push_str(" --where ");
        command.push_str(&shell_quote(where_clause));
    }
    let result = execute_readonly(executor, &command).await?;
    reject_content_protocol_error(&result)?;
    encode_single("content_query", result)
}

fn normalize_projection(value: &ProjectionArg) -> Result<String> {
    let columns = match value {
        ProjectionArg::Text(value) => value
            .split([',', ':'])
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>(),
        ProjectionArg::Columns(values) => values.clone(),
    };
    if columns.is_empty() || columns.len() > 64 {
        bail!("content query projection must contain 1 to 64 columns")
    }
    for column in &columns {
        validate_identifier("content projection column", column)?;
    }
    Ok(columns.join(":"))
}

fn reject_content_protocol_error(result: &ExecutionResult) -> Result<()> {
    let combined = format!("{}\n{}", result.stdout, result.stderr);
    if combined.contains("Error while accessing provider")
        || combined.contains("IllegalArgumentException")
        || combined.contains("SecurityException")
    {
        bail!("Android content query failed: {}", combined.trim())
    }
    Ok(())
}

async fn execute_readonly(
    executor: &dyn CommandExecutor,
    command: &str,
) -> Result<ExecutionResult> {
    let result = executor.execute_readonly(command).await?;
    crate::audit::execution_result(&result);
    Ok(result)
}

fn encode_single(kind: &str, result: ExecutionResult) -> Result<String> {
    serde_json::to_string_pretty(&json!({
        "kind": kind,
        "result": result_value(result),
    }))
    .context("cannot encode Android diagnostic result")
}

fn result_value(result: ExecutionResult) -> Value {
    let status = result_status(&result);
    json!({
        "status": status,
        "exit_code": result.exit_code,
        "stdout": result.stdout,
        "stderr": result.stderr,
    })
}

fn result_status(result: &ExecutionResult) -> &'static str {
    if result.timed_out || result.interrupted {
        "timed_out"
    } else if result.exit_code == Some(0) {
        "complete"
    } else if !result.stdout.trim().is_empty() {
        "partial"
    } else {
        "failed"
    }
}

fn first_capture(output: &str, pattern: &str) -> Option<String> {
    Regex::new(pattern)
        .ok()?
        .captures(output)?
        .get(1)
        .map(|value| value.as_str().to_owned())
}

fn parse_package_line(line: &str) -> Option<Value> {
    let value = line.strip_prefix("package:")?;
    let (path_and_package, uid) = value.rsplit_once(" uid:").unwrap_or((value, ""));
    let (path, package) = path_and_package.rsplit_once('=')?;
    Some(json!({"package": package, "apk_path": path, "uid": uid}))
}

fn foreground_component(output: &str) -> Option<String> {
    let component = Regex::new(r"([A-Za-z0-9_.]+/[A-Za-z0-9_.$]+)").ok()?;
    output
        .lines()
        .find(|line| {
            line.contains("topResumedActivity")
                || line.contains("mResumedActivity")
                || line.contains("ResumedActivity")
        })
        .and_then(|line| component.captures(line))
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_owned())
}

fn validate_identifier(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '$' | '-')
        })
    {
        bail!("invalid {label}")
    }
    Ok(())
}

fn validate_words(label: &str, value: &str) -> Result<()> {
    if value.chars().all(|character| {
        character.is_ascii_alphanumeric()
            || matches!(character, '.' | '_' | '$' | '-' | ':' | '/' | ' ')
    }) {
        Ok(())
    } else {
        bail!("invalid {label}")
    }
}

pub(crate) fn validate_dumpsys_query(args: &AndroidDumpsysArgs) -> Result<()> {
    let arguments = args.arguments.as_deref().unwrap_or_default();
    if arguments.len() > 1024
        || arguments.split_whitespace().count() > 16
        || !crate::security::readonly_dumpsys(
            &args.service,
            &arguments.split_whitespace().collect::<Vec<_>>(),
        )
    {
        bail!("dumpsys tool accepts reviewed read-only queries only; use an assessed shell command for other options")
    }
    Ok(())
}

pub(crate) fn validate_logcat_filter(value: &str) -> Result<()> {
    if crate::security::readonly_log_filter(value) {
        Ok(())
    } else {
        bail!("logcat filter must contain Tag or Tag:Priority rules, not options")
    }
}

fn validate_content_uri(value: &str) -> Result<()> {
    if value.starts_with("content://")
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(
                    character,
                    ':' | '/' | '.' | '_' | '-' | '%' | '?' | '=' | '&'
                )
        })
    {
        Ok(())
    } else {
        bail!("invalid content URI")
    }
}

fn validate_content_fragment(label: &str, value: &str) -> Result<()> {
    if value.len() <= 1024 && !value.contains(['\n', '\r', '\0']) {
        Ok(())
    } else {
        bail!("invalid content query {label}")
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::{
        foreground_component, normalize_projection, parse_package_line, validate_content_uri,
        validate_identifier, ProjectionArg,
    };

    #[test]
    fn logcat_cutoff_uses_device_timezone_and_handles_day_boundary(
    ) -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            super::logcat_cutoff("1704067200 +0800", 60)?,
            "01-01 07:59:00.000"
        );
        assert_eq!(
            super::logcat_cutoff("1704067200 -0530", 60)?,
            "12-31 18:29:00.000"
        );
        assert!(super::logcat_cutoff("0 +2460", 1).is_err());
        assert!(super::logcat_cutoff("oops +0000", 1).is_err());
        Ok(())
    }
    #[test]
    fn logcat_directed_queries_reject_options_and_broad_fallback(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut args: super::AndroidLogcatArgs = serde_json::from_value(
            serde_json::json!({"package":"com.example", "since_secs":120, "filter":"MyTag:D *:S"}),
        )?;
        super::validate_logcat_args(&args)?;
        let command = super::logcat_command(&args, 200, Some(42), Some("01-01 07:59:00.000"))?;
        let words = shell_words::split(&command)?;
        assert!(words.windows(2).any(|w| w == ["--pid", "42"]));
        assert!(words.contains(&"MyTag:D".to_owned()));
        assert!(words.contains(&"*:S".to_owned()));
        assert!(!words.iter().any(|w| w == "-c" || w == "-G"));
        args.pid = Some(42);
        assert!(super::validate_logcat_args(&args).is_err());
        args.pid = None;
        args.package = Some("com.example;id".into());
        assert!(super::validate_logcat_args(&args).is_err());
        args.package = None;
        args.since_secs = Some(3601);
        assert!(super::validate_logcat_args(&args).is_err());
        Ok(())
    }
    #[test]
    fn logcat_redacts_headers_and_quoted_secret_values() -> Result<(), Box<dyn std::error::Error>> {
        let text = "Tag: api_key=abcdef password=\"with spaces\"\nTag: Authorization: Bearer private-token\nTag: normal evidence";
        let masked = super::redact_logcat(text)?;
        for secret in ["abcdef", "with spaces", "private-token"] {
            assert!(!masked.contains(secret));
        }
        assert!(masked.contains("normal evidence"));
        Ok(())
    }
    struct MissingPackage(std::sync::Mutex<Vec<String>>);
    #[async_trait::async_trait]
    impl crate::shell::CommandExecutor for MissingPackage {
        async fn execute(
            &self,
            command: &str,
            _: bool,
            _: bool,
        ) -> anyhow::Result<crate::shell::ExecutionResult> {
            self.0
                .lock()
                .map_err(|_| anyhow::anyhow!("poisoned"))?
                .push(command.into());
            Ok(crate::shell::ExecutionResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: Some(1),
                timed_out: false,
                interrupted: false,
            })
        }
    }
    #[tokio::test]
    async fn missing_package_never_reads_unrelated_logs() -> anyhow::Result<()> {
        let executor = MissingPackage(std::sync::Mutex::new(Vec::new()));
        let args = serde_json::from_value(serde_json::json!({"package":"com.example"}))?;
        assert!(super::android_logcat(&executor, &args).await.is_err());
        let commands = executor.0.lock().map_err(|_| anyhow::anyhow!("poisoned"))?;
        assert_eq!(commands.as_slice(), ["pidof 'com.example'"]);
        Ok(())
    }

    #[test]
    fn readonly_diagnostic_arguments_reject_actions_and_logcat_options() {
        use super::{validate_dumpsys_query, validate_logcat_filter, AndroidDumpsysArgs};
        for (service, arguments) in [
            ("battery", "set level 0"),
            ("battery", "unplug"),
            ("batterystats", "--reset"),
            ("deviceidle", "force-idle"),
        ] {
            assert!(validate_dumpsys_query(&AndroidDumpsysArgs {
                service: service.into(),
                arguments: Some(arguments.into())
            })
            .is_err());
        }
        for filter in ["-c", "*:D -c", "-G", "Tag:X", "   "] {
            assert!(validate_logcat_filter(filter).is_err());
        }
        for filter in ["*:W", "ActivityManager:I", "My-App:D *:S"] {
            assert!(validate_logcat_filter(filter).is_ok());
        }
    }

    #[test]
    fn parses_foreground_component_across_common_activity_formats() {
        let output = "mResumedActivity: ActivityRecord{abc u0 com.example/.MainActivity t1}";
        assert_eq!(
            foreground_component(output).as_deref(),
            Some("com.example/.MainActivity")
        );
    }

    #[test]
    fn rejects_shell_syntax_in_structured_arguments() {
        assert!(validate_identifier("package", "com.example;id").is_err());
        assert!(validate_content_uri("content://example/items;rm").is_err());
        assert!(validate_content_uri("content://example/items?id=1").is_ok());
    }

    #[test]
    fn parses_package_list_rows() {
        let value = parse_package_line("package:/data/app/example/base.apk=com.example uid:10123")
            .unwrap_or_default();
        assert_eq!(value["package"], "com.example");
        assert_eq!(value["uid"], "10123");
    }

    #[test]
    fn normalizes_content_projection_lists() {
        assert_eq!(
            normalize_projection(&ProjectionArg::Text("_id,_display_name,size".into()))
                .ok()
                .as_deref(),
            Some("_id:_display_name:size")
        );
        assert!(normalize_projection(&ProjectionArg::Columns(vec!["bad;column".into()])).is_err());
    }
}
