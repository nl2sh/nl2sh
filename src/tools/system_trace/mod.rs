//! Bounded device Perfetto capture and native protobuf diagnostics.
mod analysis;
mod wire;

use super::{
    PreparedExecution, PreparedToolCall, ToolCategory, ToolContext, ToolMetadata, ToolOutput,
    ToolRisk,
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};
use wire::{bytes, fields, put_bytes, put_num, string};

const MAX_TRACE_BYTES: u64 = 64 * 1024 * 1024;
const PERFETTO: &str = "/system/bin/perfetto";

/// Parameters for a fixed, bounded Perfetto capture.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartArgs {
    /// Optional exact application package for atrace instrumentation; system scheduling remains global.
    #[serde(default)]
    pub package: Option<String>,
    /// Automatic stop after 1–120 seconds; default 10.
    #[serde(default = "default_duration")]
    pub duration_secs: u32,
    /// Ring buffer size in MiB, 1–32; default 8.
    #[serde(default = "default_buffer")]
    pub buffer_mb: u32,
}
fn default_duration() -> u32 {
    10
}
fn default_buffer() -> u32 {
    8
}
/// Only a trace created by start_system_trace can be stopped.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StopArgs {
    /// Opaque ID returned by start_system_trace.
    pub trace_id: String,
}
/// Parameters for native, bounded raw Perfetto protobuf analysis.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeArgs {
    /// Managed trace ID. Provide either trace_id or path.
    #[serde(default)]
    pub trace_id: Option<String>,
    /// Existing raw protobuf trace file (at most 64 MiB), including externally recorded files.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Optional target process ID; main thread has tid == pid.
    #[serde(default)]
    pub pid: Option<u32>,
    /// Optional exact recorded package name; selects one process, mutually exclusive with pid.
    #[serde(default)]
    pub package: Option<String>,
    /// Long wait/slice/Binder delivery threshold, 1–1000 ms; default 50.
    #[serde(default = "default_threshold")]
    pub threshold_ms: u64,
    /// Choreographer duration budget, 1–100 ms; default 16.667. Set for the actual refresh rate.
    #[serde(default = "default_frame_budget")]
    pub frame_budget_ms: f64,
}
fn default_threshold() -> u64 {
    50
}
fn default_frame_budget() -> f64 {
    16.667
}

macro_rules! metadata {
    ($meta:ident, $name:literal, $description:literal, $args:ty, $risk:ident, $platform:ident, $concurrency:ident) => {
        const $meta: ToolMetadata = ToolMetadata {
            name: $name,
            description: $description,
            category: ToolCategory::Android,
            risk: ToolRisk::$risk,
            requires: &[],
            group: None,
            default_enabled: true,
            platform: super::ToolPlatform::$platform,
            runtime: super::RuntimeRequirement::None,
            concurrency: super::ToolConcurrency::$concurrency,
            lifetime: super::ToolLifetime::Call,
            schema: super::descriptor_schema::<$args>,
        };
    };
}
metadata!(START, "start_system_trace", "After confirmation, start a bounded device Perfetto system trace (1–120s, 1–32 MiB buffer, 64 MiB file limit). Returns a managed trace_id. In an Agent task, registers bounded background analysis after auto-stop; direct invocation requires explicit analysis. Requires available linux.ftrace; optional FrameTimeline/process metadata are capability-probed. Does not elevate or install Perfetto.", StartArgs, Mutating, AndroidShell, Sequential);
metadata!(STOP, "stop_system_trace", "After confirmation, stop only the managed Perfetto session identified by trace_id and finalize its trace file. Safe across bridge processes; expired captures are recognized. Never kills an arbitrary PID or another tracing session.", StopArgs, Mutating, AndroidShell, Sequential);
metadata!(ANALYZE, "analyze_system_trace", "Read a bounded raw Perfetto protobuf file in Rust: runnable main-thread/RenderThread delays, long render/Choreographer slices, legacy FrameTimeline jank, Binder send-to-receive latency, CPU competition and wakeup heuristics. Explicit coverage and limitations; missing events are not proof of health. No external trace processor required.", AnalyzeArgs, ReadOnly, Any, Parallel);

define_tool!(StartTool, StartArgs, START, prepare_start);
define_tool!(StopTool, StopArgs, STOP, prepare_stop);
define_tool!(AnalyzeTool, AnalyzeArgs, ANALYZE, prepare_analyze);

