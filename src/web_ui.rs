//! Small embedded HTTP interface. All Agent actions use the same runner and confirmer as the TUI.
use crate::{
    agent::{AgentRunner, ConfirmationDecision, Confirmer, QuestionAnswers, UserQuestion},
    config::{self, Config, ConfirmPolicy},
    llm::{build_client, ConversationItem, ConversationMessage, LlmRequest, Role, TextDeltaSink},
    provider_metadata::build_metadata_client,
    security::SecurityAssessment,
    sessions::SessionStore,
    shell::{CommandExecutor, ExecutionResult, OutputSink, ShellExecutor, SystemRootProbe},
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
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
};
use tokio::{
    net::TcpListener,
    sync::{broadcast, oneshot},
};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tower_http::set_header::SetResponseHeaderLayer;

const PORT: u16 = 9999;
const MAX_REQUEST: usize = 256 * 1024;
const MAX_HISTORY: usize = 400;
const MAX_WEB_SESSIONS: usize = 64;
const MAX_EXPORT_LOG_BYTES: u64 = 20 * 1024 * 1024;
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
    events: broadcast::Sender<String>,
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
    updated: u64,
    steps: usize,
    tool_calls: usize,
    input_tokens: u64,
    output_tokens: u64,
    final_input_tokens: Option<u64>,
}

impl SessionState {
    fn empty() -> Self {
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
            updated: unix_seconds(),
            steps: 0,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            final_input_tokens: None,
        }
    }
}

fn web_session(inner: SessionState) -> Arc<WebSession> {
    let (events, _) = broadcast::channel(128);
    Arc::new(WebSession {
        inner: Mutex::new(inner),
        events,
    })
}

fn notify(session: &WebSession, event: &str) {
    let _ = session.events.send(event.to_owned());
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
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
}

#[derive(Serialize)]
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
}

#[derive(Serialize)]
struct SessionSummary {
    id: String,
    title: String,
    turns: usize,
    busy: bool,
    pending: bool,
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
        .route("/api/quick-settings", get(get_quick_settings).post(save_quick_settings))
        .route("/api/models", get(get_models))
        .route("/api/sessions", get(get_sessions))
        .route("/api/sessions/new", post(new_session))
        .route("/api/sessions/load", post(load_session))
        .route("/api/sessions/export", get(export_session))
        .route("/api/message", post(post_message))
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
        .map(|line| {
            let (kind, text) = if let Some(text) = line.strip_prefix("> ") {
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
            WebEntry {
                kind,
                text: text.to_owned(),
            }
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
    archive.write_all(b"conversation.json contains the selected Web session's visible conversation at export time.\nnl2sh.log is the shared audit log and may contain events from other sessions. An empty log means no log file existed.\n")?;
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
    Ok("配置已保存；后续 Web 任务使用新配置，当前 TUI 会话重启后生效。".into())
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
        updated: inner.updated,
    };
    let id = inner.id.clone();
    drop(inner);
    sessions.insert(id, current);
    Ok(Json(summary))
}

async fn get_sessions(State(state): State<Arc<Shared>>) -> ApiResult<Json<Vec<SessionSummary>>> {
    let runtime: Vec<_> = state
        .sessions
        .lock()
        .map_err(|_| anyhow!("web session registry lock poisoned"))?
        .values()
        .filter_map(|s| {
            s.inner.lock().ok().map(|i| SessionSummary {
                id: i.id.clone(),
                title: i.title.clone(),
                turns: i.turns.len(),
                busy: i.busy,
                pending: i.pending.is_some(),
                updated: i.updated,
            })
        })
        .collect();
    let path = state.path.clone();
    let stored = tokio::task::spawn_blocking(move || SessionStore::open(&path)?.list()).await??;
    let mut all = runtime;
    for saved in stored {
        if !all.iter().any(|s| s.id == saved.name) {
            all.push(SessionSummary {
                id: saved.name,
                title: saved.title,
                turns: saved.turns,
                busy: false,
                pending: false,
                updated: saved.updated_unix_secs,
            })
        }
    }
    all.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| b.id.cmp(&a.id)));
    Ok(Json(all))
}

