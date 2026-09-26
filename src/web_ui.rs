//! Small embedded HTTP interface. All Agent actions use the same runner and confirmer as the TUI.
use crate::{
    agent::{AgentRunner, ConfirmationDecision, Confirmer, QuestionAnswers, UserQuestion},
    config::{self, Config, ConfirmPolicy, ExecuteUserMode},
    file_references::{augment_file_references, file_suggestions},
    llm::{
        build_client, ConversationItem, ConversationMessage, LlmClient, LlmRequest, Role,
        TextDeltaSink, ToolRound,
    },
    provider_metadata::build_metadata_client,
    security::SecurityAssessment,
    session_title::generate_title,
    sessions::{SessionStore, WebCheckpoint, WebCheckpointEvent},
    shell::{CommandExecutor, ExecutionResult, OutputSink, ShellExecutor, SystemRootProbe},
    tools::{android::environment::inspect_environment, Capability, ToolRegistry},
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
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
};
use tokio::{
    net::TcpListener,
    sync::{broadcast, oneshot, watch, Mutex as AsyncMutex},
};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tower_http::set_header::SetResponseHeaderLayer;

const PORT: u16 = 9999;
const MAX_REQUEST: usize = 256 * 1024;
const MAX_HISTORY: usize = 400;
const MAX_WEB_SESSIONS: usize = 64;
const MAX_EXPORT_LOG_BYTES: u64 = 20 * 1024 * 1024;
const LIVE_TOOL_CALL_PREFIX: &str = "\u{1e}TOOL_CALL:";
const LIVE_TOOL_PENDING_PREFIX: &str = "\u{1e}TOOL_PENDING:";
static WELCOME_URL: OnceLock<String> = OnceLock::new();
static SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

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
}

impl WebServer {
    /// Address shown in the terminal welcome message.
    pub fn url(&self) -> &str {
        &self.url
    }
}

struct Shared {
    path: PathBuf,
    sessions: Mutex<BTreeMap<String, Arc<WebSession>>>,
}