fn validate_package(package: Option<&str>) -> Result<()> {
    if let Some(package) = package {
        if package.is_empty()
            || package.len() > 255
            || !package.contains('.')
            || package.split('.').any(|part| {
                part.is_empty() || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            })
        {
            bail!("package must be an exact Android package name")
        }
    }
    Ok(())
}
fn validate_id(id: &str) -> Result<()> {
    if !id.starts_with("trace-")
        || !(12..=64).contains(&id.len())
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("invalid managed trace ID")
    }
    Ok(())
}
fn root(ctx: &ToolContext<'_>) -> Result<PathBuf> {
    let source = ctx
        .config
        .and_then(|c| c.source.clone())
        .map(Ok)
        .unwrap_or_else(crate::config::default_config_path)?;
    let path = crate::config::state_dir(&source)?.join("system-traces");
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}
fn private_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).context("cannot inspect private trace directory")?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        bail!("trace directory must be owned by current UID with private permissions")
    }
    Ok(())
}
fn create_root(path: &Path) -> Result<()> {
    match fs::create_dir(path) {
        Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(0o700))?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e).context("cannot create private trace directory"),
    }
    private_directory(path)
}
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .context("cannot open trace file")?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > limit {
        bail!("trace must be a regular file within the size budget")
    }
    let mut data = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut data)
        .context("cannot read trace file")?;
    if data.len() as u64 > limit {
        bail!("trace grew beyond size budget")
    }
    Ok(data)
}
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .context("cannot create private trace state")?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
#[derive(Debug, Serialize, Deserialize)]
struct Session {
    id: String,
    deadline_unix_secs: u64,
    package: Option<String>,
    sources: Vec<String>,
}
fn trace_path(id: &str) -> Result<PathBuf> {
    validate_id(id)?;
    Ok(Path::new("/data/misc/perfetto-traces").join(format!("nl2sh-{id}.pftrace")))
}
fn managed_file(path: &Path) -> Result<u64> {
    let metadata = fs::symlink_metadata(path).context("managed trace evidence unavailable")?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        bail!("managed trace must be a private regular file owned by current UID")
    }
    Ok(metadata.len())
}
fn load_session(root: &Path, id: &str) -> Result<(PathBuf, Session)> {
    validate_id(id)?;
    private_directory(root)?;
    let dir = root.join(id);
    private_directory(&dir)?;
    let state: Session = serde_json::from_slice(&read_bounded(&dir.join("session.json"), 8192)?)
        .context("invalid trace state")?;
    if state.id != id {
        bail!("trace state identity mismatch")
    }
    Ok((dir, state))
}
fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

