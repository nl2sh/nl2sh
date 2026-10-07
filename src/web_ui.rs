//! Small embedded HTTP interface. All Agent actions use the same runner and confirmer as the TUI.
use crate::{
    agent::{AgentRunner, ConfirmationDecision, Confirmer, QuestionAnswers, UserQuestion},
    config::{self, Config, ConfirmPolicy, ExecuteUserMode},
    file_references::{augment_file_references, file_suggestions},
    history::HistoryLog,
    llm::{
        build_client, ConversationItem, ConversationMessage, LlmClient, LlmRequest, Role,
        TextDeltaSink, ToolRound,
    },
    provider_metadata::build_metadata_client,
    security::{PrivilegeBroker, SecurityAssessment},
    session_title::generate_title,
    sessions::{SessionStore, WebCheckpoint, WebCheckpointEvent, WebPresentation, WebTaskMetrics},
    shell::{
        CommandExecutor, ExecutionBroker, ExecutionResult, OutputSink, ShellExecutor,
        SystemRootProbe,
    },
    tools::{
        android::{
            diagnostics::{list_android_apps, ListAndroidAppsArgs},
            environment::inspect_environment,
        },
        memory::domain::{AgentMemory, AgentMemoryAction, AgentMemoryArgs, MemoryEntry},
        Capability, ToolRegistry,
    },
};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use axum::{
    body::Body,
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Path as AxumPath, Query, State,
    },
    http::{header, HeaderValue, StatusCode, Uri},
    middleware::{self, Next},
    response::{sse::Event, IntoResponse, Response, Sse},
    routing::{get, post},
    Json, Router,
};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Instant, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt},
    net::TcpListener,
    sync::{broadcast, oneshot, watch, Mutex as AsyncMutex},
};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tokio_util::io::ReaderStream;
use tower_http::set_header::SetResponseHeaderLayer;

const PORT: u16 = 9999;
const MAX_REQUEST: usize = 256 * 1024;
const MAX_HISTORY: usize = 400;
const MAX_WEB_SESSIONS: usize = 64;
const MAX_EXPORT_LOG_BYTES: u64 = 20 * 1024 * 1024;
const MAX_FILE_PREVIEW_BYTES: u64 = 2 * 1024 * 1024;
const MAX_IMAGE_PREVIEW_BYTES: u64 = 32 * 1024 * 1024;
const MAX_VIDEO_PREVIEW_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_AUDIO_PREVIEW_BYTES: u64 = 256 * 1024 * 1024;
const MAX_RAW_PCM_PREVIEW_BYTES: u64 = 32 * 1024 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 1000;
const LIVE_TOOL_CALL_PREFIX: &str = "\u{1e}TOOL_CALL:";
const LIVE_TOOL_PENDING_PREFIX: &str = "\u{1e}TOOL_PENDING:";
static WELCOME_URL: OnceLock<String> = OnceLock::new();
static SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static PENDING_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static MODEL_LIST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Makes the running browser address available to startup welcome text.
pub fn set_welcome_url(url: String) {
    let _ = WELCOME_URL.set(url);
}

/// Returns the browser address, if the server has started.
pub fn welcome_url() -> Option<&'static str> {
    WELCOME_URL.get().map(String::as_str)
}

/// Handle of the embedded server; its task lives until the runtime exits.
pub struct WebServer {
    url: String,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
    shutdown: tokio::sync::oneshot::Sender<()>,
    shared: Arc<Shared>,
}

impl WebServer {
    /// Actual bound port, including fallback to an ephemeral port.
    pub fn port(&self) -> u16 {
        self.shared.port
    }

    /// Cancel active tasks and pending approvals, then drain HTTP connections.
    pub async fn shutdown(self) -> Result<()> {
        if let Ok(sessions) = self.shared.sessions.lock() {
            for session in sessions.values() {
                session.cancel.send_replace(true);
                if let Ok(mut inner) = session.inner.lock() {
                    if let Some(reply) = inner.reply.take() {
                        let response = match inner.pending.take() {
                            Some(Pending::Approval { .. }) => {
                                Reply::Approval(ConfirmationDecision::Reject)
                            }
                            _ => Reply::Questions(None),
                        };
                        let _ = reply.send(response);
                    }
                }
            }
        }
        let _ = self.shutdown.send(());
        for _ in 0..50 {
            let busy = self
                .shared
                .sessions
                .lock()
                .map(|sessions| {
                    sessions
                        .values()
                        .any(|session| session.inner.lock().map(|inner| inner.busy).unwrap_or(true))
                })
                .unwrap_or(true);
            if !busy {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let mut task = self.task;
        match tokio::time::timeout(std::time::Duration::from_secs(5), &mut task).await {
            Ok(result) => result
                .context("web shutdown task failed")?
                .context("web shutdown failed"),
            Err(_) => {
                task.abort();
                let _ = task.await;
                Ok(())
            }
        }
    }
    /// Address shown in the terminal welcome message.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Waits until the HTTP server stops and reports task or serving errors.
    pub async fn wait(&mut self) -> Result<()> {
        (&mut self.task)
            .await
            .context("web interface task stopped unexpectedly")?
            .context("web interface stopped unexpectedly")
    }
}

struct Shared {
    path: PathBuf,
    started: Instant,
    port: u16,
    sessions: Mutex<BTreeMap<String, Arc<WebSession>>>,
}

struct WebSession {
    inner: Mutex<SessionState>,
    persist: AsyncMutex<()>,
    events: broadcast::Sender<String>,
    cancel: watch::Sender<bool>,
    terminal_clients: AtomicUsize,
    audit: Mutex<Option<tokio::sync::mpsc::Sender<AuditEvent>>>,
}

struct SessionState {
    id: String,
    title: String,
    history: Vec<String>,
    turns: Vec<Vec<ConversationItem>>,
    busy: bool,
    pending: Option<Pending>,
    reply: Option<oneshot::Sender<Reply>>,
    title_generated: bool,
    created: u64,
    updated: u64,
    steps: usize,
    tool_calls: usize,
    accepted_turns: usize,
    task: WebTaskMetrics,
    task_started: Option<Instant>,
    phase_started: Instant,
    token_base: (u64, u64),
    input_tokens: u64,
    output_tokens: u64,
    final_input_tokens: Option<u64>,
    activity: &'static str,
    activity_detail: Option<String>,
    activity_since_ms: u64,
    checkpoint: Option<WebCheckpoint>,
    checkpoint_start: Option<usize>,
    redaction_secrets: Vec<String>,
}

impl SessionState {
    fn empty() -> Self {
        let created = unix_seconds();
        let id = format!(
            "{}-{}",
            SessionStore::default_name(),
            SESSION_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        Self {
            title: "新会话".into(),
            id,
            history: Vec::new(),
            turns: Vec::new(),
            busy: false,
            pending: None,
            reply: None,
            title_generated: false,
            created,
            updated: created,
            steps: 0,
            tool_calls: 0,
            accepted_turns: 0,
            task: WebTaskMetrics::default(),
            task_started: None,
            phase_started: Instant::now(),
            token_base: (0, 0),
            input_tokens: 0,
            output_tokens: 0,
            final_input_tokens: None,
            activity: "idle",
            activity_detail: None,
            activity_since_ms: unix_millis(),
            checkpoint: None,
            checkpoint_start: None,
            redaction_secrets: Vec::new(),
        }
    }
}

fn web_session(inner: SessionState) -> Arc<WebSession> {
    let (events, _) = broadcast::channel(128);
    let (cancel, _) = watch::channel(false);
    Arc::new(WebSession {
        inner: Mutex::new(inner),
        persist: AsyncMutex::new(()),
        events,
        cancel,
        terminal_clients: AtomicUsize::new(0),
        audit: Mutex::new(None),
    })
}

struct TerminalClientGuard(Arc<WebSession>);

impl Drop for TerminalClientGuard {
    fn drop(&mut self) {
        self.0.terminal_clients.fetch_sub(1, Ordering::Relaxed);
    }
}

fn notify(session: &WebSession, event: &str) {
    let _ = session.events.send(event.to_owned());
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

fn task_metrics(inner: &SessionState) -> WebTaskMetrics {
    let mut task = inner.task.clone();
    task.steps = inner.steps;
    task.tool_calls = inner.tool_calls;
    if let Some(started) = inner.task_started {
        task.total_ms = started.elapsed().as_millis() as u64;
        let elapsed = inner.phase_started.elapsed().as_millis() as u64;
        match inner.activity {
            "thinking" => task.model_ms = task.model_ms.saturating_add(elapsed),
            "tool" => task.tool_ms = task.tool_ms.saturating_add(elapsed),
            "waiting" => task.waiting_ms = task.waiting_ms.saturating_add(elapsed),
            _ => {}
        }
    }
    task
}

fn presentation(inner: &SessionState) -> WebPresentation {
    WebPresentation {
        history: inner.history.clone(),
        turns: inner.accepted_turns,
        task: task_metrics(inner),
        input_tokens: inner.input_tokens,
        output_tokens: inner.output_tokens,
        final_input_tokens: inner.final_input_tokens,
    }
}

fn set_activity(inner: &mut SessionState, activity: &'static str, detail: Option<&str>) {
    if inner.activity != activity || inner.activity_detail.as_deref() != detail {
        inner.task = task_metrics(inner);
        inner.phase_started = Instant::now();
        inner.activity = activity;
        inner.activity_detail = detail.map(str::to_owned);
        inner.activity_since_ms = unix_millis();
    }
}

fn task_summary(inner: &SessionState) -> String {
    let task = task_metrics(inner);
    format!(
        "本轮耗时 {:.1}s · 模型 {:.1}s · 工具 {:.1}s · 等待 {:.1}s · 步骤 {} · 工具 {}",
        task.total_ms as f64 / 1000.0,
        task.model_ms as f64 / 1000.0,
        task.tool_ms as f64 / 1000.0,
        task.waiting_ms as f64 / 1000.0,
        inner.steps,
        inner.tool_calls
    )
}

fn finish_task(inner: &mut SessionState) {
    set_activity(inner, "idle", None);
    inner.task = task_metrics(inner);
    inner.task_started = None;
}

enum AuditEvent {
    Record(String, String),
    Flush(oneshot::Sender<()>),
}

async fn flush_audit(session: &WebSession) {
    let sender = session.audit.lock().ok().and_then(|value| value.clone());
    if let Some(sender) = sender {
        let (tx, rx) = oneshot::channel();
        if sender.send(AuditEvent::Flush(tx)).await.is_ok() {
            let _ = rx.await;
        }
    }
}

fn audit(session: &WebSession, event: &str, message: &str) {
    let encoded = if let Ok(inner) = session.inner.lock() {
        let mut message = message.to_owned();
        for secret in &inner.redaction_secrets {
            message = message.replace(secret, "[NL2SH CREDENTIAL REDACTED]");
        }
        let message = crate::limits::truncate_text(&message, 64 * 1024);
        serde_json::json!({"session_id":inner.id,"message":message}).to_string()
    } else {
        return;
    };
    if let Ok(sender) = session.audit.lock() {
        if let Some(sender) = sender.as_ref() {
            if let Err(error) = sender.try_send(AuditEvent::Record(event.into(), encoded)) {
                eprintln!("cannot queue Web audit event: {error}");
            }
        }
    }
}

async fn start_audit(path: PathBuf, cfg: &Config, session: &WebSession) -> Result<()> {
    let log_path = cfg.history_log_file.clone();
    let event_limit = cfg.history_log_event_max_bytes;
    let file_limit = cfg.history_log_max_bytes;
    let log = tokio::task::spawn_blocking(move || {
        HistoryLog::open_with_limits(&path, &log_path, event_limit, file_limit)
    })
    .await??;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AuditEvent>(128);
    *session
        .audit
        .lock()
        .map_err(|_| anyhow!("Web audit lock poisoned"))? = Some(tx);
    tokio::spawn(async move {
        while let Some(item) = rx.recv().await {
            let (event, message) = match item {
                AuditEvent::Record(event, message) => (event, message),
                AuditEvent::Flush(done) => {
                    let _ = done.send(());
                    continue;
                }
            };
            let log = log.clone();
            match tokio::task::spawn_blocking(move || log.record(&event, &message)).await {
                Ok(Ok(())) => {}
                _ => eprintln!("cannot append Web audit event"),
            }
        }
    });
    Ok(())
}

fn checkpoint_event(inner: &mut SessionState, kind: &str, tool: Option<&str>) {
    if let Some(checkpoint) = inner.checkpoint.as_mut() {
        checkpoint.events.push(WebCheckpointEvent {
            at: unix_seconds(),
            kind: kind.to_owned(),
            tool: tool.map(str::to_owned),
        });
        if checkpoint.events.len() > 120 {
            checkpoint.events.remove(0);
        }
    }
}

fn remember_redaction_secrets(inner: &mut SessionState, cfg: &Config) {
    for secret in [
        &cfg.api_key,
        &cfg.proxy_password,
        &cfg.ima_client_id,
        &cfg.ima_api_key,
        &cfg.jev_api_key,
    ] {
        if !secret.is_empty() && !inner.redaction_secrets.contains(secret) {
            inner.redaction_secrets.push(secret.clone());
        }
    }
}

async fn persist_web_session(state: &Shared, current: &WebSession) -> Result<()> {
    let _write = current.persist.lock().await;
    let (id, title, turns, checkpoint, prior_secrets, display) = {
        let inner = current
            .inner
            .lock()
            .map_err(|_| anyhow!("web session lock poisoned"))?;
        let mut checkpoint = inner.checkpoint.clone();
        if let Some(value) = checkpoint.as_mut() {
            value.activity = inner.activity.into();
            value.history = inner.history[inner.checkpoint_start.unwrap_or(0)..].to_vec();
        }
        (
            inner.id.clone(),
            inner.title.clone(),
            inner.turns.clone(),
            checkpoint,
            inner.redaction_secrets.clone(),
            presentation(&inner),
        )
    };
    let path = state.path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let cfg = config::load_or_default_unvalidated(&path)?;
        let mut secrets = vec![
            cfg.api_key,
            cfg.proxy_password,
            cfg.ima_client_id,
            cfg.ima_api_key,
            cfg.jev_api_key,
        ];
        secrets.extend(prior_secrets);
        SessionStore::open(&path)?.save_web_observed(
            &id,
            &title,
            &turns,
            cfg.model_tool_output_max_bytes,
            &secrets,
            (checkpoint.as_ref(), display),
        )
    })
    .await??;
    Ok(())
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Pending {
    Approval {
        request_id: u64,
        command: String,
        risk: String,
        explanation: String,
        root: bool,
        strong: bool,
        armed: bool,
    },
    Questions {
        request_id: u64,
        questions: Vec<WebQuestion>,
    },
}

#[derive(Clone, Serialize)]
struct WebQuestion {
    id: String,
    header: String,
    prompt: String,
    options: Vec<WebOption>,
}

#[derive(Clone, Serialize)]
struct WebOption {
    label: String,
    value: String,
    description: String,
}

enum Reply {
    Approval(ConfirmationDecision),
    Questions(Option<QuestionAnswers>),
}

#[derive(Serialize)]
struct Snapshot {
    id: String,
    title: String,
    history: Vec<String>,
    entries: Vec<WebEntry>,
    busy: bool,
    pending: Option<Pending>,
    turns: usize,
    steps: usize,
    tool_calls: usize,
    input_tokens: u64,
    output_tokens: u64,
    final_input_tokens: Option<u64>,
    activity: &'static str,
    activity_detail: Option<String>,
    activity_elapsed_ms: u64,
    task: WebTaskMetrics,
    can_retry: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct WebEntry {
    kind: &'static str,
    text: String,
}

#[derive(Serialize)]
struct ExportConversation {
    id: String,
    title: String,
    exported_unix_secs: u64,
    busy: bool,
    entries: Vec<WebEntry>,
    task: WebTaskMetrics,
    checkpoint_status: Option<String>,
    checkpoint_events: Vec<WebCheckpointEvent>,
}

#[derive(Serialize)]
struct SessionSummary {
    id: String,
    title: String,
    turns: usize,
    busy: bool,
    pending: bool,
    created: u64,
    updated: u64,
}

#[derive(Deserialize)]
struct Message {
    session_id: String,
    text: String,
}

#[derive(Deserialize)]
struct Decision {
    session_id: String,
    request_id: u64,
    action: String,
    text: Option<String>,
    answers: Option<QuestionAnswers>,
}

#[derive(Deserialize)]
struct SessionSelection {
    name: String,
}

#[derive(Serialize)]
struct QuickSettings {
    endpoint: String,
    model: String,
    provider_ready: bool,
    confirm_policy: ConfirmPolicy,
    max_context_turns: usize,
    max_agent_steps: usize,
    max_tool_calls: usize,
    context_window: Option<u64>,
    root: String,
}

#[derive(Deserialize)]
struct QuickSettingsUpdate {
    endpoint: String,
    model: String,
    confirm_policy: ConfirmPolicy,
}

/// Starts an unauthenticated browser UI on every IPv4 interface.
pub async fn start(path: PathBuf) -> Result<WebServer> {
    let listener = bind_web_listener(PORT).await?;
    start_with_listener(path, listener).await
}

/// Start at a requested port, optionally refusing an occupied port.
pub async fn start_on_port(path: PathBuf, port: u16, strict: bool) -> Result<WebServer> {
    let listener = if strict {
        TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
            .await
            .context("requested web port is unavailable")?
    } else {
        bind_web_listener(port).await?
    };
    start_with_listener(path, listener).await
}

async fn bind_web_listener(port: u16) -> Result<TcpListener> {
    match TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await {
        Ok(listener) => Ok(listener),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))
                .await
                .context("cannot start web interface on an available port")
        }
        Err(error) => Err(error).context("cannot start web interface"),
    }
}

async fn start_with_listener(path: PathBuf, listener: TcpListener) -> Result<WebServer> {
    let port = listener
        .local_addr()
        .context("cannot identify web port")?
        .port();
    let ip = local_ipv4().unwrap_or(Ipv4Addr::LOCALHOST);
    let url = format!("http://{ip}:{port}/");
    let first = web_session(SessionState::empty());
    let first_id = first
        .inner
        .lock()
        .map_err(|_| anyhow!("web session lock poisoned"))?
        .id
        .clone();
    let mut sessions = BTreeMap::new();
    sessions.insert(first_id, first);
    let shared = Arc::new(Shared {
        started: Instant::now(),
        port,
        path,
        sessions: Mutex::new(sessions),
    });
    let app = router(shared.clone());
    let (shutdown, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
    });
    Ok(WebServer {
        url,
        task,
        shutdown,
        shared,
    })
}