async fn load_session(
    State(state): State<Arc<Shared>>,
    Json(selection): Json<SessionSelection>,
) -> ApiResult<Json<SessionSummary>> {
    if let Ok(existing) = session(&state, &selection.name) {
        let i = existing
            .inner
            .lock()
            .map_err(|_| anyhow!("web session lock poisoned"))?;
        return Ok(Json(SessionSummary {
            id: i.id.clone(),
            title: i.title.clone(),
            turns: i.turns.len(),
            busy: i.busy,
            pending: i.pending.is_some(),
            updated: i.updated,
        }));
    }
    let path = state.path.clone();
    let name = selection.name;
    let (turns, info) = tokio::task::spawn_blocking(move || -> Result<_> {
        let cfg = config::load_or_default_unvalidated(&path)?;
        let store = SessionStore::open(&path)?;
        let info = store
            .list()?
            .into_iter()
            .find(|i| i.name == name)
            .context("saved session not found")?;
        let turns = store.load(
            &name,
            cfg.max_context_turns,
            cfg.model_tool_output_max_bytes,
        )?;
        Ok((turns, info))
    })
    .await??;
    let current = web_session(SessionState {
        id: info.name.clone(),
        title: info.title.clone(),
        history: render_turns(&turns),
        turns,
        busy: false,
        pending: None,
        reply: None,
        title_generated: true,
        updated: info.updated_unix_secs,
        steps: 0,
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        final_input_tokens: None,
    });
    state
        .sessions
        .lock()
        .map_err(|_| anyhow!("web session registry lock poisoned"))?
        .insert(info.name.clone(), current);
    Ok(Json(SessionSummary {
        id: info.name,
        title: info.title,
        turns: info.turns,
        busy: false,
        pending: false,
        updated: info.updated_unix_secs,
    }))
}