// A bounded machine process: no shell, root promotion, PTY, raw terminal output or PID signals.
// Explicit wait on timeout/error; concurrent pipe drains prevent stdout/stderr deadlocks.
async fn perfetto(args: &[String]) -> Result<(i32, Vec<u8>, String)> {
    perfetto_io(args, None).await
}
async fn perfetto_io(args: &[String], input: Option<Vec<u8>>) -> Result<(i32, Vec<u8>, String)> {
    if !cfg!(target_os = "android") {
        bail!("Perfetto capture requires Android")
    }
    let mut child = Command::new(PERFETTO)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("device Perfetto is unavailable")?;
    let stdout = child.stdout.take();
    let stdin = child.stdin.take();
    let stderr = child.stderr.take().context("Perfetto stderr unavailable")?;
    async fn drain<R: tokio::io::AsyncRead + Unpin>(reader: R) -> Result<Vec<u8>> {
        let mut data = Vec::new();
        reader.take(1024 * 1024 + 1).read_to_end(&mut data).await?;
        if data.len() > 1024 * 1024 {
            bail!("Perfetto reply exceeds 1 MiB")
        }
        Ok(data)
    }
    let result = tokio::time::timeout(Duration::from_secs(35), async {
        let write_config = async {
            if let (Some(mut stdin), Some(input)) = (stdin, input) {
                stdin.write_all(&input).await?;
                stdin.shutdown().await?;
            }
            Ok::<_, anyhow::Error>(())
        };
        let read_output = async {
            if let Some(stdout) = stdout {
                drain(stdout).await
            } else {
                Ok(Vec::new())
            }
        };
        let (out, err, _) = tokio::try_join!(read_output, drain(stderr), write_config)?;
        let status = child.wait().await?;
        Ok::<_, anyhow::Error>((
            status.code().unwrap_or(-1),
            out,
            String::from_utf8_lossy(&err).into_owned(),
        ))
    })
    .await;
    match result {
        Ok(Ok(result)) => Ok(result),
        other => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            match other {
                Ok(Err(error)) => Err(error),
                _ => bail!("Perfetto command timed out; child reaped"),
            }
        }
    }
}
async fn sources() -> Result<Vec<String>> {
    let (code, raw, _) = perfetto(&["--query-raw".into()]).await?;
    if code != 0 {
        bail!("Perfetto service capability query failed")
    }
    let mut sources = Vec::new();
    for source in fields(&raw)?.iter().filter(|f| f.id == 2) {
        let source = fields(source.bytes)?;
        if let Some(descriptor) = bytes(&source, 1) {
            let name = string(&fields(descriptor)?, 1);
            if [
                "linux.ftrace",
                "linux.process_stats",
                "android.surfaceflinger.frametimeline",
            ]
            .contains(&name.as_str())
                && !sources.contains(&name)
            {
                sources.push(name);
            }
        }
    }
    if !sources.iter().any(|s| s == "linux.ftrace") {
        bail!("device Perfetto service does not offer linux.ftrace")
    }
    sources.sort();
    Ok(sources)
}
fn config(args: &StartArgs, sources: &[String]) -> Vec<u8> {
    // Stable binary TraceConfig subset; never accepts model-provided pbtxt/config source.
    let mut output = Vec::new();
    let mut buffer = Vec::new();
    put_num(&mut buffer, 1, u64::from(args.buffer_mb) * 1024);
    put_num(&mut buffer, 4, 1);
    put_bytes(&mut output, 1, &buffer);
    let mut ftrace = Vec::new();
    for event in [
        "sched/sched_switch",
        "sched/sched_wakeup",
        "binder/binder_transaction",
        "binder/binder_transaction_received",
        "ftrace/print",
    ] {
        put_bytes(&mut ftrace, 1, event.as_bytes());
    }
    for category in ["gfx", "view", "binder_driver"] {
        put_bytes(&mut ftrace, 2, category.as_bytes());
    }
    if let Some(package) = &args.package {
        put_bytes(&mut ftrace, 3, package.as_bytes());
    }
    let mut compact = Vec::new();
    put_num(&mut compact, 1, 0);
    put_bytes(&mut ftrace, 12, &compact);
    let mut ds = Vec::new();
    put_bytes(&mut ds, 1, b"linux.ftrace");
    put_bytes(&mut ds, 100, &ftrace);
    add_source(&mut output, ds);
    for source in [
        "linux.process_stats",
        "android.surfaceflinger.frametimeline",
    ] {
        if sources.iter().any(|s| s == source) {
            let mut ds = Vec::new();
            put_bytes(&mut ds, 1, source.as_bytes());
            if source == "linux.process_stats" {
                // ProcessStatsConfig.scan_all_processes_on_start = 2.
                let mut stats = Vec::new();
                put_num(&mut stats, 2, 1);
                put_bytes(&mut ds, 103, &stats);
            }
            add_source(&mut output, ds);
        }
    }
    put_num(&mut output, 3, u64::from(args.duration_secs) * 1000);
    put_num(&mut output, 8, 1);
    put_num(&mut output, 9, 1000);
    put_num(&mut output, 10, MAX_TRACE_BYTES);
    output
}
fn add_source(out: &mut Vec<u8>, config: Vec<u8>) {
    let mut ds = Vec::new();
    put_bytes(&mut ds, 1, &config);
    put_bytes(out, 2, &ds);
}