fn local_ipv4() -> Option<Ipv4Addr> {
    if let Ok(socket) = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
        if socket.connect((Ipv4Addr::new(1, 1, 1, 1), 80)).is_ok() {
            if let Ok(address) = socket.local_addr() {
                if let IpAddr::V4(ip) = address.ip() {
                    if !ip.is_loopback() && !ip.is_unspecified() {
                        return Some(ip);
                    }
                }
            }
        }
    }
    interface_ipv4()
}

#[cfg(unix)]
fn interface_ipv4() -> Option<Ipv4Addr> {
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // getifaddrs owns the returned linked list until freeifaddrs is called.
    if unsafe { libc::getifaddrs(&mut list) } != 0 {
        return None;
    }
    let mut current = list;
    let mut selected = None;
    while !current.is_null() {
        // Each address pointer comes from the getifaddrs-owned list.
        let entry = unsafe { &*current };
        if !entry.ifa_addr.is_null()
            && unsafe { (*entry.ifa_addr).sa_family } == libc::AF_INET as u16
        {
            let raw = unsafe { &*(entry.ifa_addr as *const libc::sockaddr_in) };
            let ip = Ipv4Addr::from(u32::from_be(raw.sin_addr.s_addr));
            if !ip.is_loopback() && !ip.is_unspecified() && !ip.is_link_local() {
                selected = Some(ip);
                break;
            }
        }
        current = entry.ifa_next;
    }
    unsafe { libc::freeifaddrs(list) };
    selected
}

#[cfg(not(unix))]
fn interface_ipv4() -> Option<Ipv4Addr> {
    None
}

#[derive(RustEmbed)]
#[folder = "$NL2SH_WEB_DIST/"]
struct WebAssets;

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    error: anyhow::Error,
}

impl ApiError {
    fn bad(error: impl Into<anyhow::Error>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: error.into(),
        }
    }
    fn conflict(message: &'static str) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            error: anyhow!(message),
        }
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(error: E) -> Self {
        Self::bad(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, self.error.to_string()).into_response()
    }
}

type ApiResult<T> = std::result::Result<T, ApiError>;

async fn get_info(State(state): State<Arc<Shared>>) -> ApiResult<Json<serde_json::Value>> {
    let cfg = load_config(state.path.clone()).await?;
    let executor = ShellExecutor::new(cfg.clone());
    let capabilities = crate::runtime::RuntimeCapabilities::discover(&cfg, &executor).await;
    let runtime_policy = match crate::runtime_dependencies::manifest::embedded() {
        Ok(Some(policy)) => serde_json::json!({"status": "verified", "manifest": policy}),
        Ok(None) => serde_json::json!({"status": "unsigned_source_build"}),
        Err(error) => serde_json::json!({"status": "invalid", "error": error.to_string()}),
    };
    Ok(Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"), "protocol": 1,
        "runtime_policy": runtime_policy,
        "update_ownership": crate::update::ownership().await,
        "pid": std::process::id(), "uptime": state.started.elapsed().as_secs(),
        "port": state.port,
        "abi": match std::env::consts::ARCH {
            "aarch64" => "arm64-v8a", "arm" => "armeabi-v7a", other => other,
        },
        "capabilities": capabilities,
    })))
}

fn router(state: Arc<Shared>) -> Router {
    Router::new()
        .route("/healthz", get(|| async { Json(serde_json::json!({"status":"ok"})) }))
        .route("/api/info", get(get_info))
        .route("/api/version", get(|| async { env!("CARGO_PKG_VERSION") }))
        .route("/api/state", get(get_state))
        .route("/api/config", get(get_config).post(save_config))
        .route("/api/config/validate", post(validate_config))
        .route("/api/config/render", post(render_config))
        .route("/api/quick-settings", get(get_quick_settings).post(save_quick_settings))
        .route("/api/models", get(get_models).post(get_draft_models))
        .route("/api/model-check", post(check_model))
        .route("/api/device-overview", get(get_device_overview))
        .route("/api/tools", get(get_tools))
        .route("/api/tools/toggle", post(toggle_tool))
        .route("/api/apps", get(get_apps))
        .route("/api/memory", get(get_memory).post(set_memory))
        .route("/api/memory/delete", post(delete_memory))
        .route("/api/memory/clear", post(clear_memory))
        .route("/api/file-suggestions", get(get_file_suggestions))
        .route("/api/files", get(get_files))
        .route("/api/file-preview", get(get_file_preview))
        .route("/api/sessions", get(get_sessions))
        .route("/api/sessions/new", post(new_session))
        .route("/api/sessions/load", post(load_session))
        .route("/api/sessions/delete", post(delete_session))
        .route("/api/sessions/delete-all", post(delete_all_sessions))
        .route("/api/sessions/export", get(export_session))
        .route("/api/message", post(post_message))
        .route("/api/message/retry", post(retry_message))
        .route("/api/message/cancel", post(cancel_message))
        .route("/api/decision", post(post_decision))
        .route("/api/events", get(session_events))
        .route("/api/terminal/{id}", get(terminal_upgrade))
        .fallback(get(static_asset))
        .layer(middleware::from_fn(validate_origin))
        .layer(DefaultBodyLimit::max(MAX_REQUEST))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'self'; connect-src 'self' ws: wss:; script-src 'self'; style-src 'self'"),
        ))
        .with_state(state)
}

async fn validate_origin(request: axum::extract::Request, next: Next) -> Response {
    let Some(host) = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return (StatusCode::BAD_REQUEST, "Host header is required").into_response();
    };
    let host_name = host.rsplit_once(':').map_or(host, |(name, _)| name);
    if host_name != "localhost" && host_name.parse::<Ipv4Addr>().is_err() {
        return (
            StatusCode::BAD_REQUEST,
            "Host must be an IPv4 address or localhost",
        )
            .into_response();
    }
    if let Some(origin) = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        if origin != format!("http://{host}") {
            return (StatusCode::FORBIDDEN, "origin mismatch").into_response();
        }
    }
    next.run(request).await
}

async fn static_asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let Some(asset) = WebAssets::get(path).or_else(|| WebAssets::get("index.html")) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let content_type = if path.ends_with(".js") {
        "text/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "text/html; charset=utf-8"
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(asset.data.into_owned()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn session(state: &Shared, id: &str) -> Result<Arc<WebSession>> {
    state
        .sessions
        .lock()
        .map_err(|_| anyhow!("web session registry lock poisoned"))?
        .get(id)
        .cloned()
        .context("web session not found")
}

fn web_entries(history: &[String]) -> Vec<WebEntry> {
    history
        .iter()
        .filter_map(|line| {
            let (kind, text) = if line.starts_with(LIVE_TOOL_PENDING_PREFIX) {
                return None;
            } else if let Some(encoded) = line.strip_prefix(LIVE_TOOL_CALL_PREFIX) {
                let (_, name) = encoded.split_once('\t').unwrap_or(("", encoded));
                ("tool_call", name)
            } else if let Some(text) = line.strip_prefix("> ") {
                ("user", text)
            } else if let Some(text) = line.strip_prefix("🤖 ") {
                ("assistant", text)
            } else if let Some(text) = line.strip_prefix("\u{1e}REASON:") {
                ("reasoning", text)
            } else if let Some(text) = line.strip_prefix("\u{1e}TASK:") {
                ("task_summary", text)
            } else if let Some(text) = line.strip_prefix("… ") {
                ("stream", text)
            } else if let Some(text) = line.strip_prefix("🔧 ") {
                ("tool_call", text)
            } else if let Some(text) = line.strip_prefix("\u{1e}TOOL_OK:") {
                ("tool_result", text)
            } else if let Some(text) = line.strip_prefix("\u{1e}TOOL_ERR:") {
                ("tool_error", text)
            } else if line.starts_with("[OUT]") || line.starts_with("[ERR]") {
                ("tool_output", line.as_str())
            } else if let Some(text) = line.strip_prefix("❌ ") {
                ("error", text)
            } else {
                ("notice", line.as_str())
            };
            Some(WebEntry {
                kind,
                text: text.to_owned(),
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct StateQuery {
    id: String,
}

async fn get_state(
    State(state): State<Arc<Shared>>,
    Query(query): Query<StateQuery>,
) -> ApiResult<Json<Snapshot>> {
    let current = session(&state, &query.id)?;
    let inner = current
        .inner
        .lock()
        .map_err(|_| anyhow!("web session lock poisoned"))?;
    Ok(Json(Snapshot {
        id: inner.id.clone(),
        title: inner.title.clone(),
        history: inner.history.clone(),
        entries: web_entries(&inner.history),
        busy: inner.busy,
        pending: inner.pending.clone(),
        turns: inner.accepted_turns,
        steps: inner.steps,
        tool_calls: inner.tool_calls,
        input_tokens: inner.input_tokens,
        output_tokens: inner.output_tokens,
        final_input_tokens: inner.final_input_tokens,
        activity: inner.activity,
        activity_detail: inner.activity_detail.clone(),
        activity_elapsed_ms: unix_millis().saturating_sub(inner.activity_since_ms),
        task: task_metrics(&inner),
        can_retry: retryable_input(&inner).is_some(),
    }))
}

async fn export_session(
    State(state): State<Arc<Shared>>,
    Query(query): Query<StateQuery>,
) -> ApiResult<Response> {
    let current = session(&state, &query.id)?;
    let conversation = {
        let inner = current
            .inner
            .lock()
            .map_err(|_| anyhow!("web session lock poisoned"))?;
        ExportConversation {
            id: inner.id.clone(),
            title: inner.title.clone(),
            exported_unix_secs: unix_seconds(),
            busy: inner.busy,
            entries: web_entries(&inner.history),
            task: task_metrics(&inner),
            checkpoint_status: inner.checkpoint.as_ref().map(|value| value.status.clone()),
            checkpoint_events: inner
                .checkpoint
                .as_ref()
                .map_or_else(Vec::new, |value| value.events.clone()),
        }
    };
    let path = state.path.clone();
    let id = conversation.id.clone();
    let archive =
        tokio::task::spawn_blocking(move || build_session_export(&path, &conversation)).await??;
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"nl2sh-{id}.zip\""))
        .context("invalid export filename")?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/zip"),
            ),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        archive,
    )
        .into_response())
}

fn build_session_export(
    path: &std::path::Path,
    conversation: &ExportConversation,
) -> Result<Vec<u8>> {
    let cfg = config::load_or_default_unvalidated(path)?;
    let log_path = if cfg.history_log_file.is_absolute() {
        cfg.history_log_file
    } else {
        config::state_dir(path)?.join(cfg.history_log_file)
    };
    let log = match std::fs::metadata(&log_path) {
        Ok(metadata) if !metadata.is_file() || metadata.len() > MAX_EXPORT_LOG_BYTES => {
            bail!("history log is invalid or exceeds export size limit")
        }
        Ok(_) => std::fs::read(&log_path).context("cannot read history log")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error).context("cannot inspect history log"),
    };
    if log.len() as u64 > MAX_EXPORT_LOG_BYTES {
        bail!("history log exceeds export size limit")
    }
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file("conversation.json", options)?;
    serde_json::to_writer_pretty(&mut archive, conversation)
        .context("cannot encode conversation")?;
    archive.start_file("nl2sh.log", options)?;
    archive.write_all(&log).context("cannot add history log")?;
    archive.start_file("README.txt", options)?;
    archive.write_all(b"conversation.json contains the selected Web session's visible conversation and unfinished-task checkpoint events at export time.\nnl2sh.log is the shared audit log and may contain events from other sessions. An empty log means no log file existed.\n")?;
    Ok(archive.finish()?.into_inner())
}

async fn get_config(State(state): State<Arc<Shared>>) -> ApiResult<String> {
    let path = state.path.clone();
    Ok(tokio::task::spawn_blocking(move || -> Result<String> {
        if path.exists() {
            std::fs::read_to_string(&path).context("cannot read configuration")
        } else {
            toml::to_string_pretty(&Config::default()).context("cannot render defaults")
        }
    })
    .await??)
}

async fn save_config(State(state): State<Arc<Shared>>, body: String) -> ApiResult<String> {
    let path = state.path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let cfg: Config = toml::from_str(&body).context("invalid TOML configuration")?;
        config::save_config(&path, &cfg)
    })
    .await??;
    Ok("配置已保存；后续 Web 任务自动使用新配置，当前 TUI 会话重启后生效。".into())
}

#[derive(Serialize)]
struct ConfigPreview {
    valid: bool,
    error: Option<String>,
    config: Option<Config>,
}

async fn validate_config(body: String) -> Json<ConfigPreview> {
    let result = tokio::task::spawn_blocking(move || -> Result<Config> {
        let cfg: Config = toml::from_str(&body).context("invalid TOML configuration")?;
        cfg.validate_runtime()?;
        Ok(cfg)
    })
    .await;
    let result = result.unwrap_or_else(|error| Err(error.into()));
    match result {
        Ok(config) => Json(ConfigPreview {
            valid: true,
            error: None,
            config: Some(config),
        }),
        Err(error) => Json(ConfigPreview {
            valid: false,
            error: Some(format!("{error:#}")),
            config: None,
        }),
    }
}

#[derive(Serialize)]
struct RenderedConfig {
    toml: String,
    valid: bool,
    error: Option<String>,
}

async fn render_config(Json(config): Json<Config>) -> ApiResult<Json<RenderedConfig>> {
    tokio::task::spawn_blocking(move || -> Result<Json<RenderedConfig>> {
        let toml = toml::to_string_pretty(&config).context("cannot render configuration")?;
        let error = config
            .validate_runtime()
            .err()
            .map(|error| format!("{error:#}"));
        Ok(Json(RenderedConfig {
            toml,
            valid: error.is_none(),
            error,
        }))
    })
    .await?
    .map_err(Into::into)
}

async fn load_config(path: PathBuf) -> Result<Config> {
    tokio::task::spawn_blocking(move || -> Result<Config> {
        let cfg = config::load_or_default_unvalidated(&path)?;
        cfg.validate_runtime()?;
        Ok(cfg)
    })
    .await?
}

async fn get_quick_settings(State(state): State<Arc<Shared>>) -> ApiResult<Json<QuickSettings>> {
    let cfg = load_config(state.path.clone()).await?;
    Ok(Json(QuickSettings {
        endpoint: cfg.endpoint.clone(),
        model: cfg.model.clone(),
        provider_ready: cfg.provider_is_configured(),
        confirm_policy: cfg.execute_confirm_policy,
        max_context_turns: cfg.max_context_turns,
        max_agent_steps: cfg.max_agent_steps.min(cfg.hard_max_agent_steps),
        max_tool_calls: cfg.max_tool_calls,
        context_window: cfg.effective_context_window(),
        root: format!("{:?}", SystemRootProbe.status()),
    }))
}

async fn save_quick_settings(
    State(state): State<Arc<Shared>>,
    Json(update): Json<QuickSettingsUpdate>,
) -> ApiResult<String> {
    let path = state.path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut cfg = config::load_or_default_unvalidated(&path)?;
        cfg.endpoint = update.endpoint;
        cfg.model = update.model;
        cfg.execute_confirm_policy = update.confirm_policy;
        config::save_config(&path, &cfg)
    })
    .await??;
    Ok("快捷设置已保存，后续 Web 任务生效".into())
}

async fn get_models(State(state): State<Arc<Shared>>) -> ApiResult<Json<Vec<serde_json::Value>>> {
    let cfg = load_config(state.path.clone()).await?;
    list_models(state.path.clone(), cfg, "saved").await
}

#[derive(Deserialize)]
struct ModelListDraft {
    endpoint: String,
    api_key: String,
}