struct WebSession {
    inner: Mutex<SessionState>,
    persist: AsyncMutex<()>,
    events: broadcast::Sender<String>,
    cancel: watch::Sender<bool>,
    terminal_clients: AtomicUsize,
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

fn set_activity(inner: &mut SessionState, activity: &'static str, detail: Option<&str>) {
    if inner.activity != activity || inner.activity_detail.as_deref() != detail {
        inner.activity = activity;
        inner.activity_detail = detail.map(str::to_owned);
        inner.activity_since_ms = unix_millis();
    }
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
    let (id, title, turns, checkpoint, prior_secrets) = {
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
        SessionStore::open(&path)?.save_web_state(
            &id,
            &title,
            &turns,
            cfg.model_tool_output_max_bytes,
            &secrets,
            checkpoint.as_ref(),
        )
    })
    .await??;
    Ok(())
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Pending {
    Approval {
        command: String,
        risk: String,
        explanation: String,
        root: bool,
        strong: bool,
    },
    Questions {
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
        path,
        sessions: Mutex::new(sessions),
    });
    let app = router(shared);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(WebServer { url })
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

fn router(state: Arc<Shared>) -> Router {
    Router::new()
        .route("/api/state", get(get_state))
        .route("/api/config", get(get_config).post(save_config))
        .route("/api/config/validate", post(validate_config))
        .route("/api/config/render", post(render_config))
        .route("/api/quick-settings", get(get_quick_settings).post(save_quick_settings))
        .route("/api/models", get(get_models))
        .route("/api/model-check", post(check_model))
        .route("/api/device-overview", get(get_device_overview))
        .route("/api/tools", get(get_tools))
        .route("/api/file-suggestions", get(get_file_suggestions))
        .route("/api/sessions", get(get_sessions))
        .route("/api/sessions/new", post(new_session))
        .route("/api/sessions/load", post(load_session))
        .route("/api/sessions/delete", post(delete_session))
        .route("/api/sessions/delete-all", post(delete_all_sessions))
        .route("/api/sessions/export", get(export_session))
        .route("/api/message", post(post_message))
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
        turns: inner.turns.len(),
        steps: inner.steps,
        tool_calls: inner.tool_calls,
        input_tokens: inner.input_tokens,
        output_tokens: inner.output_tokens,
        final_input_tokens: inner.final_input_tokens,
        activity: inner.activity,
        activity_detail: inner.activity_detail.clone(),
        activity_elapsed_ms: unix_millis().saturating_sub(inner.activity_since_ms),
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
    let models = build_metadata_client(&cfg).list_models(&cfg).await?;
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
}

async fn get_tools(State(state): State<Arc<Shared>>) -> ApiResult<Json<Vec<ToolSummary>>> {
    let cfg = load_config(state.path.clone()).await?;
    let capabilities = if cfg.ima_enabled {
        vec![Capability::Ima]
    } else {
        Vec::new()
    };
    let registry = ToolRegistry::builtin(&capabilities);
    Ok(Json(
        registry
            .definitions()
            .into_iter()
            .filter_map(|tool| {
                let metadata = registry.get(&tool.name)?.metadata();
                Some(ToolSummary {
                    name: tool.name,
                    description: tool.description,
                    category: format!("{:?}", metadata.category),
                    risk: format!("{:?}", metadata.risk),
                })
            })
            .collect(),
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
                    turns: inner.turns.len(),
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
                turns: inner.turns.len(),
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
        let mut history = render_turns(&turns);
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
                history.extend(current.history.clone());
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
            steps: 0,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            final_input_tokens: None,
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
    let path = state.path.clone();
    let cfg =
        tokio::task::spawn_blocking(move || config::load_or_default_unvalidated(&path)).await??;
    let current = {
        let sessions = state
            .sessions
            .lock()
            .map_err(|_| anyhow!("web session registry lock poisoned"))?;
        let current = sessions
            .get(&message.session_id)
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
        remember_redaction_secrets(&mut inner, &cfg);
        current.cancel.send_replace(false);
        set_activity(&mut inner, "thinking", None);
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
    if let Err(error) = persist_web_session(&state, &current).await {
        if let Ok(mut inner) = current.inner.lock() {
            inner.busy = false;
            inner.checkpoint = None;
            inner.checkpoint_start = None;
            set_activity(&mut inner, "idle", None);
            push(&mut inner, format!("❌ 无法保存会话检查点：{error:#}"));
        }
        return Err(error.into());
    }
    notify(&current, "state");
    tokio::spawn(run_message(state, current, text));
    Ok((StatusCode::ACCEPTED, "任务已开始".into()))
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
    let pending = inner.pending.as_ref().context("no pending request")?;
    let reply = match pending {
        Pending::Approval { strong, root, .. } => Reply::Approval(match decision.action.as_str() {
            "approve" if !strong || decision.text.as_deref() == Some("CONFIRM") => {
                ConfirmationDecision::ApproveCaptured
            }
            "remember" if !strong && !root => ConfirmationDecision::ApproveForTask,
            "reject" => ConfirmationDecision::Reject,
            "edit" => ConfirmationDecision::Edit(decision.text.unwrap_or_default()),
            _ => {
                return Err(ApiError::bad(anyhow!(
                    "invalid decision or missing CONFIRM"
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
    while assessment.requires_confirmation {
        match confirmer.confirm(&command, &assessment).await? {
            ConfirmationDecision::Approve
            | ConfirmationDecision::ApproveCaptured
            | ConfirmationDecision::ApproveInteractive
            | ConfirmationDecision::ApproveForTask
            | ConfirmationDecision::ApproveForRun => break,
            ConfirmationDecision::Reject => bail!("command rejected"),
            ConfirmationDecision::Edit(edited) => {
                command = edited;
                assessment = crate::security::assess(&command, &cfg)
            }
        }
    }
    let output = Arc::new(WebOutput {
        session: current.clone(),
    });
    let executor = CapturedExecutor(ShellExecutor::new(cfg).with_output(output));
    let result = executor
        .execute(&command, assessment.requires_root, false)
        .await?;
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
        inner.steps = inner.steps.saturating_add(result.steps);
        inner.tool_calls = inner.tool_calls.saturating_add(result.tool_calls);
        inner.input_tokens = inner
            .input_tokens
            .saturating_add(result.usage.input_tokens.unwrap_or(0));
        inner.output_tokens = inner
            .output_tokens
            .saturating_add(result.usage.output_tokens.unwrap_or(0));
        inner.final_input_tokens = result.final_input_tokens;
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
        inner.busy = false;
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

    fn tool_started(&self, call_id: &str, name: &str) {
        if let Ok(mut inner) = self.session.inner.lock() {
            checkpoint_event(&mut inner, "tool_started", Some(name));
            push(
                &mut inner,
                format!("{LIVE_TOOL_CALL_PREFIX}{call_id}\t{name}"),
            );
            push(&mut inner, format!("{LIVE_TOOL_PENDING_PREFIX}{call_id}"));
        }
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
            command: command.to_owned(),
            risk: format!("{:?}", assessment.risk_level),
            explanation: approval_explanation(assessment),
            root: assessment.requires_root,
            strong: assessment.requires_double_confirmation,
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
        match self.wait(Pending::Questions { questions }).await? {
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
        {
            let mut inner = current
                .inner
                .lock()
                .map_err(|_| anyhow!("web lock poisoned"))?;
            inner.busy = true;
            inner.pending = Some(Pending::Approval {
                command: "touch /tmp/x".into(),
                risk: "Mutating".into(),
                explanation: "test".into(),
                root: false,
                strong: false,
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

    #[test]
    fn approval_explanation_uses_local_rule_evidence() {
        let assessment = crate::security::assess("rm -rf /", &Config::default());
        assert_eq!(assessment.risk_level, crate::security::RiskLevel::Critical);
        assert_eq!(approval_explanation(&assessment), "检测到递归删除关键目录");
    }

    fn state(path: PathBuf) -> Arc<Shared> {
        let mut inner = SessionState::empty();
        inner.id = "web-test".into();
        let current = web_session(inner);
        Arc::new(Shared {
            path,
            sessions: Mutex::new(BTreeMap::from([("web-test".into(), current)])),
        })
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