async fn post_message(
    State(state): State<Arc<Shared>>,
    Json(message): Json<Message>,
) -> ApiResult<(StatusCode, String)> {
    let text = message.text.trim().to_owned();
    if text.is_empty() || text.len() > 16 * 1024 {
        return Err(ApiError::bad(anyhow!("message must contain 1–16384 bytes")));
    }
    let current = session(&state, &message.session_id)?;
    let mut inner = current
        .inner
        .lock()
        .map_err(|_| anyhow!("web session lock poisoned"))?;
    if inner.busy || inner.pending.is_some() {
        return Err(ApiError::conflict("this Agent session is already running"));
    }
    inner.busy = true;
    inner.updated = unix_seconds();
    push(&mut inner, format!("> {text}"));
    drop(inner);
    notify(&current, "state");
    tokio::spawn(run_message(state, current, text));
    Ok((StatusCode::ACCEPTED, "任务已开始".into()))
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
                ConversationItem::Tools(round) => {
                    for call in &round.calls {
                        lines.push(format!("🔧 {}", call.name));
                    }
                    for result in &round.results {
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
        inner.history.drain(..inner.history.len() - MAX_HISTORY);
    }
}

async fn run_message(state: Arc<Shared>, current: Arc<WebSession>, text: String) {
    let result = run_agent(state.clone(), current.clone(), text).await;
    if let Ok(mut inner) = current.inner.lock() {
        if let Err(error) = result {
            push(&mut inner, format!("❌ {error:#}"));
        }
        inner.busy = false;
        inner.pending = None;
        inner.reply = None;
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
    if !cfg.provider_is_configured() {
        return Err(anyhow!("请先在 Web 配置页填写模型服务设置"));
    }
    let llm = build_client(&cfg)?;
    let output = Arc::new(WebOutput {
        session: current.clone(),
    });
    let executor = CapturedExecutor(ShellExecutor::new(cfg.clone()).with_output(output));
    let confirmer = WebConfirmer {
        session: current.clone(),
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
    };
    let result = runner
        .run_with_history_streaming_owned(text.clone(), history, &sink)
        .await?;
    let generated_title = if needs_title {
        generate_title(&llm, &cfg, &text, &result.final_text).await
    } else {
        None
    };
    let (turns, id, title) = {
        let mut inner = current
            .inner
            .lock()
            .map_err(|_| anyhow!("web session lock poisoned"))?;
        inner.history.retain(|line| !line.starts_with("… "));
        for item in &result.transcript {
            if let ConversationItem::Tools(round) = item {
                for call in &round.calls {
                    push(&mut inner, format!("🔧 {}", call.name));
                }
                for output in &round.results {
                    push(
                        &mut inner,
                        format!(
                            "{}{}",
                            if output.success {
                                "\u{1e}TOOL_OK:"
                            } else {
                                "\u{1e}TOOL_ERR:"
                            },
                            output.output
                        ),
                    );
                }
            }
        }
        push(&mut inner, format!("🤖 {}", result.final_text));
        inner.turns.push(result.transcript);
        if inner.turns.len() > cfg.max_context_turns {
            inner.turns.remove(0);
        }
        if let Some(title) = generated_title {
            inner.title = title;
        }
        if needs_title {
            inner.title_generated = true;
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
        (inner.turns.clone(), inner.id.clone(), inner.title.clone())
    };
    notify(&current, "state");
    let secrets = vec![
        cfg.api_key.clone(),
        cfg.proxy_password.clone(),
        cfg.ima_api_key.clone(),
        cfg.jev_api_key.clone(),
    ];
    let path = state.path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        SessionStore::open(&path)?.save_redacted_with_title(
            &id,
            &title,
            &turns,
            cfg.model_tool_output_max_bytes,
            &secrets,
        )
    })
    .await??;
    Ok(())
}

async fn generate_title(
    llm: &dyn crate::llm::LlmClient,
    cfg: &Config,
    input: &str,
    answer: &str,
) -> Option<String> {
    let request = LlmRequest {
        model: cfg.model.clone(),
        items: vec![
            ConversationItem::Message(ConversationMessage::new(Role::System, "Generate a concise title for this conversation. Use the user's language. Output only the title, at most 12 Chinese characters or 8 words. Do not use quotes or punctuation at the ends.")),
            ConversationItem::Message(ConversationMessage::new(Role::User, format!("User: {}\nAssistant: {}", crate::limits::truncate_text(input, 1000), crate::limits::truncate_text(answer, 1000)))),
        ],
        tools: Vec::new(),
    };
    let response = llm.complete(request).await.ok()?;
    normalize_title(response.text.as_deref()?)
}

fn normalize_title(value: &str) -> Option<String> {
    let title = value
        .lines()
        .next()?
        .trim()
        .trim_matches(|c| matches!(c, '"' | '\'' | '`' | '#' | '*' | '：' | ':'))
        .trim();
    if title.is_empty() {
        return None;
    }
    let mut bounded = String::new();
    for character in title.chars().take(48) {
        if bounded.len() + character.len_utf8() > 150 {
            break;
        }
        bounded.push(character);
    }
    (!bounded.is_empty()).then_some(bounded)
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
}
impl TextDeltaSink for WebTextSink {
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
}

struct WebConfirmer {
    session: Arc<WebSession>,
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
            explanation: assessment.explanation.clone(),
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
        let (tx, rx) = oneshot::channel();
        {
            let mut inner = self
                .session
                .inner
                .lock()
                .map_err(|_| anyhow!("web state lock poisoned"))?;
            inner.pending = Some(pending);
            inner.reply = Some(tx);
        }
        notify(&self.session, "pending");
        rx.await.context("browser decision was canceled")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tempfile::tempdir;

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
            normalize_title("**\"检查网络连接\"**\nextra"),
            Some("检查网络连接".into())
        );
        assert_eq!(normalize_title("   "), None);
        assert_eq!(
            normalize_title(&"a".repeat(60)).map(|value| value.chars().count()),
            Some(48)
        );
        assert!(normalize_title(&"😀".repeat(60)).is_some_and(|value| value.len() <= 150));
    }

    #[test]
    fn streaming_deltas_share_one_entry_and_tool_results_are_typed() -> Result<()> {
        let state = state(PathBuf::from("unused.toml"));
        let current = session(&state, "web-test")?;
        let sink = WebTextSink {
            session: current.clone(),
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
    fn export_keeps_selected_entries_when_log_does_not_exist() -> Result<()> {
        let dir = tempdir()?;
        let conversation = ExportConversation {
            id: "selected".into(),
            title: "当前会话".into(),
            exported_unix_secs: 1,
            busy: true,
            entries: web_entries(&["> 本轮问题".into(), "🤖 本轮回答".into()]),
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
}