async fn get_draft_models(
    State(state): State<Arc<Shared>>,
    Json(draft): Json<ModelListDraft>,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    let mut cfg = load_config(state.path.clone()).await?;
    cfg.endpoint = draft.endpoint.trim().to_owned();
    cfg.api_key = draft.api_key.trim().to_owned();
    if cfg.endpoint.is_empty() {
        return Err(ApiError::bad(anyhow!(
            "Base URL is required to list models"
        )));
    }
    list_models(state.path.clone(), cfg, "quick_start").await
}

async fn record_model_list_event(path: PathBuf, cfg: &Config, event: &'static str, data: String) {
    let log_path = cfg.history_log_file.clone();
    let event_limit = cfg.history_log_event_max_bytes;
    let file_limit = cfg.history_log_max_bytes;
    let result = tokio::task::spawn_blocking(move || -> Result<()> {
        HistoryLog::open_with_limits(&path, &log_path, event_limit, file_limit)?
            .record(event, &data)
    })
    .await;
    if !matches!(result, Ok(Ok(()))) {
        eprintln!("cannot append model list diagnostic event");
    }
}

async fn list_models(
    path: PathBuf,
    cfg: Config,
    source: &'static str,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    let request_id = MODEL_LIST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let endpoint_host = url::Url::parse(&cfg.endpoint)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "[invalid host]".into());
    let metadata = serde_json::json!({
        "request_id": request_id,
        "source": source,
        "endpoint_host": endpoint_host,
        "provider": format!("{:?}", crate::provider_metadata::provider_kind(&cfg.endpoint)),
        "credential_present": !cfg.api_key.is_empty() || std::env::var_os("NL2SH_API_KEY").is_some(),
        "proxy_enabled": cfg.proxy_enabled,
    });
    record_model_list_event(
        path.clone(),
        &cfg,
        "web_model_list_started",
        metadata.to_string(),
    )
    .await;
    let started = Instant::now();
    let result = build_metadata_client(&cfg).list_models(&cfg).await;
    let diagnostic = match &result {
        Ok(models) => {
            serde_json::json!({"request_id":request_id,"outcome":"ok","model_count":models.len(),"elapsed_ms":started.elapsed().as_millis()})
        }
        Err(error) => {
            serde_json::json!({"request_id":request_id,"outcome":"error","error":error.to_string(),"elapsed_ms":started.elapsed().as_millis()})
        }
    };
    record_model_list_event(
        path,
        &cfg,
        "web_model_list_finished",
        diagnostic.to_string(),
    )
    .await;
    let models = result
        .map_err(|error| ApiError::bad(anyhow!("model list request #{request_id}: {error}")))?;
    Ok(Json(models.into_iter().map(|m|serde_json::json!({"id":m.id,"context_window":m.context_window,"max_output_tokens":m.max_output_tokens})).collect()))
}

#[derive(Serialize)]
struct ModelCheck {
    ok: bool,
}

async fn check_model(State(state): State<Arc<Shared>>) -> ApiResult<Json<ModelCheck>> {
    let cfg = load_config(state.path.clone()).await?;
    if !cfg.provider_is_configured() {
        return Err(ApiError::bad(anyhow!("model provider is not configured")));
    }
    let client = build_client(&cfg)?;
    let request = LlmRequest {
        model: cfg.model.clone(),
        items: vec![ConversationItem::Message(ConversationMessage::new(
            Role::User,
            "Reply with OK.",
        ))],
        tools: Vec::new(),
    };
    let response =
        tokio::time::timeout(std::time::Duration::from_secs(30), client.complete(request))
            .await
            .map_err(|_| anyhow!("model check timed out"))??;
    if response
        .text
        .as_deref()
        .is_none_or(|text| text.trim().is_empty())
    {
        return Err(ApiError::bad(anyhow!("model returned no text")));
    }
    Ok(Json(ModelCheck { ok: true }))
}

async fn get_device_overview(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<serde_json::Value>> {
    let path = state.path.clone();
    let mut cfg =
        tokio::task::spawn_blocking(move || config::load_or_default_unvalidated(&path)).await??;
    cfg.execute_user_mode = ExecuteUserMode::Normal;
    cfg.enable_pty = false;
    cfg.execute_timeout_secs = cfg.execute_timeout_secs.clamp(1, 15);
    let executor = ShellExecutor::new(cfg);
    let details = inspect_environment(&executor).await?;
    Ok(Json(serde_json::from_str(&details)?))
}

#[derive(Serialize)]
struct ToolSummary {
    name: String,
    description: String,
    category: String,
    risk: String,
    group: Option<&'static str>,
    enabled: bool,
    available: bool,
}

async fn get_tools(State(state): State<Arc<Shared>>) -> ApiResult<Json<Vec<ToolSummary>>> {
    let cfg = load_config(state.path.clone()).await?;
    let capabilities = if cfg.ima_enabled {
        vec![Capability::Ima]
    } else {
        Vec::new()
    };
    let executor = ShellExecutor::new(cfg.clone());
    let runtime = crate::runtime::RuntimeCapabilities::discover(&cfg, &executor).await;
    let registry = ToolRegistry::catalog(&capabilities);
    Ok(Json(
        registry
            .definitions()
            .into_iter()
            .filter_map(|tool| {
                let metadata = registry.get(&tool.name)?.metadata();
                Some(ToolSummary {
                    name: tool.name.clone(),
                    description: tool.description,
                    category: format!("{:?}", metadata.category),
                    risk: format!("{:?}", metadata.risk),
                    group: crate::tools::optional_group(&tool.name),
                    enabled: crate::tools::tool_enabled(&cfg, &tool.name),
                    available: runtime.supports(metadata),
                })
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolToggle {
    group: Option<String>,
    tool: Option<String>,
    enabled: bool,
}

async fn toggle_tool(
    State(state): State<Arc<Shared>>,
    Json(change): Json<ToolToggle>,
) -> ApiResult<Json<Vec<ToolSummary>>> {
    let path = state.path.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let mut cfg = config::load_or_default_unvalidated(&path)?;
        match (change.group, change.tool) {
            (Some(group), None) if matches!(group.as_str(), "jadx" | "tailcat") => {
                cfg.tool_overrides
                    .retain(|name, _| !crate::tools::tool_in_group(name, &group));
                cfg.tool_groups.insert(group, change.enabled);
            }
            (None, Some(tool)) if crate::tools::optional_tool_names().contains(&tool.as_str()) => {
                cfg.tool_overrides.insert(tool, change.enabled);
            }
            _ => anyhow::bail!("specify one known optional tool or group"),
        }
        cfg.validate_runtime()?;
        config::save_config(&path, &cfg)
    })
    .await??;
    get_tools(State(state)).await
}

#[derive(Serialize)]
struct InstalledApp {
    package: String,
}

async fn get_apps(State(state): State<Arc<Shared>>) -> ApiResult<Json<Vec<InstalledApp>>> {
    let path = state.path.clone();
    let mut cfg =
        tokio::task::spawn_blocking(move || config::load_or_default_unvalidated(&path)).await??;
    cfg.execute_user_mode = ExecuteUserMode::Normal;
    cfg.enable_pty = false;
    cfg.execute_timeout_secs = cfg.execute_timeout_secs.clamp(1, 15);
    let executor = ShellExecutor::new(cfg);
    let output = list_android_apps(
        &executor,
        &ListAndroidAppsArgs {
            scope: None,
            limit: Some(500),
        },
    )
    .await?;
    let value: serde_json::Value = serde_json::from_str(&output)?;
    if value["status"] == "failed" || value["status"] == "timed_out" {
        return Err(ApiError::bad(anyhow!("cannot list Android applications")));
    }
    let apps = value["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["package"].as_str())
        .map(|package| InstalledApp {
            package: package.to_owned(),
        })
        .collect();
    Ok(Json(apps))
}

#[derive(Deserialize)]
struct MemorySetRequest {
    key: String,
    value: String,
}

#[derive(Deserialize)]
struct MemoryKeyRequest {
    key: String,
}

async fn get_memory(State(state): State<Arc<Shared>>) -> ApiResult<Json<Vec<MemoryEntry>>> {
    memory_entries(state.path.clone()).await
}

async fn set_memory(
    State(state): State<Arc<Shared>>,
    Json(request): Json<MemorySetRequest>,
) -> ApiResult<Json<Vec<MemoryEntry>>> {
    let path = state.path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        AgentMemory::open(&path)?.apply(&AgentMemoryArgs {
            action: AgentMemoryAction::Set,
            key: Some(request.key),
            value: Some(request.value),
        })?;
        Ok(())
    })
    .await??;
    memory_entries(state.path.clone()).await
}

async fn delete_memory(
    State(state): State<Arc<Shared>>,
    Json(request): Json<MemoryKeyRequest>,
) -> ApiResult<Json<Vec<MemoryEntry>>> {
    let path = state.path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        AgentMemory::open(&path)?.apply(&AgentMemoryArgs {
            action: AgentMemoryAction::Delete,
            key: Some(request.key),
            value: None,
        })?;
        Ok(())
    })
    .await??;
    memory_entries(state.path.clone()).await
}

async fn clear_memory(State(state): State<Arc<Shared>>) -> ApiResult<Json<Vec<MemoryEntry>>> {
    let path = state.path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        AgentMemory::open(&path)?.apply(&AgentMemoryArgs {
            action: AgentMemoryAction::Clear,
            key: None,
            value: None,
        })?;
        Ok(())
    })
    .await??;
    memory_entries(state.path.clone()).await
}

async fn memory_entries(path: PathBuf) -> ApiResult<Json<Vec<MemoryEntry>>> {
    Ok(Json(
        tokio::task::spawn_blocking(move || AgentMemory::open(&path)?.entries()).await??,
    ))
}

#[derive(Deserialize)]
struct FileSuggestionQuery {
    fragment: String,
}

async fn get_file_suggestions(
    Query(query): Query<FileSuggestionQuery>,
) -> ApiResult<Json<Vec<String>>> {
    if query.fragment.len() > 1024 || query.fragment.chars().any(char::is_whitespace) {
        return Err(ApiError::bad(anyhow!("invalid file suggestion fragment")));
    }
    let suggestions =
        tokio::task::spawn_blocking(move || file_suggestions(&query.fragment)).await?;
    Ok(Json(suggestions))
}

#[derive(Deserialize)]
struct FilePathQuery {
    path: String,
}

#[derive(Serialize)]
struct BrowserFile {
    name: String,
    path: String,
    is_dir: bool,
    size: Option<u64>,
    modified_ms: Option<u64>,
    preview_kind: Option<&'static str>,
}

fn preview_type(path: &Path) -> Option<(&'static str, &'static str, u64)> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if matches!(
        name.as_str(),
        "readme" | "license" | "makefile" | "dockerfile" | ".gitignore" | ".env.example"
    ) {
        return Some(("text", "text/plain; charset=utf-8", MAX_FILE_PREVIEW_BYTES));
    }
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let (kind, mime, limit) = match extension.as_str() {
        "png" => ("image", "image/png", MAX_IMAGE_PREVIEW_BYTES),
        "jpg" | "jpeg" => ("image", "image/jpeg", MAX_IMAGE_PREVIEW_BYTES),
        "gif" => ("image", "image/gif", MAX_IMAGE_PREVIEW_BYTES),
        "webp" => ("image", "image/webp", MAX_IMAGE_PREVIEW_BYTES),
        "bmp" => ("image", "image/bmp", MAX_IMAGE_PREVIEW_BYTES),
        "avif" => ("image", "image/avif", MAX_IMAGE_PREVIEW_BYTES),
        "mp4" | "m4v" => ("video", "video/mp4", MAX_VIDEO_PREVIEW_BYTES),
        "webm" => ("video", "video/webm", MAX_VIDEO_PREVIEW_BYTES),
        "ogv" => ("video", "video/ogg", MAX_VIDEO_PREVIEW_BYTES),
        "mov" => ("video", "video/quicktime", MAX_VIDEO_PREVIEW_BYTES),
        "wav" => ("audio", "audio/wav", MAX_AUDIO_PREVIEW_BYTES),
        "pcm" | "raw" => (
            "audio",
            "application/octet-stream",
            MAX_RAW_PCM_PREVIEW_BYTES,
        ),
        "mp3" => ("audio", "audio/mpeg", MAX_AUDIO_PREVIEW_BYTES),
        "m4a" | "aac" => ("audio", "audio/mp4", MAX_AUDIO_PREVIEW_BYTES),
        "ogg" | "oga" => ("audio", "audio/ogg", MAX_AUDIO_PREVIEW_BYTES),
        "flac" => ("audio", "audio/flac", MAX_AUDIO_PREVIEW_BYTES),
        "txt" | "log" | "md" | "markdown" | "rs" | "py" | "js" | "jsx" | "ts" | "tsx" | "json"
        | "toml" | "yaml" | "yml" | "xml" | "css" | "html" | "htm" | "sh" | "bash" | "zsh"
        | "java" | "kt" | "kts" | "c" | "h" | "cpp" | "hpp" | "go" | "sql" | "ini" | "conf"
        | "properties" | "gradle" | "gitignore" | "dockerfile" => {
            ("text", "text/plain; charset=utf-8", MAX_FILE_PREVIEW_BYTES)
        }
        _ => return None,
    };
    Some((kind, mime, limit))
}

fn checked_browser_path(path: &str) -> ApiResult<PathBuf> {
    if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
        return Err(ApiError::bad(anyhow!("invalid file path")));
    }
    if path == "~" || path.starts_with("~/") {
        let home = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .ok_or_else(|| ApiError::bad(anyhow!("home directory unavailable")))?;
        Ok(PathBuf::from(home).join(path.trim_start_matches('~').trim_start_matches('/')))
    } else {
        Ok(PathBuf::from(path))
    }
}

async fn get_files(Query(query): Query<FilePathQuery>) -> ApiResult<Json<Vec<BrowserFile>>> {
    let directory = checked_browser_path(&query.path)?;
    let files = tokio::task::spawn_blocking(move || -> Result<Vec<BrowserFile>> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&directory)
            .context("cannot read directory")?
            .take(MAX_DIRECTORY_ENTRIES)
        {
            let entry = entry.context("cannot read directory entry")?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let is_dir = metadata.is_dir();
            let path = entry.path();
            let Some(path) = path.to_str().map(str::to_owned) else {
                continue;
            };
            let preview_kind = (!is_dir && metadata.is_file())
                .then(|| preview_type(Path::new(&path)))
                .flatten()
                .and_then(|(kind, _, limit)| (metadata.len() <= limit).then_some(kind));
            let modified_ms = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .and_then(|duration| u64::try_from(duration.as_millis()).ok());
            files.push(BrowserFile {
                name,
                path,
                is_dir,
                size: (!is_dir).then_some(metadata.len()),
                modified_ms,
                preview_kind,
            });
        }
        files.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(files)
    })
    .await??;
    Ok(Json(files))
}

fn requested_range(value: &str, length: u64) -> Option<(u64, u64)> {
    let value = value.strip_prefix("bytes=")?;
    if value.contains(',') || length == 0 {
        return None;
    }
    let (start, end) = value.split_once('-')?;
    if start.is_empty() {
        let suffix = end.parse::<u64>().ok()?;
        if suffix == 0 {
            return None;
        }
        return Some((length.saturating_sub(suffix), length - 1));
    }
    let start = start.parse::<u64>().ok()?;
    let end = if end.is_empty() {
        length - 1
    } else {
        end.parse::<u64>().ok()?.min(length - 1)
    };
    (start < length && start <= end).then_some((start, end))
}

async fn get_file_preview(
    Query(query): Query<FilePathQuery>,
    headers: axum::http::HeaderMap,
) -> ApiResult<Response> {
    let path = checked_browser_path(&query.path)?;
    let (_, mime, limit) =
        preview_type(&path).ok_or_else(|| ApiError::bad(anyhow!("unsupported preview type")))?;
    let mut file = tokio::fs::File::open(&path)
        .await
        .context("cannot open preview file")?;
    let metadata = file
        .metadata()
        .await
        .context("cannot inspect preview file")?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(ApiError::bad(anyhow!(
            "preview file is unavailable or too large"
        )));
    }
    let length = metadata.len();
    let range_header = headers.get(header::RANGE);
    let (start, end, status) = if let Some(value) = range_header {
        let range = value
            .to_str()
            .ok()
            .and_then(|value| requested_range(value, length));
        let Some((start, end)) = range else {
            return Ok(Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{length}"))
                .body(Body::empty())
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()));
        };
        (start, end, StatusCode::PARTIAL_CONTENT)
    } else {
        (0, length.saturating_sub(1), StatusCode::OK)
    };
    file.seek(std::io::SeekFrom::Start(start))
        .await
        .context("cannot seek preview file")?;
    let bytes = if length == 0 { 0 } else { end - start + 1 };
    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, bytes.to_string())
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_DISPOSITION, "inline")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    if status == StatusCode::PARTIAL_CONTENT {
        response = response.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{length}"),
        );
    }
    Ok(response
        .body(Body::from_stream(ReaderStream::new(file.take(bytes))))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}