struct StartOperation {
    args: StartArgs,
    root: PathBuf,
    sources: Vec<String>,
}
async fn prepare_start(ctx: &ToolContext<'_>, args: StartArgs) -> Result<PreparedToolCall> {
    validate_package(args.package.as_deref())?;
    if !(1..=120).contains(&args.duration_secs) || !(1..=32).contains(&args.buffer_mb) {
        bail!("duration must be 1–120s and buffer 1–32 MiB")
    }
    let sources = sources().await?;
    let root = root(ctx)?;
    let preview = format!("Start device Perfetto system trace: duration={}s buffer={} MiB file_limit=64 MiB package={:?}\nPrivate metadata: {}; trace files: /data/misc/perfetto-traces/nl2sh-<trace_id>.pftrace (0600)\nSystem-wide scheduler/Binder events may contain sensitive process and UI names; no upload. Sources: {:?}", args.duration_secs, args.buffer_mb, args.package, root.display(), sources);
    Ok(PreparedToolCall::operation(
        preview,
        Box::new(StartOperation {
            args,
            root,
            sources,
        }),
    ))
}
#[async_trait]
impl PreparedExecution for StartOperation {
    async fn execute(self: Box<Self>, ctx: &mut ToolContext<'_>) -> Result<ToolOutput> {
        // Re-probe after approval; fail instead of silently changing the approved data sources.
        if sources().await? != self.sources {
            bail!("Perfetto capabilities changed after confirmation; request a new capture")
        }
        let root = self.root.clone();
        let config = config(&self.args, &self.sources);
        let stored_config = config.clone();
        let (_dir, state) = tokio::task::spawn_blocking(move || -> Result<_> {
            create_root(&root)?;
            let _root_lock = create_lock(&root)?;
            // Limit retained sessions/storage; do not silently remove evidence.
            if fs::read_dir(&root)?
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().starts_with("trace-"))
                .take(16)
                .count()
                >= 16
            {
                bail!("16 retained traces reached; remove old trace directories explicitly")
            }
            let temp = tempfile::Builder::new()
                .prefix("trace-")
                .rand_bytes(16)
                .tempdir_in(&root)?;
            let dir = temp.path().to_path_buf();
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
            private_directory(&dir)?;
            let id = dir
                .file_name()
                .context("trace directory missing name")?
                .to_string_lossy()
                .into_owned();
            let state = Session {
                id,
                deadline_unix_secs: now()? + u64::from(self.args.duration_secs) + 5,
                package: self.args.package,
                sources: self.sources,
            };
            write_private(&dir.join("config.pb"), &stored_config)?;
            write_private(&dir.join("session.json"), &serde_json::to_vec(&state)?)?;

            write_private(&dir.join("lock"), &[])?;
            Ok((temp.keep(), state))
        })
        .await
        .context("trace state worker failed")??;
        // Android SELinux confines Perfetto file access to its system trace directory.
        // stdin supplies the private config; Perfetto itself creates a 0600 output file.
        let trace_path = trace_path(&state.id)?;
        let path_check = trace_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            match fs::symlink_metadata(path_check) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                _ => bail!("managed Perfetto output already exists or cannot be inspected"),
            }
        })
        .await??;
        let args = vec![
            format!("--detach={}", state.id),
            "-c".into(),
            "-".into(),
            "-o".into(),
            trace_path.to_string_lossy().into_owned(),
        ];
        let result = perfetto_io(&args, Some(config)).await;
        match result {
            Ok((0, _, _)) => {
                let (scheduled, scheduling_error) = match schedule_analysis(ctx, &state) {
                    Ok(scheduled) => (scheduled, None),
                    Err(error) => (false, Some(format!("{error:#}"))),
                };
                Ok(ToolOutput::success(serde_json::to_string(
                    &json!({"status":"recording", "trace_id":state.id,"path":trace_path,"auto_stop_deadline_unix_secs":state.deadline_unix_secs,"sources":state.sources,"background_analysis_scheduled":scheduled,"background_analysis_error":scheduling_error,"next":if scheduled {"Task runtime will wait until the deadline and invoke analyze_system_trace through the normal tool boundary before requesting the final model answer. Cancellation or process exit cancels continuation, not the bounded Perfetto capture."} else {"Background analysis is not scheduled: after auto-stop, invoke analyze_system_trace explicitly; stop_system_trace requires approval if needed."}}),
                )?))
            }
            other => {
                // A detach acknowledgement can fail after service start. Attempt only this key.
                let _ = perfetto(&[format!("--attach={}", state.id), "--stop".into()]).await;
                match other {
                    Ok((code, _, error)) => bail!(
                        "Perfetto start failed (exit {code}): {}",
                        crate::limits::truncate_text(&error, 2048)
                    ),
                    Err(error) => Err(error),
                }
            }
        }
    }
}

