use crate::shell::{CommandExecutor, ExecutionResult};
use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};

const DEFAULT_LOGCAT_LINES: usize = 200;
const MAX_LOGCAT_LINES: usize = 500;

#[derive(Debug, Clone, Deserialize)]
pub struct InspectAndroidAppArgs {
    #[serde(default)]
    pub package: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListAndroidAppsArgs {
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TopAndroidAppsArgs {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AndroidDumpsysArgs {
    pub service: String,
    #[serde(default)]
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AndroidLogcatArgs {
    #[serde(default)]
    pub lines: Option<usize>,
    #[serde(default)]
    pub filter: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AndroidSettingsArgs {
    pub namespace: String,
    #[serde(default)]
    pub key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AndroidContentQueryArgs {
    pub uri: String,
    #[serde(default)]
    pub projection: Option<ProjectionArg>,
    #[serde(default)]
    pub where_clause: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
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
    let lines = args
        .lines
        .unwrap_or(DEFAULT_LOGCAT_LINES)
        .clamp(1, MAX_LOGCAT_LINES);
    let mut command = format!("logcat -d -t {lines}");
    if let Some(filter) = args.filter.as_deref().filter(|value| !value.is_empty()) {
        validate_logcat_filter(filter)?;
        command.push(' ');
        command.push_str(&shell_quote(filter));
    }
    encode_single("logcat", execute_readonly(executor, &command).await?)
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
    executor.execute_quiet(command, false, false).await
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

fn validate_logcat_filter(value: &str) -> Result<()> {
    if value.chars().all(|character| {
        character.is_ascii_alphanumeric()
            || matches!(character, '.' | '_' | '$' | '-' | ':' | '*' | ' ')
    }) {
        Ok(())
    } else {
        bail!("invalid logcat filter")
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