async fn new_session(State(state): State<Arc<Shared>>) -> ApiResult<Json<SessionSummary>> {
    let mut sessions = state
        .sessions
        .lock()
        .map_err(|_| anyhow!("web session registry lock poisoned"))?;
    if sessions.len() >= MAX_WEB_SESSIONS {
        return Err(ApiError::conflict("web session limit reached"));
    }
    let current = web_session(SessionState::empty());
    let inner = current
        .inner
        .lock()
        .map_err(|_| anyhow!("web session lock poisoned"))?;
    let summary = SessionSummary {
        id: inner.id.clone(),
        title: inner.title.clone(),
        turns: 0,
        busy: false,
        pending: false,
        created: inner.created,
        updated: inner.updated,
    };
    let id = inner.id.clone();
    drop(inner);
    sessions.insert(id, current);
    Ok(Json(summary))
}

async fn get_sessions(State(state): State<Arc<Shared>>) -> ApiResult<Json<Vec<SessionSummary>>> {
    tokio::task::spawn_blocking(move || -> ApiResult<Json<Vec<SessionSummary>>> {
        let sessions = state
            .sessions
            .lock()
            .map_err(|_| anyhow!("web session registry lock poisoned"))?;
        let mut all: Vec<_> = sessions
            .values()
            .filter_map(|session| {
                session.inner.lock().ok().map(|inner| SessionSummary {
                    id: inner.id.clone(),
                    title: inner.title.clone(),
                    turns: inner.accepted_turns,
                    busy: inner.busy,
                    pending: inner.pending.is_some(),
                    created: inner.created,
                    updated: inner.updated,
                })
            })
            .collect();
        let stored = SessionStore::open(&state.path)?.list()?;
        for saved in stored {
            if !all.iter().any(|session| session.id == saved.name) {
                all.push(SessionSummary {
                    id: saved.name,
                    title: saved.title,
                    turns: saved.turns,
                    busy: false,
                    pending: false,
                    created: saved.created_unix_secs,
                    updated: saved.updated_unix_secs,
                });
            }
        }
        all.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| b.id.cmp(&a.id)));
        Ok(Json(all))
    })
    .await?
}

async fn load_session(
    State(state): State<Arc<Shared>>,
    Json(selection): Json<SessionSelection>,
) -> ApiResult<Json<SessionSummary>> {
    tokio::task::spawn_blocking(move || -> ApiResult<Json<SessionSummary>> {
        let mut sessions = state
            .sessions
            .lock()
            .map_err(|_| anyhow!("web session registry lock poisoned"))?;
        if let Some(existing) = sessions.get(&selection.name) {
            let inner = existing
                .inner
                .lock()
                .map_err(|_| anyhow!("web session lock poisoned"))?;
            return Ok(Json(SessionSummary {
                id: inner.id.clone(),
                title: inner.title.clone(),
                turns: inner.accepted_turns,
                busy: inner.busy,
                pending: inner.pending.is_some(),
                created: inner.created,
                updated: inner.updated,
            }));
        }
        let cfg = config::load_or_default_unvalidated(&state.path)?;
        let store = SessionStore::open(&state.path)?;
        let info = store
            .list()?
            .into_iter()
            .find(|item| item.name == selection.name)
            .context("saved session not found")?;
        let saved_turns =
            store.load(&selection.name, usize::MAX, cfg.model_tool_output_max_bytes)?;
        let turns = saved_turns[saved_turns.len().saturating_sub(cfg.max_context_turns)..].to_vec();
        let mut checkpoint = store.load_web_checkpoint(&selection.name)?;
        let display = store.load_web_presentation(&selection.name)?;
        let mut history = display
            .as_ref()
            .map_or_else(|| render_turns(&turns), |value| value.history.clone());
        if let Some(current) = checkpoint.as_mut() {
            if current.status != "completed" {
                mark_unresolved_tools(&mut current.history);
            }
            if current.status == "running" {
                current.status = "interrupted".into();
                current.events.push(WebCheckpointEvent {
                    at: unix_seconds(),
                    kind: "interrupted_after_restart".into(),
                    tool: None,
                });
                current
                    .history
                    .push("⏹ 上次任务在服务重启时中断；以下结果仅供核对，不会自动重试".into());
                store.save_web_state(
                    &selection.name,
                    &info.title,
                    &saved_turns,
                    cfg.model_tool_output_max_bytes,
                    &[],
                    Some(current),
                )?;
            }
            if current.status != "completed" {
                if display.is_some() {
                    mark_unresolved_tools(&mut history);
                    if current.status == "interrupted" {
                        history.push("⏹ 上次任务已中断，不会自动重试".into());
                    }
                } else {
                    history.extend(current.history.clone());
                }
            }
        }
        let current = web_session(SessionState {
            id: info.name.clone(),
            title: info.title.clone(),
            history,
            turns,
            busy: false,
            pending: None,
            reply: None,
            title_generated: info.title != "新会话",
            created: info.created_unix_secs,
            updated: info.updated_unix_secs,
            steps: display.as_ref().map_or_else(
                || {
                    saved_turns.last().map_or(0, |turn| {
                        turn.iter()
                            .filter(|item| matches!(item, ConversationItem::Tools(_)))
                            .count()
                            + 1
                    })
                },
                |value| value.task.steps,
            ),
            tool_calls: display.as_ref().map_or_else(
                || {
                    saved_turns
                        .last()
                        .map_or(0, |turn| turns_tool_count(std::slice::from_ref(turn)))
                },
                |value| value.task.tool_calls,
            ),
            accepted_turns: display.as_ref().map_or(info.turns, |value| value.turns),
            task: display
                .as_ref()
                .map_or_else(WebTaskMetrics::default, |value| value.task.clone()),
            task_started: None,
            phase_started: Instant::now(),
            token_base: (0, 0),
            input_tokens: display.as_ref().map_or(0, |value| value.input_tokens),
            output_tokens: display.as_ref().map_or(0, |value| value.output_tokens),
            final_input_tokens: display.as_ref().and_then(|value| value.final_input_tokens),
            activity: "idle",
            activity_detail: None,
            activity_since_ms: unix_millis(),
            checkpoint,
            checkpoint_start: None,
            redaction_secrets: Vec::new(),
        });
        sessions.insert(info.name.clone(), current);
        Ok(Json(SessionSummary {
            id: info.name,
            title: info.title,
            turns: info.turns,
            busy: false,
            pending: false,
            created: info.created_unix_secs,
            updated: info.updated_unix_secs,
        }))
    })
    .await?
}

async fn delete_session(
    State(state): State<Arc<Shared>>,
    Json(selection): Json<SessionSelection>,
) -> ApiResult<String> {
    let path = state.path.clone();
    tokio::task::spawn_blocking(move || -> ApiResult<String> {
        let mut sessions = state
            .sessions
            .lock()
            .map_err(|_| anyhow!("web session registry lock poisoned"))?;
        if let Some(current) = sessions.get(&selection.name) {
            let inner = current
                .inner
                .lock()
                .map_err(|_| anyhow!("web session lock poisoned"))?;
            if inner.busy
                || inner.pending.is_some()
                || current.terminal_clients.load(Ordering::Relaxed) > 0
            {
                return Err(ApiError::conflict(
                    "cannot delete a running, pending, or terminal-connected session",
                ));
            }
        }
        let store = SessionStore::open(&path)?;
        match store.delete(&selection.name) {
            Ok(()) => {}
            Err(error)
                if sessions.contains_key(&selection.name)
                    && error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) => {}
            Err(error) => return Err(error.into()),
        }
        sessions.remove(&selection.name);
        Ok("会话已删除".into())
    })
    .await?
}

async fn delete_all_sessions(State(state): State<Arc<Shared>>) -> ApiResult<String> {
    let path = state.path.clone();
    tokio::task::spawn_blocking(move || -> ApiResult<String> {
        let mut sessions = state
            .sessions
            .lock()
            .map_err(|_| anyhow!("web session registry lock poisoned"))?;
        for current in sessions.values() {
            let inner = current
                .inner
                .lock()
                .map_err(|_| anyhow!("web session lock poisoned"))?;
            if inner.busy
                || inner.pending.is_some()
                || current.terminal_clients.load(Ordering::Relaxed) > 0
            {
                return Err(ApiError::conflict(
                    "cannot delete all sessions while one is running, pending, or terminal-connected",
                ));
            }
        }
        SessionStore::open(&path)?.delete_all()?;
        sessions.clear();
        Ok("所有会话已删除".into())
    })
    .await?
}

async fn post_message(
    State(state): State<Arc<Shared>>,
    Json(message): Json<Message>,
) -> ApiResult<(StatusCode, String)> {
    let text = message.text.trim().to_owned();
    if text.is_empty() || text.len() > 16 * 1024 {
        return Err(ApiError::bad(anyhow!("message must contain 1–16384 bytes")));
    }
    start_message(state, &message.session_id, text).await
}

async fn retry_message(
    State(state): State<Arc<Shared>>,
    Json(selection): Json<SessionSelection>,
) -> ApiResult<(StatusCode, String)> {
    let current = session(&state, &selection.name)?;
    let text = {
        let inner = current
            .inner
            .lock()
            .map_err(|_| anyhow!("web session lock poisoned"))?;
        let Some(text) = retryable_input(&inner) else {
            return Err(ApiError::conflict(
                "this session has no failed task available to retry",
            ));
        };
        text
    };
    start_message(state, &selection.name, text).await
}

async fn start_message(
    state: Arc<Shared>,
    session_id: &str,
    text: String,
) -> ApiResult<(StatusCode, String)> {
    let path = state.path.clone();
    let cfg =
        tokio::task::spawn_blocking(move || config::load_or_default_unvalidated(&path)).await??;
    let current = {
        let sessions = state
            .sessions
            .lock()
            .map_err(|_| anyhow!("web session registry lock poisoned"))?;
        let current = sessions
            .get(session_id)
            .cloned()
            .context("web session not found")?;
        let mut inner = current
            .inner
            .lock()
            .map_err(|_| anyhow!("web session lock poisoned"))?;
        if inner.busy || inner.pending.is_some() {
            return Err(ApiError::conflict("this Agent session is already running"));
        }
        inner.busy = true;
        inner.accepted_turns = inner.accepted_turns.saturating_add(1);
        inner.steps = 0;
        inner.tool_calls = 0;
        inner.task = WebTaskMetrics {
            timing_available: true,
            ..WebTaskMetrics::default()
        };
        inner.task_started = Some(Instant::now());
        inner.phase_started = Instant::now();
        inner.token_base = (inner.input_tokens, inner.output_tokens);
        remember_redaction_secrets(&mut inner, &cfg);
        current.cancel.send_replace(false);
        set_activity(&mut inner, "idle", None);
        inner.updated = unix_seconds();
        inner.checkpoint_start = Some(inner.history.len());
        push(&mut inner, format!("> {text}"));
        inner.checkpoint = Some(WebCheckpoint {
            status: "running".into(),
            activity: "thinking".into(),
            history: Vec::new(),
            events: Vec::new(),
        });
        checkpoint_event(&mut inner, "input_accepted", None);
        drop(inner);
        current
    };
    if let Err(error) = start_audit(state.path.clone(), &cfg, &current).await {
        eprintln!("cannot open Web audit log: {error:#}");
    }
    audit(&current, "web_user", &text);
    if let Err(error) = persist_web_session(&state, &current).await {
        if let Ok(mut inner) = current.inner.lock() {
            inner.busy = false;
            inner.checkpoint = None;
            inner.checkpoint_start = None;
            finish_task(&mut inner);
            push(&mut inner, format!("❌ 无法保存会话检查点：{error:#}"));
        }
        return Err(error.into());
    }
    notify(&current, "state");
    tokio::spawn(run_message(state, current, text));
    Ok((StatusCode::ACCEPTED, "任务已开始".into()))
}

fn retryable_input(inner: &SessionState) -> Option<String> {
    let checkpoint = inner.checkpoint.as_ref()?;
    if !matches!(
        checkpoint.status.as_str(),
        "failed" | "interrupted" | "cancelled"
    ) {
        return None;
    }
    let history = if checkpoint.history.is_empty() {
        let start = inner.checkpoint_start.unwrap_or(0);
        inner.history.get(start..)?
    } else {
        checkpoint.history.as_slice()
    };
    history.iter().find_map(|line| {
        let text = line.strip_prefix("> ")?.trim();
        (!text.is_empty() && !text.contains("[NL2SH CREDENTIAL REDACTED]")).then(|| text.to_owned())
    })
}

async fn cancel_message(
    State(state): State<Arc<Shared>>,
    Json(message): Json<SessionSelection>,
) -> ApiResult<String> {
    let current = session(&state, &message.name)?;
    {
        let mut inner = current
            .inner
            .lock()
            .map_err(|_| anyhow!("web session lock poisoned"))?;
        if !inner.busy {
            return Err(ApiError::conflict("this Agent session is not running"));
        }
        current.cancel.send_replace(true);
        if let Some(reply) = inner.reply.take() {
            let response = match inner.pending.take() {
                Some(Pending::Approval { .. }) => Reply::Approval(ConfirmationDecision::Reject),
                _ => Reply::Questions(None),
            };
            let _ = reply.send(response);
        }
        set_activity(&mut inner, "cancelling", None);
        checkpoint_event(&mut inner, "cancel_requested", None);
    }
    if let Err(error) = persist_web_session(&state, &current).await {
        eprintln!("cannot save Web cancellation checkpoint: {error:#}");
    }
    notify(&current, "cancelling");
    Ok("正在取消".into())
}

async fn post_decision(
    State(state): State<Arc<Shared>>,
    Json(decision): Json<Decision>,
) -> ApiResult<String> {
    let current = session(&state, &decision.session_id)?;
    let mut inner = current
        .inner
        .lock()
        .map_err(|_| anyhow!("web session lock poisoned"))?;
    let pending = inner.pending.as_mut().context("no pending request")?;
    let pending_id = match pending {
        Pending::Approval { request_id, .. } | Pending::Questions { request_id, .. } => *request_id,
    };
    if pending_id != decision.request_id {
        return Err(ApiError::bad(anyhow!("approval request changed")));
    }
    if let Pending::Approval {
        strong: true,
        armed,
        ..
    } = pending
    {
        if decision.action == "arm" {
            *armed = true;
            drop(inner);
            audit(&current, "web_decision", "arm");
            notify(&current, "state");
            return Ok("请再次确认高风险操作".into());
        }
    }
    let reply = match pending {
        Pending::Approval {
            strong,
            root,
            armed,
            ..
        } => Reply::Approval(match decision.action.as_str() {
            "approve" if !*strong || *armed => ConfirmationDecision::ApproveCaptured,
            "remember" if !*strong && !*root => ConfirmationDecision::ApproveForTask,
            "reject" => ConfirmationDecision::Reject,
            "edit" => ConfirmationDecision::Edit(decision.text.unwrap_or_default()),
            _ => {
                return Err(ApiError::bad(anyhow!(
                    "invalid decision or strong confirmation missing"
                )))
            }
        }),
        Pending::Questions { .. } => Reply::Questions(if decision.action == "answer" {
            decision.answers
        } else {
            None
        }),
    };
    inner.pending = None;
    set_activity(&mut inner, "tool", None);
    inner
        .reply
        .take()
        .context("pending request has no receiver")?
        .send(reply)
        .map_err(|_| anyhow!("request no longer active"))?;
    drop(inner);
    audit(&current, "web_decision", &decision.action);
    notify(&current, "state");
    Ok("已提交".into())
}

async fn session_events(
    State(state): State<Arc<Shared>>,
    Query(query): Query<StateQuery>,
) -> ApiResult<
    Sse<impl tokio_stream::Stream<Item = std::result::Result<Event, std::convert::Infallible>>>,
> {
    let current = session(&state, &query.id)?;
    let updates = BroadcastStream::new(current.events.subscribe())
        .filter_map(|item| item.ok())
        .map(|data| Ok(Event::default().data(data)));
    let stream = tokio_stream::once(Ok(Event::default().data("connected"))).chain(updates);
    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}

#[derive(Deserialize)]
struct TerminalCommand {
    command: String,
}
async fn terminal_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<Shared>>,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<impl IntoResponse> {
    let current = session(&state, &id)?;
    Ok(ws.on_upgrade(move |socket| terminal_socket(socket, state, current)))
}
async fn terminal_socket(mut socket: WebSocket, state: Arc<Shared>, current: Arc<WebSession>) {
    current.terminal_clients.fetch_add(1, Ordering::Relaxed);
    let _terminal_client = TerminalClientGuard(current.clone());
    while let Some(Ok(message)) = socket.recv().await {
        let WsMessage::Text(text) = message else {
            continue;
        };
        let response = match serde_json::from_str::<TerminalCommand>(&text) {
            Ok(request) => run_terminal_command(&state, &current, request.command)
                .await
                .unwrap_or_else(|e| format!("error: {e:#}")),
            Err(e) => format!("error: invalid terminal message: {e}"),
        };
        if socket.send(WsMessage::Text(response.into())).await.is_err() {
            break;
        }
    }
}
async fn run_terminal_command(
    state: &Shared,
    current: &Arc<WebSession>,
    mut command: String,
) -> Result<String> {
    if command.trim().is_empty() || command.len() > 16 * 1024 {
        bail!("command must contain 1–16384 bytes")
    }
    let cfg = load_config(state.path.clone()).await?;
    let confirmer = WebConfirmer {
        session: current.clone(),
        state: None,
    };
    let mut assessment = crate::security::assess(&command, &cfg);
    let mut approved_command = None;
    while assessment.requires_confirmation {
        match confirmer.confirm(&command, &assessment).await? {
            ConfirmationDecision::Approve
            | ConfirmationDecision::ApproveCaptured
            | ConfirmationDecision::ApproveInteractive
            | ConfirmationDecision::ApproveForTask
            | ConfirmationDecision::ApproveForRun => {
                approved_command = Some(command.clone());
                break;
            }
            ConfirmationDecision::Reject => bail!("command rejected"),
            ConfirmationDecision::Edit(edited) => {
                command = edited;
                assessment = crate::security::assess(&command, &cfg)
            }
        }
    }
    let capability =
        PrivilegeBroker::authorize(&command, &assessment, &cfg, approved_command.as_deref())?;
    let output = Arc::new(WebOutput {
        session: current.clone(),
    });
    let executor = CapturedExecutor(ShellExecutor::new(cfg).with_output(output));
    let result = ExecutionBroker::execute(&executor, capability, false).await?;
    Ok(serde_json::json!({"stdout":result.stdout,"stderr":result.stderr,"exit_code":result.exit_code,"timed_out":result.timed_out,"interrupted":result.interrupted}).to_string())
}