fn schedule_analysis(ctx: &mut ToolContext<'_>, state: &Session) -> Result<bool> {
    let Some(runtime) = ctx.runtime.as_deref_mut() else {
        return Ok(false);
    };
    runtime.background.schedule(
        Duration::from_secs(state.deadline_unix_secs.saturating_sub(now()?)),
        crate::llm::ToolCall {
            id: String::new(),
            name: "analyze_system_trace".into(),
            arguments: json!({"trace_id":state.id,"package":state.package}),
        },
    )
}
struct StopOperation {
    root: PathBuf,
    id: String,
}
async fn prepare_stop(ctx: &ToolContext<'_>, args: StopArgs) -> Result<PreparedToolCall> {
    validate_id(&args.trace_id)?;
    let root = root(ctx)?;
    let id = args.trace_id.clone();
    let lookup = root.clone();
    let (_, session) = tokio::task::spawn_blocking(move || load_session(&lookup, &id)).await??;
    Ok(PreparedToolCall::operation(format!("Stop and finalize only managed Perfetto trace {} (package {:?}). Keep its private protobuf evidence; no arbitrary PID signals.", session.id, session.package), Box::new(StopOperation {root, id: args.trace_id})))
}
async fn detached(id: &str) -> Result<bool> {
    let (code, _, _) = perfetto(&[format!("--is_detached={id}")]).await?;
    match code {
        0 => Ok(true),
        2 => Ok(false),
        _ => bail!("cannot establish Perfetto session state"),
    }
}
#[async_trait]
impl PreparedExecution for StopOperation {
    async fn execute(self: Box<Self>, _: &mut ToolContext<'_>) -> Result<ToolOutput> {
        let root = self.root.clone();
        let id = self.id.clone();
        let (dir, state) = tokio::task::spawn_blocking(move || load_session(&root, &id)).await??;
        // Serializes stop/analyze across processes; lock never held while waiting for user approval.
        let lock_dir = dir.clone();
        let _lock = tokio::task::spawn_blocking(move || lock(&lock_dir)).await??;
        let mut status = "already_stopped";
        if detached(&state.id).await? {
            let (code, _, error) =
                perfetto(&[format!("--attach={}", state.id), "--stop".into()]).await?;
            if code != 0 {
                bail!(
                    "Perfetto stop failed: {}",
                    crate::limits::truncate_text(&error, 2048)
                )
            }
            status = "stopped";
        }
        let trace_path = trace_path(&state.id)?;
        let inspect = trace_path.clone();
        let bytes = tokio::task::spawn_blocking(move || managed_file(&inspect)).await??;
        if bytes == 0 {
            bail!("Perfetto session ended without trace evidence")
        }
        Ok(ToolOutput::success(serde_json::to_string(
            &json!({"status":status,"trace_id":state.id,"path":trace_path,"bytes":bytes,"next":"analyze_system_trace"}),
        )?))
    }
}
fn create_lock(dir: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join("lock"))?;
    if unsafe {
        libc::flock(
            std::os::fd::AsRawFd::as_raw_fd(&file),
            libc::LOCK_EX | libc::LOCK_NB,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error()).context("cannot lock trace state");
    }
    Ok(file)
}
fn lock(dir: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(dir.join("lock"))?;
    if !file.metadata()?.is_file() {
        bail!("invalid trace lock")
    }
    if unsafe {
        libc::flock(
            std::os::fd::AsRawFd::as_raw_fd(&file),
            libc::LOCK_EX | libc::LOCK_NB,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error())
            .context("trace is busy; retry after the current operation");
    }
    Ok(file)
}
struct AnalyzeOperation {
    args: AnalyzeArgs,
    root: PathBuf,
}
async fn prepare_analyze(ctx: &ToolContext<'_>, args: AnalyzeArgs) -> Result<PreparedToolCall> {
    if args.trace_id.is_some() == args.path.is_some() {
        bail!("provide exactly one of trace_id or path")
    }
    if let Some(id) = &args.trace_id {
        validate_id(id)?;
    }
    validate_package(args.package.as_deref())?;
    if args.pid == Some(0) || (args.pid.is_some() && args.package.is_some()) {
        bail!("provide one nonzero pid or package")
    }
    if !(1..=1000).contains(&args.threshold_ms)
        || !args.frame_budget_ms.is_finite()
        || !(1.0..=100.0).contains(&args.frame_budget_ms)
    {
        bail!("threshold must be 1–1000 ms and frame budget 1–100 ms")
    }
    let root = root(ctx)?;
    Ok(PreparedToolCall::operation(
        String::new(),
        Box::new(AnalyzeOperation { args, root }),
    ))
}
#[async_trait]
impl PreparedExecution for AnalyzeOperation {
    async fn execute(self: Box<Self>, _: &mut ToolContext<'_>) -> Result<ToolOutput> {
        let mut held_lock = None;
        let path = if let Some(id) = &self.args.trace_id {
            let root = self.root.clone();
            let id_owned = id.clone();
            let (_dir, state, guard) = tokio::task::spawn_blocking(move || -> Result<_> {
                let (dir, state) = load_session(&root, &id_owned)?;
                let guard = lock(&dir)?;
                Ok((dir, state, guard))
            })
            .await??;
            held_lock = Some(guard);
            if detached(id).await? {
                bail!("trace is still recording; call stop_system_trace first")
            }
            trace_path(&state.id)?
        } else {
            self.args.path.clone().context("trace path missing")?
        };
        let result = tokio::task::spawn_blocking(move || {
            if held_lock.is_some() {
                managed_file(&path)?;
            }
            let _guard = held_lock;
            let data = read_bounded(&path, MAX_TRACE_BYTES)?;
            if data.is_empty() {
                bail!("trace file is empty")
            }
            analysis::analyze(
                &data,
                self.args.pid,
                self.args.package.as_deref(),
                self.args.threshold_ms,
                self.args.frame_budget_ms,
            )
        })
        .await
        .context("trace analysis worker failed")??;
        Ok(ToolOutput::success(result))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Tool, ToolRegistry};
    use super::*;
    #[test]
    fn trace_schedules_exact_analysis_only_for_an_agent_owner() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let files = crate::tools::file::domain::FileToolExecutor::new(temp.path())?;
        let mut runtime = crate::agent::TaskRuntime::new();
        let state = Session {
            id: "trace-abcdefghijklmnop".into(),
            deadline_unix_secs: now()? + 35,
            package: Some("com.example.app".into()),
            sources: vec![],
        };
        let mut ctx = ToolContext {
            file_tools: &files,
            ima: None,
            config: None,
            executor: None,
            llm: None,
            confirmer: None,
            audio_tools: None,
            runtime: Some(&mut runtime),
            audio_cache: None,
        };
        assert!(!schedule_analysis(&mut ctx, &state)?);
        ctx.runtime
            .as_deref_mut()
            .context("runtime")?
            .background
            .enable();
        assert!(schedule_analysis(&mut ctx, &state)?);
        let job = ctx
            .runtime
            .as_deref_mut()
            .context("runtime")?
            .background
            .pop()
            .context("job")?;
        assert_eq!(job.call.name, "analyze_system_trace");
        assert_eq!(
            job.call.arguments,
            json!({"trace_id":state.id,"package":state.package})
        );
        assert!(
            job.ready_at
                .saturating_duration_since(tokio::time::Instant::now())
                <= Duration::from_secs(35)
        );
        Ok(())
    }
    #[test]
    fn descriptors_require_confirmation_and_restrict_input() -> Result<()> {
        let registry = ToolRegistry::builtin(&[]);
        for name in ["start_system_trace", "stop_system_trace"] {
            let tool = registry.get(name).context("trace tool missing")?;
            assert!(
                tool.metadata()
                    .assessment()
                    .context("assessment missing")?
                    .requires_confirmation
            );
            assert_eq!(tool.metadata().risk, ToolRisk::Mutating);
        }
        assert_eq!(AnalyzeTool.metadata().risk, ToolRisk::ReadOnly);
        for id in ["../bad", "trace-../../outside", "trace-abc;kill", "trace-a"] {
            assert!(validate_id(id).is_err());
        }
        for p in ["*", "com.test;id", "com.test\n", "com..test"] {
            assert!(validate_package(Some(p)).is_err());
        }
        assert!(serde_json::from_value::<StartArgs>(json!({"command":"id"})).is_err());
        assert!(
            serde_json::from_value::<StopArgs>(json!({"trace_id":"trace-abcdefgh", "pid":1}))
                .is_err()
        );
        Ok(())
    }
    #[tokio::test]
    async fn rejecting_prepared_stop_never_touches_capture_state() -> Result<()> {
        use crate::agent::{ConfirmationDecision, Confirmer, TaskRuntime};
        struct Reject;
        #[async_trait]
        impl Confirmer for Reject {
            async fn confirm(
                &self,
                _: &str,
                _: &crate::security::SecurityAssessment,
            ) -> Result<ConfirmationDecision> {
                Ok(ConfirmationDecision::Reject)
            }
        }
        let temp = tempfile::tempdir()?;
        let config = crate::config::Config {
            source: Some(temp.path().join("config.toml")),
            ..Default::default()
        };
        let root = temp.path().join("system-traces");
        create_root(&root)?;
        let id = "trace-abcdefghijk";
        let dir = root.join(id);
        fs::create_dir(&dir)?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        let state = serde_json::to_vec(&Session {
            id: id.into(),
            deadline_unix_secs: 0,
            package: None,
            sources: vec![],
        })?;
        write_private(&dir.join("session.json"), &state)?;
        let files = crate::tools::file::domain::FileToolExecutor::new(temp.path())?;
        let mut runtime = TaskRuntime::new();
        let mut ctx = ToolContext {
            file_tools: &files,
            ima: None,
            config: Some(&config),
            executor: None,
            llm: None,
            confirmer: Some(&Reject),
            audio_tools: None,
            runtime: Some(&mut runtime),
            audio_cache: None,
        };
        let prepared = StopTool.prepare(&ctx, json!({"trace_id":id})).await?;
        let super::super::PreparedAction::Operation(operation) = prepared.action else {
            bail!("wrong action")
        };
        let output = crate::tools::runtime::execute_prepared_operation(
            &STOP,
            prepared.risk,
            &prepared.preview,
            operation,
            &mut ctx,
            &Reject,
        )
        .await?;
        assert!(!output.success);
        assert_eq!(fs::read(dir.join("session.json"))?, state);
        assert!(!dir.join("lock").exists());
        Ok(())
    }
    #[test]
    fn managed_session_lock_is_nonblocking_and_released() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let first = create_lock(dir.path())?;
        assert!(lock(dir.path()).is_err());
        drop(first);
        assert!(lock(dir.path()).is_ok());
        Ok(())
    }
    #[test]
    fn binary_config_has_limits_and_disables_compact_sched() -> Result<()> {
        let args = StartArgs {
            package: Some("com.example.test".into()),
            duration_secs: 10,
            buffer_mb: 8,
        };
        let encoded = config(
            &args,
            &["linux.ftrace".into(), "linux.process_stats".into()],
        );
        let p = fields(&encoded)?;
        assert_eq!(wire::num(&p, 3), Some(10000));
        assert_eq!(wire::num(&p, 8), Some(1));
        assert_eq!(wire::num(&p, 10), Some(MAX_TRACE_BYTES));
        let ds = fields(bytes(&p, 2).context("source missing")?)?;
        let config = fields(bytes(&ds, 1).context("source config missing")?)?;
        assert_eq!(string(&config, 1), "linux.process_stats");
        let ftrace_ds = p.iter().find(|p| p.id == 2).context("ftrace missing")?;
        let ftrace_ds = fields(ftrace_ds.bytes)?;
        let ds = fields(bytes(&ftrace_ds, 1).context("ftrace config missing")?)?;
        let ft = fields(bytes(&ds, 100).context("ft missing")?)?;
        let compact = fields(bytes(&ft, 12).context("compact missing")?)?;
        assert_eq!(wire::num(&compact, 1), Some(0));
        Ok(())
    }
    #[test]
    fn paths_and_reads_reject_symlinks_and_nonregular_files() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().join("traces");
        create_root(&root)?;
        let target = dir.path().join("target");
        write_private(&target, b"abc")?;
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link)?;
        assert!(read_bounded(&link, 64).is_err());
        assert!(read_bounded(dir.path(), 64).is_err());
        assert!(read_bounded(&target, 2).is_err());
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755))?;
        assert!(private_directory(&root).is_err());
        Ok(())
    }
}