fn append_tool_round(lines: &mut Vec<String>, round: &ToolRound) {
    let mut remaining = round.results.iter().collect::<Vec<_>>();
    for call in &round.calls {
        lines.push(format!("🔧 {}", call.name));
        if let Some(index) = remaining
            .iter()
            .position(|result| result.call_id == call.id)
        {
            let result = remaining.remove(index);
            lines.push(format!(
                "{}{}",
                if result.success {
                    "\u{1e}TOOL_OK:"
                } else {
                    "\u{1e}TOOL_ERR:"
                },
                result.output
            ));
        }
    }
    for result in remaining {
        lines.push(format!(
            "{}{}",
            if result.success {
                "\u{1e}TOOL_OK:"
            } else {
                "\u{1e}TOOL_ERR:"
            },
            result.output
        ));
    }
}

fn turns_tool_count(turns: &[Vec<ConversationItem>]) -> usize {
    turns
        .iter()
        .flatten()
        .map(|item| match item {
            ConversationItem::Tools(round) => round.calls.len(),
            _ => 0,
        })
        .sum()
}

fn render_turns(turns: &[Vec<ConversationItem>]) -> Vec<String> {
    let mut lines = Vec::new();
    for turn in turns {
        for item in turn {
            match item {
                ConversationItem::Message(message) => match message.role {
                    Role::User => lines.push(format!("> {}", message.content)),
                    Role::Assistant => lines.push(format!("🤖 {}", message.content)),
                    _ => {}
                },
                ConversationItem::Tools(round) => append_tool_round(&mut lines, round),
            }
        }
    }
    if lines.len() > MAX_HISTORY {
        lines.drain(..lines.len() - MAX_HISTORY);
    }
    lines
}

fn push(inner: &mut SessionState, line: String) {
    inner.history.push(line);
    if inner.history.len() > MAX_HISTORY {
        let excess = inner.history.len() - MAX_HISTORY;
        inner.history.drain(..excess);
        if let Some(start) = inner.checkpoint_start.as_mut() {
            *start = start.saturating_sub(excess);
        }
    }
}

fn clear_live_tool_output(inner: &mut SessionState) {
    inner
        .history
        .retain(|line| !line.starts_with("[OUT]") && !line.starts_with("[ERR]"));
}

fn mark_unresolved_tools(history: &mut [String]) {
    for line in history {
        if line.starts_with(LIVE_TOOL_PENDING_PREFIX) {
            *line = "\u{1e}TOOL_ERR:任务中断，工具结果未记录；执行状态未知".into();
        }
    }
}

async fn run_message(state: Arc<Shared>, current: Arc<WebSession>, text: String) {
    let result = run_agent(state.clone(), current.clone(), text).await;
    let failed = result.is_err();
    if let Err(error) = &result {
        WebTextSink {
            session: current.clone(),
            state: None,
        }
        .record_generation();
        audit(&current, "web_task_error", &format!("{error:#}"));
    }
    if let Ok(mut inner) = current.inner.lock() {
        clear_live_tool_output(&mut inner);
        if let Err(error) = result {
            mark_unresolved_tools(&mut inner.history);
            if *current.cancel.borrow() {
                push(&mut inner, "⏹ 任务已取消".into());
                if let Some(checkpoint) = inner.checkpoint.as_mut() {
                    checkpoint.status = "cancelled".into();
                }
                checkpoint_event(&mut inner, "cancelled", None);
            } else {
                push(&mut inner, format!("❌ {error:#}"));
                if let Some(checkpoint) = inner.checkpoint.as_mut() {
                    checkpoint.status = "failed".into();
                }
                checkpoint_event(&mut inner, "failed", None);
            }
        } else if *current.cancel.borrow() {
            push(&mut inner, "⏹ 停止请求已收到，已完成的结果已保留".into());
        }
    }
    if let Ok(mut inner) = current.inner.lock() {
        finish_task(&mut inner);
        if failed {
            let summary = task_summary(&inner);
            push(&mut inner, format!("\u{1e}TASK:{summary}"));
        }
    }
    let summary =
        current.inner.lock().ok().map(|inner| {
            serde_json::json!({"failed":failed,"task":task_metrics(&inner)}).to_string()
        });
    if let Some(summary) = summary {
        audit(&current, "web_task_finished", &summary);
    }
    flush_audit(&current).await;
    if failed {
        if let Err(error) = persist_web_session(&state, &current).await {
            eprintln!("cannot save failed Web task checkpoint: {error:#}");
        }
    }
    if let Ok(mut inner) = current.inner.lock() {
        inner.busy = false;
        inner.pending = None;
        inner.reply = None;
        set_activity(&mut inner, "idle", None);
    }
    notify(&current, "state");
}

async fn run_agent(state: Arc<Shared>, current: Arc<WebSession>, text: String) -> Result<()> {
    let path = state.path.clone();
    let cfg = tokio::task::spawn_blocking(move || -> Result<Config> {
        let cfg = config::load_or_default_unvalidated(&path)?;
        cfg.validate_runtime()?;
        Ok(cfg)
    })
    .await??;
    {
        let mut inner = current
            .inner
            .lock()
            .map_err(|_| anyhow!("web session lock poisoned"))?;
        remember_redaction_secrets(&mut inner, &cfg);
    }
    if !cfg.provider_is_configured() {
        return Err(anyhow!("请先在 Web 配置页填写模型服务设置"));
    }
    let llm = build_client(&cfg)?;
    let output = Arc::new(WebOutput {
        session: current.clone(),
    });
    let mut execution_config = cfg.clone();
    execution_config.enable_pty = false;
    let executor = CapturedExecutor(
        ShellExecutor::new(execution_config)
            .with_output(output)
            .with_cancel(current.cancel.subscribe()),
    );
    let confirmer = WebConfirmer {
        session: current.clone(),
        state: Some(state.clone()),
    };
    let runner = AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &executor,
        confirmer: &confirmer,
    };
    let (history, needs_title) = {
        let inner = current
            .inner
            .lock()
            .map_err(|_| anyhow!("web session lock poisoned"))?;
        (inner.turns.clone(), !inner.title_generated)
    };
    let sink = WebTextSink {
        session: current.clone(),
        state: Some(state.clone()),
    };
    let agent_text = tokio::task::spawn_blocking({
        let text = text.clone();
        move || augment_file_references(&text)
    })
    .await?;
    let result = runner
        .run_with_history_streaming_cancellable(
            agent_text,
            history,
            &sink,
            current.cancel.subscribe(),
        )
        .await?;
    let answer = result.final_text.clone();
    audit(&current, "web_assistant", &answer);
    {
        let mut inner = current
            .inner
            .lock()
            .map_err(|_| anyhow!("web session lock poisoned"))?;
        inner.history.retain(|line| !line.starts_with("… "));
        push(&mut inner, format!("🤖 {}", result.final_text));
        inner.turns.push(result.transcript);
        if inner.turns.len() > cfg.max_context_turns {
            inner.turns.remove(0);
        }
        if needs_title {
            inner.title_generated = true;
        }
        checkpoint_event(&mut inner, "answer_completed", None);
        if let Some(checkpoint) = inner.checkpoint.as_mut() {
            checkpoint.status = "completed".into();
        }
        inner.steps = result.steps;
        inner.tool_calls = result.tool_calls;
        inner.final_input_tokens = result.final_input_tokens;
        finish_task(&mut inner);
        let summary = task_summary(&inner);
        push(&mut inner, format!("\u{1e}TASK:{summary}"));
        inner.updated = unix_seconds();
    }
    notify(&current, "state");
    persist_web_session(&state, &current).await?;
    {
        let mut inner = current
            .inner
            .lock()
            .map_err(|_| anyhow!("web session lock poisoned"))?;
        inner.checkpoint = None;
        inner.checkpoint_start = None;
    }
    if let Err(error) = persist_web_session(&state, &current).await {
        eprintln!("cannot clear completed Web checkpoint: {error:#}");
    }
    if let Ok(mut inner) = current.inner.lock() {
        set_activity(&mut inner, "idle", None);
    }
    notify(&current, "state");
    if needs_title && !*current.cancel.borrow() {
        tokio::spawn(generate_title_after_reply(
            state, current, llm, cfg, text, answer,
        ));
    }
    Ok(())
}

async fn generate_title_after_reply(
    state: Arc<Shared>,
    current: Arc<WebSession>,
    llm: impl LlmClient,
    cfg: Config,
    input: String,
    answer: String,
) {
    let Some(title) = generate_title(&llm, &cfg, &input, &answer).await else {
        return;
    };
    let _write = current.persist.lock().await;
    let path = state.path.clone();
    let registered_current = current.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<()> {
        let sessions = state
            .sessions
            .lock()
            .map_err(|_| anyhow!("web session registry lock poisoned"))?;
        let Some(registered) = sessions
            .values()
            .find(|session| Arc::ptr_eq(session, &registered_current))
        else {
            return Ok(());
        };
        let (id, turns, checkpoint, prior_secrets) = {
            let inner = registered
                .inner
                .lock()
                .map_err(|_| anyhow!("web session lock poisoned"))?;
            let mut checkpoint = inner.checkpoint.clone();
            if let Some(value) = checkpoint.as_mut() {
                value.activity = inner.activity.into();
                value.history = inner.history[inner.checkpoint_start.unwrap_or(0)..].to_vec();
            }
            (
                inner.id.clone(),
                inner.turns.clone(),
                checkpoint,
                inner.redaction_secrets.clone(),
            )
        };
        let cfg = config::load_or_default_unvalidated(&path)?;
        let mut secrets = vec![
            cfg.api_key,
            cfg.proxy_password,
            cfg.ima_client_id,
            cfg.ima_api_key,
            cfg.jev_api_key,
        ];
        secrets.extend(prior_secrets);
        SessionStore::open(&path)?.save_web_state(
            &id,
            &title,
            &turns,
            cfg.model_tool_output_max_bytes,
            &secrets,
            checkpoint.as_ref(),
        )?;
        if let Ok(mut inner) = registered.inner.lock() {
            inner.title = title;
            inner.updated = unix_seconds();
        }
        notify(registered, "state");
        Ok(())
    })
    .await;
    if let Err(error) = result {
        eprintln!("cannot finish Web title update: {error:#}");
    } else if let Ok(Err(error)) = result {
        eprintln!("cannot save Web title: {error:#}");
    }
}

async fn wait_web_cancel(mut receiver: watch::Receiver<bool>) {
    while !*receiver.borrow() {
        if receiver.changed().await.is_err() {
            return;
        }
    }
}

// Browser clients cannot drive a terminal PTY. Keep every web execution in
// the captured pipeline while preserving the runner's assessment and approval.
struct CapturedExecutor(ShellExecutor);

#[async_trait]
impl CommandExecutor for CapturedExecutor {
    async fn runtime_context(&self) -> Result<Option<String>> {
        self.0.runtime_context().await
    }

    async fn execute(
        &self,
        command: &str,
        needs_root: bool,
        _interactive: bool,
    ) -> Result<ExecutionResult> {
        self.0.execute(command, needs_root, false).await
    }

    async fn execute_quiet(
        &self,
        command: &str,
        needs_root: bool,
        _interactive: bool,
    ) -> Result<ExecutionResult> {
        self.0.execute_quiet(command, needs_root, false).await
    }
}

struct WebOutput {
    session: Arc<WebSession>,
}
impl OutputSink for WebOutput {
    fn stdout(&self, text: &str) {
        self.add("[OUT]", text);
    }
    fn stderr(&self, text: &str) {
        self.add("[ERR]", text);
    }
}
impl WebOutput {
    fn add(&self, prefix: &str, text: &str) {
        if let Ok(mut inner) = self.session.inner.lock() {
            push(
                &mut inner,
                format!("{prefix} {}", crate::limits::truncate_text(text, 16 * 1024)),
            );
        }
        notify(&self.session, "output");
    }
}

struct WebTextSink {
    session: Arc<WebSession>,
    state: Option<Arc<Shared>>,
}

impl WebTextSink {
    fn record_generation(&self) {
        let lines = self
            .session
            .inner
            .lock()
            .ok()
            .map(|inner| {
                inner
                    .history
                    .iter()
                    .rev()
                    .take_while(|line| line.starts_with("… ") || line.starts_with("\u{1e}REASON:"))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for line in lines {
            audit(&self.session, "web_model_output", &line);
        }
        self.schedule_checkpoint();
    }

    fn schedule_checkpoint(&self) {
        if let Some(state) = &self.state {
            let state = state.clone();
            let session = self.session.clone();
            tokio::spawn(async move {
                if let Err(error) = persist_web_session(&state, &session).await {
                    eprintln!("cannot save Web tool checkpoint: {error:#}");
                }
            });
        }
    }
}

impl TextDeltaSink for WebTextSink {
    fn agent_progress(&self, steps: usize, tools: usize, usage: &crate::llm::Usage) {
        if let Ok(mut inner) = self.session.inner.lock() {
            inner.steps = steps;
            inner.tool_calls = tools;
            inner.input_tokens = inner
                .token_base
                .0
                .saturating_add(usage.input_tokens.unwrap_or(0));
            inner.output_tokens = inner
                .token_base
                .1
                .saturating_add(usage.output_tokens.unwrap_or(0));
        }
        notify(&self.session, "progress");
    }

    fn begin(&self) {
        if let Ok(mut inner) = self.session.inner.lock() {
            // Keep previous tool-step commentary visible and stop merging generations.
            if let Some(line) = inner
                .history
                .last_mut()
                .filter(|line| line.starts_with("… "))
            {
                *line = format!("🤖 {}", &line["… ".len()..]);
            }
        }
    }

    fn reasoning_delta(&self, text: &str) {
        if let Ok(mut inner) = self.session.inner.lock() {
            let bounded = crate::limits::truncate_text(text, 4096);
            if let Some(line) = inner
                .history
                .last_mut()
                .filter(|line| line.starts_with("\u{1e}REASON:"))
            {
                line.push_str(&bounded);
                *line = crate::limits::truncate_text(line, 16 * 1024);
            } else {
                push(&mut inner, format!("\u{1e}REASON:{bounded}"));
            }
        }
        notify(&self.session, "reasoning");
    }

    fn end(&self, completed: bool) {
        if completed {
            self.record_generation();
        }
    }

    fn agent_activity(&self, activity: &'static str, detail: Option<&str>) {
        let mut changed = false;
        if let Ok(mut inner) = self.session.inner.lock() {
            if !*self.session.cancel.borrow() {
                changed = inner.activity != activity || inner.activity_detail.as_deref() != detail;
                set_activity(&mut inner, activity, detail);
                if changed {
                    checkpoint_event(&mut inner, activity, detail);
                }
            }
        }
        notify(&self.session, "activity");
        if changed {
            audit(&self.session, "web_activity", activity);
            self.schedule_checkpoint();
        }
    }

    fn delta(&self, text: &str) {
        if let Ok(mut inner) = self.session.inner.lock() {
            let bounded = crate::limits::truncate_text(text, 4096);
            if let Some(current) = inner
                .history
                .last_mut()
                .filter(|line| line.starts_with("… "))
            {
                current.push_str(&bounded);
                if current.len() > 16 * 1024 {
                    *current = crate::limits::truncate_text(current, 16 * 1024);
                }
            } else {
                push(&mut inner, format!("… {bounded}"));
            }
        }
        notify(&self.session, "delta");
    }

    fn tool_requested(&self, call: &crate::llm::ToolCall) {
        audit(
            &self.session,
            "web_tool_requested",
            &serde_json::json!(call).to_string(),
        );
    }

    fn tool_started(&self, call_id: &str, name: &str) {
        if let Ok(mut inner) = self.session.inner.lock() {
            if let Some(line) = inner
                .history
                .last_mut()
                .filter(|line| line.starts_with("… "))
            {
                *line = format!("🤖 {}", &line["… ".len()..]);
            }
            checkpoint_event(&mut inner, "tool_started", Some(name));
            push(
                &mut inner,
                format!("{LIVE_TOOL_CALL_PREFIX}{call_id}\t{name}"),
            );
            push(&mut inner, format!("{LIVE_TOOL_PENDING_PREFIX}{call_id}"));
        }
        audit(
            &self.session,
            "web_tool_started",
            &serde_json::json!({"call_id":call_id,"name":name}).to_string(),
        );
        notify(&self.session, "tool_started");
        self.schedule_checkpoint();
    }

    fn tool_finished(&self, call_id: &str, output: &str, success: bool) {
        if let Ok(mut inner) = self.session.inner.lock() {
            clear_live_tool_output(&mut inner);
            let result = format!(
                "{}{}",
                if success {
                    "\u{1e}TOOL_OK:"
                } else {
                    "\u{1e}TOOL_ERR:"
                },
                output
            );
            let pending = format!("{LIVE_TOOL_PENDING_PREFIX}{call_id}");
            if let Some(entry) = inner
                .history
                .iter_mut()
                .rev()
                .find(|line| line.as_str() == pending)
            {
                *entry = result;
            } else {
                push(&mut inner, result);
            }
            set_activity(&mut inner, "idle", None);
            checkpoint_event(
                &mut inner,
                if success {
                    "tool_completed"
                } else {
                    "tool_failed"
                },
                None,
            );
        }
        audit(
            &self.session,
            "web_tool_finished",
            &serde_json::json!({"call_id":call_id,"success":success,"output":output}).to_string(),
        );
        notify(&self.session, "tool_finished");
        self.schedule_checkpoint();
    }
}

struct WebConfirmer {
    session: Arc<WebSession>,
    state: Option<Arc<Shared>>,
}

fn approval_explanation(assessment: &SecurityAssessment) -> String {
    if let Some(rule) = assessment.matched_rules.first() {
        return match rule.id.as_str() {
            "delete-system" | "delete-system-split-flags" => "检测到递归删除关键目录".into(),
            "filesystem-format" => "检测到格式化文件系统".into(),
            "block-write" | "device-redirect" => "检测到直接写入设备".into(),
            "root-permissions" => "检测到递归修改根目录权限".into(),
            "fork-bomb" => "检测到可能耗尽设备资源的命令".into(),
            "power" => "检测到关机、重启或擦除操作".into(),
            "fastboot-erase" => "检测到擦除分区".into(),
            "remount-rw" | "mount-change" => "检测到修改挂载状态".into(),
            "android-state-change" => "检测到修改应用或系统服务状态".into(),
            "explicit-elevation" => "检测到显式申请更高权限".into(),
            _ => format!("本地安全规则：{}", rule.message),
        };
    }
    match assessment.risk_level {
        crate::security::RiskLevel::ReadOnly => "当前确认策略要求批准这项只读操作".into(),
        crate::security::RiskLevel::Mutating => "检测到可能写入或改变设备状态的操作".into(),
        _ => "本地安全检查将这项操作标记为高风险".into(),
    }
}

#[async_trait]
impl Confirmer for WebConfirmer {
    async fn confirm(
        &self,
        command: &str,
        assessment: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        let pending = Pending::Approval {
            request_id: PENDING_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            command: command.to_owned(),
            risk: format!("{:?}", assessment.risk_level),
            explanation: approval_explanation(assessment),
            root: assessment.requires_root,
            strong: assessment.requires_double_confirmation,
            armed: false,
        };
        match self.wait(pending).await? {
            Reply::Approval(decision) => Ok(decision),
            Reply::Questions(_) => Err(anyhow!("unexpected question reply")),
        }
    }
    async fn ask_questions(&self, questions: &[UserQuestion]) -> Result<Option<QuestionAnswers>> {
        let questions = questions
            .iter()
            .map(|q| WebQuestion {
                id: q.id.clone(),
                header: q.header.clone(),
                prompt: q.prompt.clone(),
                options: q
                    .options
                    .iter()
                    .map(|o| WebOption {
                        label: o.label.clone(),
                        value: o.value.clone(),
                        description: o.description.clone(),
                    })
                    .collect(),
            })
            .collect();
        match self
            .wait(Pending::Questions {
                request_id: PENDING_SEQUENCE.fetch_add(1, Ordering::Relaxed),
                questions,
            })
            .await?
        {
            Reply::Questions(answers) => Ok(answers),
            Reply::Approval(_) => Err(anyhow!("unexpected approval reply")),
        }
    }
}
impl WebConfirmer {
    async fn wait(&self, pending: Pending) -> Result<Reply> {
        let is_approval = matches!(&pending, Pending::Approval { .. });
        let cancelled_reply = || {
            if is_approval {
                Reply::Approval(ConfirmationDecision::Reject)
            } else {
                Reply::Questions(None)
            }
        };
        if *self.session.cancel.borrow() {
            return Ok(cancelled_reply());
        }
        let (tx, rx) = oneshot::channel();
        {
            let mut inner = self
                .session
                .inner
                .lock()
                .map_err(|_| anyhow!("web state lock poisoned"))?;
            inner.pending = Some(pending);
            inner.reply = Some(tx);
            set_activity(&mut inner, "waiting", None);
            checkpoint_event(
                &mut inner,
                if is_approval {
                    "approval_waiting"
                } else {
                    "input_waiting"
                },
                None,
            );
        }
        audit(
            &self.session,
            "web_waiting",
            if is_approval { "approval" } else { "questions" },
        );
        notify(&self.session, "pending");
        if let Some(state) = &self.state {
            if let Err(error) = persist_web_session(state, &self.session).await {
                eprintln!("cannot save Web waiting checkpoint: {error:#}");
            }
        }
        tokio::select! {
            reply = rx => reply.context("browser decision was canceled"),
            _ = wait_web_cancel(self.session.cancel.subscribe()) => {
                if let Ok(mut inner) = self.session.inner.lock() {
                    inner.pending = None;
                    inner.reply = None;
                    set_activity(&mut inner, "cancelling", None);
                }
                notify(&self.session, "cancelling");
                Ok(cancelled_reply())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ApiType;
    use crate::llm::{FinishReason, LlmResponse, ToolCall, ToolResult, Usage};
    use std::io::Read;
    use tempfile::tempdir;
    use wiremock::{
        matchers::{body_string_contains, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    async fn browser_files_expose_metadata_and_bounded_inline_preview() -> Result<()> {
        let directory = tempdir()?;
        let text_path = directory.path().join("example.rs");
        std::fs::write(&text_path, b"hello browser")?;
        std::fs::create_dir(directory.path().join("folder"))?;
        let Json(files) = get_files(Query(FilePathQuery {
            path: directory.path().to_string_lossy().into_owned(),
        }))
        .await
        .map_err(|error| error.error)?;
        assert_eq!(files.len(), 2);
        assert!(files[0].is_dir);
        assert_eq!(files[1].preview_kind, Some("text"));
        assert_eq!(files[1].size, Some(13));
        assert!(files[1].modified_ms.is_some());

        let query = FilePathQuery {
            path: text_path.to_string_lossy().into_owned(),
        };
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(header::RANGE, HeaderValue::from_static("bytes=6-12"));
        let response = get_file_preview(Query(query), headers)
            .await
            .map_err(|error| error.error)?;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 6-12/13");
        let body = axum::body::to_bytes(response.into_body(), 100).await?;
        assert_eq!(&body[..], b"browser");
        assert_eq!(requested_range("bytes=-4", 13), Some((9, 12)));
        assert_eq!(requested_range("bytes=99-", 13), None);
        assert!(preview_type(Path::new("secret.bin")).is_none());
        assert_eq!(
            preview_type(Path::new("sound.wav")).map(|value| value.0),
            Some("audio")
        );
        assert_eq!(
            preview_type(Path::new("samples.pcm")).map(|value| value.0),
            Some("audio")
        );
        Ok(())
    }

    #[test]
    fn task_timing_separates_model_tools_and_waits_and_freezes_on_finish() {
        let mut inner = SessionState::empty();
        inner.task_started = Some(Instant::now() - std::time::Duration::from_secs(10));
        inner.activity = "thinking";
        inner.phase_started = Instant::now() - std::time::Duration::from_secs(2);
        set_activity(&mut inner, "tool", Some("read_file"));
        inner.phase_started = Instant::now() - std::time::Duration::from_secs(3);
        set_activity(&mut inner, "waiting", None);
        inner.phase_started = Instant::now() - std::time::Duration::from_secs(4);
        finish_task(&mut inner);
        let task = task_metrics(&inner);
        assert!((2000..2200).contains(&task.model_ms));
        assert!((3000..3200).contains(&task.tool_ms));
        assert!((4000..4200).contains(&task.waiting_ms));
        assert!((10000..10200).contains(&task.total_ms));
        inner.phase_started = Instant::now() - std::time::Duration::from_secs(500);
        assert_eq!(task_metrics(&inner).total_ms, task.total_ms);
        assert_eq!(task_metrics(&inner).waiting_ms, task.waiting_ms);
    }

    #[tokio::test]
    async fn live_progress_and_reasoning_survive_failed_task_restart_without_model_history(
    ) -> Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let shared = state(path.clone());
        let current = session(&shared, "web-test")?;
        {
            let mut inner = current.inner.lock().map_err(|_| anyhow!("lock"))?;
            inner.busy = true;
            inner.accepted_turns = 2;
            inner.token_base = (10, 20);
            inner.task_started = Some(Instant::now());
            inner.checkpoint_start = Some(0);
            inner.redaction_secrets = vec!["private-test-key".into()];
            push(&mut inner, "> 原任务".into());
            inner.checkpoint = Some(WebCheckpoint {
                status: "failed".into(),
                activity: "thinking".into(),
                history: Vec::new(),
                events: Vec::new(),
            });
        }
        let sink = WebTextSink {
            session: current.clone(),
            state: None,
        };
        sink.agent_progress(
            3,
            2,
            &Usage {
                input_tokens: Some(100),
                output_tokens: Some(40),
            },
        );
        sink.reasoning_delta("检查 private-test-key");
        sink.reasoning_delta(" 的结果");
        sink.delta("中间说明");
        sink.tool_started("call", "read_file");
        sink.tool_finished("call", "失败证据", false);
        let snapshot = get_state(
            State(shared.clone()),
            Query(StateQuery {
                id: "web-test".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?
        .0;
        assert_eq!(
            (snapshot.turns, snapshot.steps, snapshot.tool_calls),
            (2, 3, 2)
        );
        assert_eq!((snapshot.input_tokens, snapshot.output_tokens), (110, 60));
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.kind == "reasoning"));
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.kind == "assistant" && entry.text == "中间说明"));
        {
            let mut inner = current.inner.lock().map_err(|_| anyhow!("lock"))?;
            finish_task(&mut inner);
            inner.busy = false;
        }
        persist_web_session(&shared, &current).await?;
        let raw = std::fs::read_to_string(directory.path().join("sessions/web-test.json"))?;
        assert!(!raw.contains("private-test-key"));
        let restored = state(path.clone());
        restored
            .sessions
            .lock()
            .map_err(|_| anyhow!("lock"))?
            .clear();
        let _ = load_session(
            State(restored.clone()),
            Json(SessionSelection {
                name: "web-test".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        let snapshot = get_state(
            State(restored),
            Query(StateQuery {
                id: "web-test".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?
        .0;
        assert_eq!(
            (snapshot.turns, snapshot.steps, snapshot.tool_calls),
            (2, 3, 2)
        );
        assert_eq!((snapshot.input_tokens, snapshot.output_tokens), (110, 60));
        assert!(snapshot.can_retry);
        assert!(!snapshot.busy);
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.kind == "reasoning"));
        assert!(SessionStore::open(&path)?
            .load("web-test", 10, 1024)?
            .is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn web_audit_identifies_sessions_and_redacts_model_and_tool_content() -> Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let shared = state(path.clone());
        let current = session(&shared, "web-test")?;
        let cfg = Config {
            api_key: "private-test-key".into(),
            ..Config::default()
        };
        {
            let mut inner = current.inner.lock().map_err(|_| anyhow!("lock"))?;
            remember_redaction_secrets(&mut inner, &cfg);
        }
        start_audit(path.clone(), &cfg, &current).await?;
        audit(&current, "web_user", "任务 private-test-key");
        let sink = WebTextSink {
            session: current.clone(),
            state: None,
        };
        sink.reasoning_delta("思考 private-test-key");
        sink.delta("说明");
        sink.end(true);
        sink.tool_requested(&ToolCall {
            id: "call".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({"path":"private-test-key"}),
        });
        sink.tool_started("call", "read_file");
        sink.tool_finished("call", "private-test-key 结果", true);
        flush_audit(&current).await;
        let raw = std::fs::read_to_string(directory.path().join(&cfg.history_log_file))?;
        assert!(!raw.contains("private-test-key"));
        assert!(raw.contains("CREDENTIAL REDACTED"));
        for line in raw.lines() {
            let record: serde_json::Value = serde_json::from_str(line)?;
            let payload: serde_json::Value =
                serde_json::from_str(record["message"].as_str().context("message")?)?;
            assert_eq!(payload["session_id"], "web-test");
        }
        assert!(raw.contains("web_tool_requested"));
        assert!(raw.contains("web_tool_finished"));
        assert!(raw.contains("思考"));
        Ok(())
    }

    #[tokio::test]
    async fn model_check_uses_real_provider_request_without_a_session() -> Result<()> {
        let provider = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "OK"}, "finish_reason": "stop"}],
                "usage": {}
            })))
            .expect(1)
            .mount(&provider)
            .await;
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let cfg = Config {
            endpoint: format!("{}/v1", provider.uri()),
            api_key: "test-key".into(),
            model: "test-model".into(),
            api_type: ApiType::ChatCompletions,
            ..Config::default()
        };
        config::save_config(&path, &cfg)?;
        let shared = state(path);
        let Json(result) = check_model(State(shared.clone()))
            .await
            .map_err(|error| error.error)?;
        assert!(result.ok);
        assert_eq!(
            shared
                .sessions
                .lock()
                .map_err(|_| anyhow!("web lock poisoned"))?
                .len(),
            1
        );
        Ok(())
    }

    #[tokio::test]
    async fn device_overview_works_without_model_configuration() -> Result<()> {
        let directory = tempdir()?;
        let shared = state(directory.path().join("missing-config.toml"));
        let Json(result) = get_device_overview(State(shared))
            .await
            .map_err(|error| error.error)?;
        assert_eq!(result["kind"], "android_environment");
        assert!(result["status"] == "complete" || result["status"] == "partial");
        Ok(())
    }

    #[tokio::test]
    async fn cancel_request_rejects_pending_approval_and_keeps_session_busy_until_cleanup(
    ) -> Result<()> {
        let directory = tempdir()?;
        let shared = state(directory.path().join("config.toml"));
        let current = session(&shared, "web-test")?;
        let (reply, waiting) = oneshot::channel();
        let request_id = PENDING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        {
            let mut inner = current
                .inner
                .lock()
                .map_err(|_| anyhow!("web lock poisoned"))?;
            inner.busy = true;
            inner.pending = Some(Pending::Approval {
                request_id,
                command: "touch /tmp/x".into(),
                risk: "Mutating".into(),
                explanation: "test".into(),
                root: false,
                strong: false,
                armed: false,
            });
            inner.reply = Some(reply);
        }
        cancel_message(
            State(shared),
            Json(SessionSelection {
                name: "web-test".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        assert!(*current.cancel.borrow());
        assert!(matches!(
            waiting.await?,
            Reply::Approval(ConfirmationDecision::Reject)
        ));
        let inner = current
            .inner
            .lock()
            .map_err(|_| anyhow!("web lock poisoned"))?;
        assert!(inner.busy);
        assert!(inner.pending.is_none());
        assert_eq!(inner.activity, "cancelling");
        Ok(())
    }

    #[tokio::test]
    async fn dangerous_web_approval_requires_two_explicit_decisions() -> Result<()> {
        let directory = tempdir()?;
        let shared = state(directory.path().join("config.toml"));
        let current = session(&shared, "web-test")?;
        let (reply, waiting) = oneshot::channel();
        let request_id = PENDING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        {
            let mut inner = current
                .inner
                .lock()
                .map_err(|_| anyhow!("web lock poisoned"))?;
            inner.busy = true;
            inner.pending = Some(Pending::Approval {
                request_id,
                command: "rm -rf /tmp/test".into(),
                risk: "Dangerous".into(),
                explanation: "test".into(),
                root: false,
                strong: true,
                armed: false,
            });
            inner.reply = Some(reply);
        }
        let decision = |action: &str| Decision {
            session_id: "web-test".into(),
            request_id,
            action: action.into(),
            text: None,
            answers: None,
        };
        assert!(post_decision(
            State(shared.clone()),
            Json(Decision {
                request_id: request_id.saturating_add(1),
                ..decision("arm")
            })
        )
        .await
        .is_err());
        assert!(
            post_decision(State(shared.clone()), Json(decision("approve")))
                .await
                .is_err()
        );
        post_decision(State(shared.clone()), Json(decision("arm")))
            .await
            .map_err(|error| error.error)?;
        {
            let inner = current
                .inner
                .lock()
                .map_err(|_| anyhow!("web lock poisoned"))?;
            assert!(matches!(
                inner.pending,
                Some(Pending::Approval { armed: true, .. })
            ));
            assert!(inner.reply.is_some());
        }
        post_decision(State(shared), Json(decision("approve")))
            .await
            .map_err(|error| error.error)?;
        assert!(matches!(
            waiting.await?,
            Reply::Approval(ConfirmationDecision::ApproveCaptured)
        ));
        Ok(())
    }

    #[test]
    fn approval_explanation_uses_local_rule_evidence() {
        let assessment = crate::security::assess("rm -rf /", &Config::default());
        assert_eq!(assessment.risk_level, crate::security::RiskLevel::Critical);
        assert_eq!(approval_explanation(&assessment), "检测到递归删除关键目录");
    }

    #[test]
    fn consecutive_pending_requests_have_distinct_frontend_identities() -> Result<()> {
        let first = Pending::Approval {
            request_id: PENDING_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            command: "same command".into(),
            risk: "Mutating".into(),
            explanation: "test".into(),
            root: false,
            strong: false,
            armed: false,
        };
        let second = Pending::Approval {
            request_id: PENDING_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            command: "same command".into(),
            risk: "Mutating".into(),
            explanation: "test".into(),
            root: false,
            strong: false,
            armed: false,
        };
        let first = serde_json::to_value(first)?;
        let second = serde_json::to_value(second)?;
        assert_ne!(first["request_id"], second["request_id"]);
        Ok(())
    }

    fn state(path: PathBuf) -> Arc<Shared> {
        let mut inner = SessionState::empty();
        inner.id = "web-test".into();
        let current = web_session(inner);
        Arc::new(Shared {
            started: Instant::now(),
            port: 0,
            path,
            sessions: Mutex::new(BTreeMap::from([("web-test".into(), current)])),
        })
    }

    #[tokio::test]
    async fn memory_api_supports_crud_in_the_shared_sqlite_store() -> Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let shared = state(path.clone());

        let Json(initial) = get_memory(State(shared.clone()))
            .await
            .map_err(|error| error.error)?;
        assert!(initial.is_empty());

        let Json(created) = set_memory(
            State(shared.clone()),
            Json(MemorySetRequest {
                key: "user_name".into(),
                value: "ernest".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].key, "user_name");
        assert_eq!(created[0].value, "ernest");

        let Json(updated) = set_memory(
            State(shared.clone()),
            Json(MemorySetRequest {
                key: "user_name".into(),
                value: "Ernest".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        assert_eq!(updated[0].value, "Ernest");

        let Json(deleted) = delete_memory(
            State(shared.clone()),
            Json(MemoryKeyRequest {
                key: "user_name".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        assert!(deleted.is_empty());

        let _ = set_memory(
            State(shared.clone()),
            Json(MemorySetRequest {
                key: "language".into(),
                value: "zh-CN".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        let Json(cleared) = clear_memory(State(shared))
            .await
            .map_err(|error| error.error)?;
        assert!(cleared.is_empty());
        assert!(config::memory_dir(&path)?
            .join("agent-memory.sqlite3")
            .is_file());
        assert!(!directory.path().join(".nl2sh-agent-memory.json").exists());
        Ok(())
    }

    #[test]
    fn generated_titles_are_bounded_and_cleaned() {
        assert_eq!(
            crate::session_title::normalize_title("**\"检查网络连接\"**\nextra"),
            Some("检查网络连接".into())
        );
        assert_eq!(crate::session_title::normalize_title("   "), None);
        assert_eq!(
            crate::session_title::normalize_title(&"a".repeat(60))
                .map(|value| value.chars().count()),
            Some(48)
        );
        assert!(crate::session_title::normalize_title(&"😀".repeat(60))
            .is_some_and(|value| value.len() <= 150));
    }

    struct StaticTitle;

    #[async_trait]
    impl LlmClient for StaticTitle {
        async fn complete(&self, _: LlmRequest) -> Result<LlmResponse> {
            Ok(LlmResponse {
                text: Some("已完成任务".into()),
                tool_calls: Vec::new(),
                usage: Usage::default(),
                finish_reason: FinishReason::Stop,
            })
        }
    }

    #[tokio::test]
    async fn delayed_title_does_not_restore_a_deleted_session() -> Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let shared = state(path.clone());
        let current = session(&shared, "web-test")?;
        {
            let mut inner = current.inner.lock().map_err(|_| anyhow!("poisoned"))?;
            inner.turns.push(vec![
                ConversationItem::Message(ConversationMessage::new(Role::User, "问题")),
                ConversationItem::Message(ConversationMessage::new(Role::Assistant, "回答")),
            ]);
            inner.history = render_turns(&inner.turns);
        }
        persist_web_session(&shared, &current).await?;
        delete_session(
            State(shared.clone()),
            Json(SessionSelection {
                name: "web-test".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        generate_title_after_reply(
            shared,
            current,
            StaticTitle,
            Config::default(),
            "问题".into(),
            "回答".into(),
        )
        .await;
        assert!(SessionStore::open(&path)?.list()?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn reply_is_idle_and_saved_before_title_request_finishes() -> Result<()> {
        let provider = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("Generate a concise title"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_secs(1))
                    .set_body_json(serde_json::json!({
                        "choices": [{"message": {"content": "简短标题"}, "finish_reason": "stop"}]
                    })),
            )
            .with_priority(1)
            .expect(1)
            .mount(&provider)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(concat!(
                        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"先核对证据\"}}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{\"content\":\"任务完成\"},\"finish_reason\":null}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                        "data: [DONE]\n\n"
                    )),
            )
            .with_priority(10)
            .expect(1)
            .mount(&provider)
            .await;
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        config::save_config(
            &path,
            &Config {
                endpoint: format!("{}/v1", provider.uri()),
                api_key: "test-key".into(),
                model: "test-model".into(),
                api_type: ApiType::ChatCompletions,
                ..Config::default()
            },
        )?;
        let shared = state(path.clone());
        post_message(
            State(shared.clone()),
            Json(Message {
                session_id: "web-test".into(),
                text: "请简短回答".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        let current = session(&shared, "web-test")?;
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let ready = current
                    .inner
                    .lock()
                    .is_ok_and(|inner| !inner.busy && inner.turns.len() == 1);
                if ready {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await?;
        assert_eq!(
            current.inner.lock().map_err(|_| anyhow!("poisoned"))?.title,
            "新会话"
        );
        let store = SessionStore::open(&path)?;
        assert_eq!(store.load("web-test", 10, 1024)?.len(), 1);
        assert!(store.load_web_checkpoint("web-test")?.is_none());
        let display = store
            .load_web_presentation("web-test")?
            .context("display")?;
        assert_eq!(display.turns, 1);
        assert_eq!(display.task.steps, 1);
        assert_eq!(display.task.tool_calls, 0);
        assert!(display.task.timing_available);
        assert!(display
            .history
            .iter()
            .any(|line| line.starts_with("\u{1e}TASK:")));
        assert!(display
            .history
            .iter()
            .any(|line| line.contains("先核对证据")));
        let log = std::fs::read_to_string(directory.path().join("nl2sh.log"))?;
        assert!(log.contains("web_user"));
        assert!(log.contains("web_assistant"));
        assert!(log.contains("web_task_finished"));

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if current
                    .inner
                    .lock()
                    .is_ok_and(|inner| inner.title == "简短标题")
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await?;
        assert_eq!(store.list()?[0].title, "简短标题");
        Ok(())
    }

    #[tokio::test]
    async fn tool_start_is_checkpointed_before_a_result_exists() -> Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let shared = state(path.clone());
        let current = session(&shared, "web-test")?;
        {
            let mut inner = current.inner.lock().map_err(|_| anyhow!("poisoned"))?;
            inner.checkpoint = Some(WebCheckpoint {
                status: "running".into(),
                activity: "thinking".into(),
                history: Vec::new(),
                events: Vec::new(),
            });
        }
        let sink = WebTextSink {
            session: current,
            state: Some(shared),
        };
        sink.tool_started("pending", "read_file");
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Ok(Some(checkpoint)) = SessionStore::open(&path)
                    .and_then(|store| store.load_web_checkpoint("web-test"))
                {
                    if checkpoint
                        .events
                        .iter()
                        .any(|event| event.kind == "tool_started")
                    {
                        assert!(checkpoint
                            .history
                            .iter()
                            .any(|line| line.contains(LIVE_TOOL_PENDING_PREFIX)));
                        break;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn checkpoint_keeps_redacting_keys_after_provider_changes() -> Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let mut cfg = Config {
            api_key: "old-private-key".into(),
            ..Config::default()
        };
        config::save_config(&path, &cfg)?;
        let shared = state(path.clone());
        let current = session(&shared, "web-test")?;
        {
            let mut inner = current.inner.lock().map_err(|_| anyhow!("poisoned"))?;
            remember_redaction_secrets(&mut inner, &cfg);
            inner
                .history
                .push("> old-private-key new-private-key".into());
            inner.checkpoint = Some(WebCheckpoint {
                status: "running".into(),
                activity: "thinking".into(),
                history: Vec::new(),
                events: Vec::new(),
            });
        }
        cfg.api_key = "new-private-key".into();
        config::save_config(&path, &cfg)?;
        persist_web_session(&shared, &current).await?;
        let raw = std::fs::read_to_string(directory.path().join("sessions/web-test.json"))?;
        assert!(!raw.contains("old-private-key"));
        assert!(!raw.contains("new-private-key"));
        Ok(())
    }

    #[test]
    fn retry_uses_only_the_original_failed_input() {
        let mut inner = SessionState::empty();
        inner.history = vec![
            "> inspect the device".into(),
            "\u{1e}TOOL_OK:untrusted output".into(),
            "❌ provider failed".into(),
        ];
        inner.checkpoint_start = Some(0);
        inner.checkpoint = Some(WebCheckpoint {
            status: "failed".into(),
            activity: "thinking".into(),
            history: Vec::new(),
            events: Vec::new(),
        });
        assert_eq!(
            retryable_input(&inner).as_deref(),
            Some("inspect the device")
        );

        if let Some(checkpoint) = inner.checkpoint.as_mut() {
            checkpoint.history = vec!["> [NL2SH CREDENTIAL REDACTED]".into()];
        }
        assert!(retryable_input(&inner).is_none());
    }

    #[test]
    fn streaming_deltas_share_one_entry_and_tool_results_are_typed() -> Result<()> {
        let state = state(PathBuf::from("unused.toml"));
        let current = session(&state, "web-test")?;
        let sink = WebTextSink {
            session: current.clone(),
            state: None,
        };
        sink.delta("第一段");
        sink.delta("第二段");
        let mut inner = current.inner.lock().map_err(|_| anyhow!("lock poisoned"))?;
        push(&mut inner, "\u{1e}TOOL_OK:result".into());
        assert_eq!(inner.history[0], "… 第一段第二段");
        let entries = web_entries(&inner.history);
        assert_eq!(entries[0].kind, "stream");
        assert_eq!(entries[1].kind, "tool_result");
        Ok(())
    }

    #[test]
    fn tool_events_are_visible_immediately_and_match_results_by_call_id() -> Result<()> {
        let state = state(PathBuf::from("unused.toml"));
        let current = session(&state, "web-test")?;
        let sink = WebTextSink {
            session: current.clone(),
            state: None,
        };

        sink.tool_started("first", "analyze_audio");
        sink.tool_started("second", "analyze_audio");
        {
            let inner = current.inner.lock().map_err(|_| anyhow!("lock poisoned"))?;
            assert_eq!(
                web_entries(&inner.history),
                vec![
                    WebEntry {
                        kind: "tool_call",
                        text: "analyze_audio".into(),
                    },
                    WebEntry {
                        kind: "tool_call",
                        text: "analyze_audio".into(),
                    },
                ]
            );
        }

        sink.tool_finished("second", "second result", true);
        sink.tool_finished("first", "first result", false);
        let inner = current.inner.lock().map_err(|_| anyhow!("lock poisoned"))?;
        assert_eq!(
            web_entries(&inner.history),
            vec![
                WebEntry {
                    kind: "tool_call",
                    text: "analyze_audio".into(),
                },
                WebEntry {
                    kind: "tool_error",
                    text: "first result".into(),
                },
                WebEntry {
                    kind: "tool_call",
                    text: "analyze_audio".into(),
                },
                WebEntry {
                    kind: "tool_result",
                    text: "second result".into(),
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn live_shell_output_disappears_when_tool_result_is_ready() -> Result<()> {
        let state = state(PathBuf::from("unused.toml"));
        let current = session(&state, "web-test")?;
        let sink = WebTextSink {
            session: current.clone(),
            state: None,
        };
        let output = WebOutput {
            session: current.clone(),
        };

        sink.tool_started("shell", "execute_shell_command");
        output.stdout("14");
        output.stderr("warning");
        {
            let inner = current.inner.lock().map_err(|_| anyhow!("lock poisoned"))?;
            assert_eq!(
                web_entries(&inner.history)
                    .iter()
                    .filter(|entry| entry.kind == "tool_output")
                    .count(),
                2
            );
        }

        sink.tool_finished("shell", "stdout:\n14\nstderr:\nwarning", true);
        let inner = current.inner.lock().map_err(|_| anyhow!("lock poisoned"))?;
        assert_eq!(
            web_entries(&inner.history),
            vec![
                WebEntry {
                    kind: "tool_call",
                    text: "execute_shell_command".into(),
                },
                WebEntry {
                    kind: "tool_result",
                    text: "stdout:\n14\nstderr:\nwarning".into(),
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn restored_chart_keeps_matching_call_and_structured_result() {
        let chart = serde_json::json!({
            "chart_type": "bar",
            "title": "Storage",
            "source": "android_storage result",
            "unit": "GiB",
            "labels": ["apps", "media"],
            "values": [2.5, 4.0]
        })
        .to_string();
        let round = ToolRound {
            calls: vec![ToolCall {
                id: "chart-1".into(),
                name: "create_chart".into(),
                arguments: serde_json::json!({}),
            }],
            results: vec![ToolResult {
                call_id: "chart-1".into(),
                output: chart.clone(),
                success: true,
                attachments: Vec::new(),
            }],
        };
        let entries = web_entries(&render_turns(&[vec![ConversationItem::Tools(round)]]));
        assert_eq!(entries[0].kind, "tool_call");
        assert_eq!(entries[0].text, "create_chart");
        assert_eq!(entries[1].kind, "tool_result");
        assert_eq!(entries[1].text, chart);
    }

    #[test]
    fn tool_round_results_follow_their_matching_calls_in_live_and_restored_history() {
        let round = ToolRound {
            calls: vec![
                ToolCall {
                    id: "first".into(),
                    name: "read_file".into(),
                    arguments: serde_json::json!({}),
                },
                ToolCall {
                    id: "second".into(),
                    name: "list_dir".into(),
                    arguments: serde_json::json!({}),
                },
            ],
            results: vec![
                ToolResult {
                    call_id: "second".into(),
                    output: "second output".into(),
                    success: false,
                    attachments: Vec::new(),
                },
                ToolResult {
                    call_id: "first".into(),
                    output: "first output".into(),
                    success: true,
                    attachments: Vec::new(),
                },
            ],
        };
        let expected = vec![
            "🔧 read_file",
            "\u{1e}TOOL_OK:first output",
            "🔧 list_dir",
            "\u{1e}TOOL_ERR:second output",
        ];
        let mut live = Vec::new();
        append_tool_round(&mut live, &round);
        assert_eq!(live, expected);
        assert_eq!(
            render_turns(&[vec![ConversationItem::Tools(round)]]),
            expected
        );
    }

    #[test]
    fn export_keeps_selected_entries_when_log_does_not_exist() -> Result<()> {
        let dir = tempdir()?;
        let conversation = ExportConversation {
            id: "selected".into(),
            title: "当前会话".into(),
            exported_unix_secs: 1,
            task: WebTaskMetrics::default(),
            busy: true,
            entries: web_entries(&["> 本轮问题".into(), "🤖 本轮回答".into()]),
            checkpoint_status: None,
            checkpoint_events: Vec::new(),
        };
        let bytes = build_session_export(&dir.path().join("config.toml"), &conversation)?;
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
        let mut json = String::new();
        archive
            .by_name("conversation.json")?
            .read_to_string(&mut json)?;
        let value: serde_json::Value = serde_json::from_str(&json)?;
        assert_eq!(value["id"], "selected");
        assert_eq!(value["entries"][0]["text"], "本轮问题");
        assert_eq!(value["entries"][1]["text"], "本轮回答");
        assert_eq!(archive.by_name("nl2sh.log")?.size(), 0);
        Ok(())
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::io::Read;
    use tempfile::tempdir;

    #[tokio::test]
    async fn model_list_uses_unsaved_quick_start_credentials() -> Result<()> {
        let provider_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let provider_port = provider_listener.local_addr()?.port();
        let provider = Router::new().route(
            "/v1/models",
            get(|headers: axum::http::HeaderMap| async move {
                let authorization = headers
                    .get(header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok());
                match authorization {
                    Some("Bearer draft-key") => (
                        StatusCode::OK,
                        Json(serde_json::json!({"data":[{"id":"draft-model"}]})),
                    ),
                    Some("Bearer gateway-test-key") => (
                        StatusCode::BAD_GATEWAY,
                        Json(serde_json::json!({"error":"upstream detail must stay private"})),
                    ),
                    _ => (
                        StatusCode::UNAUTHORIZED,
                        Json(serde_json::json!({"error":"unauthorized"})),
                    ),
                }
            }),
        );
        let provider_task = tokio::spawn(async move {
            let _ = axum::serve(provider_listener, provider).await;
        });
        let dir = tempdir()?;
        let path = dir.path().join("config.toml");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let server = start_with_listener(path.clone(), listener).await?;
        let web_port = url::Url::parse(server.url())?
            .port()
            .context("web URL lacks port")?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let response = client
            .post(format!("http://127.0.0.1:{web_port}/api/models"))
            .json(&serde_json::json!({
                "endpoint": format!("http://127.0.0.1:{provider_port}/v1"),
                "api_key": "draft-key"
            }))
            .send()
            .await?;
        assert!(response.status().is_success());
        let models: serde_json::Value = response.json().await?;
        assert_eq!(models[0]["id"], "draft-model");
        let failure = client
            .post(format!("http://127.0.0.1:{web_port}/api/models"))
            .json(&serde_json::json!({
                "endpoint": format!("http://127.0.0.1:{provider_port}/v1"),
                "api_key": "gateway-test-key"
            }))
            .send()
            .await?;
        assert_eq!(failure.status(), StatusCode::BAD_REQUEST);
        assert!(failure.text().await?.contains("502 Bad Gateway"));
        let log = std::fs::read_to_string(config::state_dir(&path)?.join("nl2sh.log"))?;
        assert!(log.contains("web_model_list_started"));
        assert!(log.contains("web_model_list_finished"));
        assert!(log.contains("502 Bad Gateway"));
        assert!(!log.contains("draft-key"));
        assert!(!log.contains("gateway-test-key"));
        assert!(!log.contains("upstream detail must stay private"));
        assert!(!path.exists());
        provider_task.abort();
        Ok(())
    }

    #[tokio::test]
    async fn health_and_info_identify_actual_listener_without_provider_or_sessions() -> Result<()> {
        let dir = tempdir()?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let port = listener.local_addr()?.port();
        let server = start_with_listener(dir.path().join("missing.toml"), listener).await?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let base = format!("http://127.0.0.1:{port}");
        let health: serde_json::Value = client
            .get(format!("{base}/healthz"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(health, serde_json::json!({"status":"ok"}));
        let info: serde_json::Value = client
            .get(format!("{base}/api/info"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(info["port"], port);
        assert_eq!(info["pid"], std::process::id());
        assert_eq!(info["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(info["protocol"], 1);
        assert!(info["uptime"].is_u64());
        assert!(info["capabilities"]["android_shell"].is_boolean());
        assert!(info["capabilities"]["jadx_provisionable"].is_boolean());
        assert!(!info.to_string().contains("api_key"));
        server.task.abort();
        Ok(())
    }

    #[tokio::test]
    async fn config_editor_validates_renders_and_applies_saved_settings() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("config.toml");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let server = start_with_listener(path.clone(), listener).await?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let port = url::Url::parse(server.url())?
            .port()
            .context("web URL lacks port")?;
        let base = format!("http://127.0.0.1:{port}");
        let version = client
            .get(format!("{base}/api/version"))
            .send()
            .await?
            .text()
            .await?;
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
        let initial_quick: serde_json::Value = client
            .get(format!("{base}/api/quick-settings"))
            .send()
            .await?
            .json()
            .await?;
        if std::env::var_os("NL2SH_API_KEY").is_none() {
            assert_eq!(initial_quick["provider_ready"], false);
        }
        let invalid: serde_json::Value = client
            .post(format!("{base}/api/config/validate"))
            .body("model = [")
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(invalid["valid"], false);
        let original = client
            .get(format!("{base}/api/config"))
            .send()
            .await?
            .text()
            .await?;
        let preview: serde_json::Value = client
            .post(format!("{base}/api/config/validate"))
            .body(original)
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(preview["valid"], true);
        let mut config = preview["config"].clone();
        config["model"] = serde_json::json!("web-editor-test");
        config["api_key"] = serde_json::json!("test-key");
        let rendered = client
            .post(format!("{base}/api/config/render"))
            .json(&config)
            .send()
            .await?;
        assert!(rendered.status().is_success());
        let rendered: serde_json::Value = rendered.json().await?;
        assert_eq!(rendered["valid"], true);
        let rendered = rendered["toml"].as_str().context("missing rendered TOML")?;
        assert!(rendered.contains("model = \"web-editor-test\""));
        let saved = client
            .post(format!("{base}/api/config"))
            .body(rendered.to_owned())
            .send()
            .await?;
        assert!(saved.status().is_success());
        assert_eq!(load_config(path).await?.model, "web-editor-test");
        let ready_quick: serde_json::Value = client
            .get(format!("{base}/api/quick-settings"))
            .send()
            .await?
            .json()
            .await?;
        if std::env::var_os("NL2SH_API_KEY").is_none() {
            assert_eq!(ready_quick["provider_ready"], true);
        }
        assert!(ready_quick.get("api_key").is_none());
        Ok(())
    }

    #[tokio::test]
    async fn deletes_single_and_all_web_sessions_without_touching_other_state() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("config.toml");
        let store = SessionStore::open(&path)?;
        store.save("saved-one", &[], 1024)?;
        store.save("saved-two", &[], 1024)?;
        let state_directory = config::state_dir(&path)?;
        std::fs::write(state_directory.join("keep.txt"), b"keep")?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let server = start_with_listener(path, listener).await?;
        let port = url::Url::parse(server.url())?
            .port()
            .context("web URL lacks port")?;
        let base = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::builder().no_proxy().build()?;
        let created: serde_json::Value = client
            .post(format!("{base}/api/sessions/new"))
            .send()
            .await?
            .json()
            .await?;
        let created_id = created["id"].as_str().context("missing session id")?;
        assert!(client
            .post(format!("{base}/api/sessions/delete"))
            .json(&serde_json::json!({"name": "saved-one"}))
            .send()
            .await?
            .status()
            .is_success());
        assert!(store
            .list()?
            .iter()
            .all(|session| session.name != "saved-one"));
        assert!(client
            .post(format!("{base}/api/sessions/delete"))
            .json(&serde_json::json!({"name": created_id}))
            .send()
            .await?
            .status()
            .is_success());
        assert_eq!(
            client
                .get(format!("{base}/api/state?id={created_id}"))
                .send()
                .await?
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert!(client
            .post(format!("{base}/api/sessions/delete-all"))
            .send()
            .await?
            .status()
            .is_success());
        let remaining: serde_json::Value = client
            .get(format!("{base}/api/sessions"))
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(remaining.as_array().map(Vec::len), Some(0));
        assert!(store.list()?.is_empty());
        assert_eq!(std::fs::read(state_directory.join("keep.txt"))?, b"keep");
        Ok(())
    }

    #[tokio::test]
    async fn refuses_to_delete_running_sessions() -> Result<()> {
        let dir = tempdir()?;
        let state = Arc::new(Shared {
            started: Instant::now(),
            port: 0,
            path: dir.path().join("config.toml"),
            sessions: Mutex::new(BTreeMap::from([(
                "web-test".into(),
                web_session(SessionState {
                    id: "web-test".into(),
                    ..SessionState::empty()
                }),
            )])),
        });
        let current = session(&state, "web-test")?;
        current.inner.lock().map_err(|_| anyhow!("poisoned"))?.busy = true;
        assert!(matches!(
            delete_session(
                State(state.clone()),
                Json(SessionSelection {
                    name: "web-test".into()
                })
            )
            .await,
            Err(ApiError {
                status: StatusCode::CONFLICT,
                ..
            })
        ));
        assert!(matches!(
            delete_all_sessions(State(state)).await,
            Err(ApiError {
                status: StatusCode::CONFLICT,
                ..
            })
        ));
        Ok(())
    }

    #[tokio::test]
    async fn serves_browser_page_and_state_over_http() -> Result<()> {
        let dir = tempdir()?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let server = start_with_listener(dir.path().join("config.toml"), listener).await?;
        let url = url::Url::parse(server.url())?;
        let port = url.port().context("web URL lacks port")?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let base = format!("http://127.0.0.1:{port}");
        let page = client.get(format!("{base}/")).send().await?;
        assert!(page.status().is_success());
        assert!(page.headers().contains_key("content-security-policy"));
        let page = page.text().await?;
        assert!(page.contains("nl2sh Web"));
        assert!(page.contains("/assets/"));
        let sessions: serde_json::Value = client
            .get(format!("{base}/api/sessions"))
            .send()
            .await?
            .json()
            .await?;
        let id = sessions[0]["id"].as_str().context("missing session id")?;
        let state: serde_json::Value = client
            .get(format!("{base}/api/state?id={id}"))
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(state["busy"], false);
        assert!(state["history"].as_array().is_some());
        let tools: serde_json::Value = client
            .get(format!("{base}/api/tools"))
            .send()
            .await?
            .json()
            .await?;
        assert!(tools
            .as_array()
            .is_some_and(|items| items.iter().any(|tool| {
                tool["name"] == "read_file" && tool["description"].as_str().is_some()
            })));
        assert!(tools.as_array().is_some_and(|items| items
            .iter()
            .any(|tool| tool["name"] == "tailcat_check" && tool["enabled"] == false)));
        let changed: serde_json::Value = client
            .post(format!("{base}/api/tools/toggle"))
            .json(&serde_json::json!({"group":"tailcat","enabled":true}))
            .send()
            .await?
            .json()
            .await?;
        assert!(changed.as_array().is_some_and(|items| items
            .iter()
            .any(|tool| tool["name"] == "tailcat_check" && tool["enabled"] == true)));
        let changed: serde_json::Value = client
            .post(format!("{base}/api/tools/toggle"))
            .json(&serde_json::json!({"tool":"tailcat_serve","enabled":false}))
            .send()
            .await?
            .json()
            .await?;
        assert!(changed.as_array().is_some_and(|items| items
            .iter()
            .any(|tool| tool["name"] == "tailcat_serve" && tool["enabled"] == false)));
        let persisted =
            crate::config::load_or_default_unvalidated(&dir.path().join("config.toml"))?;
        assert!(persisted.tool_groups["tailcat"]);
        assert_eq!(persisted.tool_overrides.get("tailcat_serve"), Some(&false));
        let apps: serde_json::Value = client
            .get(format!("{base}/api/apps"))
            .send()
            .await?
            .json()
            .await?;
        assert!(apps.as_array().is_some());
        std::fs::write(dir.path().join("web-reference.txt"), "example")?;
        let suggestions: Vec<String> = client
            .get(format!("{base}/api/file-suggestions"))
            .query(&[(
                "fragment",
                dir.path().join("web-ref").to_string_lossy().to_string(),
            )])
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(
            suggestions,
            vec![dir.path().join("web-reference.txt").to_string_lossy()]
        );
        let mut events = client
            .get(format!("{base}/api/events?id={id}"))
            .send()
            .await?;
        let first = tokio::time::timeout(std::time::Duration::from_secs(2), events.chunk())
            .await??
            .context("missing initial SSE event")?;
        assert!(std::str::from_utf8(&first)?.contains("connected"));

        let (mut terminal, _) =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/api/terminal/{id}"))
                .await?;
        terminal
            .send(tokio_tungstenite::tungstenite::Message::Text(
                "not-json".into(),
            ))
            .await?;
        let reply = tokio::time::timeout(std::time::Duration::from_secs(2), terminal.next())
            .await?
            .context("missing WebSocket reply")??;
        assert!(reply.to_text()?.contains("invalid terminal message"));
        Ok(())
    }

    #[tokio::test]
    async fn occupied_web_port_selects_an_available_port() -> Result<()> {
        let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let port = occupied.local_addr()?.port();
        let fallback = bind_web_listener(port).await?;
        assert_ne!(fallback.local_addr()?.port(), port);
        Ok(())
    }

    #[tokio::test]
    async fn exports_selected_conversation_and_shared_log_as_zip() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("config.toml");
        std::fs::write(dir.path().join("nl2sh.log"), b"{\"event\":\"test\"}\n")?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let server = start_with_listener(path, listener).await?;
        let port = url::Url::parse(server.url())?
            .port()
            .context("missing port")?;
        let base = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::builder().no_proxy().build()?;
        let sessions: serde_json::Value = client
            .get(format!("{base}/api/sessions"))
            .send()
            .await?
            .json()
            .await?;
        let id = sessions[0]["id"].as_str().context("missing session id")?;
        let response = client
            .get(format!("{base}/api/sessions/export?id={id}"))
            .send()
            .await?;
        assert!(response.status().is_success());
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/zip");
        let bytes = response.bytes().await?;
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
        let mut conversation = String::new();
        archive
            .by_name("conversation.json")?
            .read_to_string(&mut conversation)?;
        let conversation: serde_json::Value = serde_json::from_str(&conversation)?;
        assert_eq!(conversation["id"], id);
        assert_eq!(conversation["entries"].as_array().map(Vec::len), Some(0));
        let mut log = String::new();
        archive.by_name("nl2sh.log")?.read_to_string(&mut log)?;
        assert_eq!(log, "{\"event\":\"test\"}\n");
        assert_eq!(
            client
                .get(format!("{base}/api/sessions/export?id=missing"))
                .send()
                .await?
                .status(),
            StatusCode::BAD_REQUEST
        );
        Ok(())
    }

    #[tokio::test]
    async fn listed_saved_session_can_be_loaded_with_its_history() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("config.toml");
        let turns = vec![vec![
            ConversationItem::Message(ConversationMessage::new(Role::User, "历史问题")),
            ConversationItem::Message(ConversationMessage::new(Role::Assistant, "历史回答")),
        ]];
        SessionStore::open(&path)?.save_redacted_with_title(
            "saved-chat",
            "历史会话",
            &turns,
            1024,
            &[],
        )?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let server = start_with_listener(path, listener).await?;
        let port = url::Url::parse(server.url())?
            .port()
            .context("web URL lacks port")?;
        let base = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::builder().no_proxy().build()?;
        let listed: serde_json::Value = client
            .get(format!("{base}/api/sessions"))
            .send()
            .await?
            .json()
            .await?;
        assert!(listed
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["id"] == "saved-chat")));
        let loaded = client
            .post(format!("{base}/api/sessions/load"))
            .json(&serde_json::json!({"name": "saved-chat"}))
            .send()
            .await?;
        assert!(loaded.status().is_success());
        let snapshot: serde_json::Value = client
            .get(format!("{base}/api/state?id=saved-chat"))
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(snapshot["entries"][0]["text"], "历史问题");
        assert_eq!(snapshot["entries"][1]["text"], "历史回答");
        Ok(())
    }

    #[tokio::test]
    async fn unfinished_web_request_recovers_as_diagnostic_history() -> Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let mut initial = SessionState::empty();
        initial.id = "web-test".into();
        let original = Arc::new(Shared {
            started: Instant::now(),
            port: 0,
            path: path.clone(),
            sessions: Mutex::new(BTreeMap::from([("web-test".into(), web_session(initial))])),
        });
        let current = session(&original, "web-test")?;
        {
            let mut inner = current.inner.lock().map_err(|_| anyhow!("poisoned"))?;
            inner.busy = true;
            inner.turns.push(vec![
                ConversationItem::Message(ConversationMessage::new(Role::User, "旧问题")),
                ConversationItem::Message(ConversationMessage::new(Role::Assistant, "旧回答")),
            ]);
            inner.history = render_turns(&inner.turns);
            inner.checkpoint_start = Some(inner.history.len());
            inner.history.extend([
                "> 检查设备".into(),
                "🔧 inspect_android_environment".into(),
                "\u{1e}TOOL_OK:设备已检查".into(),
                format!("{LIVE_TOOL_CALL_PREFIX}pending\tread_file"),
                format!("{LIVE_TOOL_PENDING_PREFIX}pending"),
            ]);
            inner.checkpoint = Some(WebCheckpoint {
                status: "running".into(),
                activity: "thinking".into(),
                history: Vec::new(),
                events: vec![WebCheckpointEvent {
                    at: 1,
                    kind: "tool_completed".into(),
                    tool: None,
                }],
            });
        }
        persist_web_session(&original, &current).await?;
        let restarted = Arc::new(Shared {
            started: Instant::now(),
            port: 0,
            path: path.clone(),
            sessions: Mutex::new(BTreeMap::new()),
        });
        let Json(_loaded) = load_session(
            State(restarted.clone()),
            Json(SessionSelection {
                name: "web-test".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        let Json(snapshot) = get_state(
            State(restarted),
            Query(StateQuery {
                id: "web-test".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        assert!(!snapshot.busy);
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.kind == "tool_result"));
        assert!(snapshot.entries.iter().any(|entry| {
            entry.kind == "tool_error" && entry.text.contains("执行状态未知")
        }));
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.text.contains("上次任务")));
        assert_eq!(
            SessionStore::open(&path)?.load("web-test", 10, 1024)?.len(),
            1
        );
        assert_eq!(
            snapshot
                .entries
                .iter()
                .filter(|entry| entry.text == "旧问题")
                .count(),
            1
        );
        assert_eq!(
            SessionStore::open(&path)?
                .load_web_checkpoint("web-test")?
                .context("missing checkpoint")?
                .status,
            "interrupted"
        );
        Ok(())
    }
}
