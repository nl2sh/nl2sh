use super::*;

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
    pub(in crate::web) protocol: Option<crate::protocol::HttpServer>,
    pub(in crate::web) url: String,
    pub(in crate::web) task: tokio::task::JoinHandle<std::io::Result<()>>,
    pub(in crate::web) shutdown: tokio::sync::oneshot::Sender<()>,
    pub(in crate::web) shared: Arc<Shared>,
}

impl WebServer {
    /// Actual bound port, including fallback to an ephemeral port.
    pub fn port(&self) -> u16 {
        self.shared.port
    }

    /// Cancel active tasks and pending approvals, then drain HTTP connections.
    pub async fn shutdown(mut self) -> Result<()> {
        let protocol_result = match self.protocol.as_mut() {
            Some(server) => server.shutdown().await,
            None => Ok(()),
        };
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
        let result = match tokio::time::timeout(std::time::Duration::from_secs(5), &mut task).await
        {
            Ok(result) => result
                .context("web shutdown task failed")?
                .context("web shutdown failed"),
            Err(_) => {
                task.abort();
                let _ = task.await;
                Ok(())
            }
        };
        result.and(protocol_result)
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

pub(in crate::web) async fn bind_web_listener(port: u16) -> Result<TcpListener> {
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

pub(in crate::web) async fn start_with_listener(
    path: PathBuf,
    listener: TcpListener,
) -> Result<WebServer> {
    let config = crate::config::load_or_default_unvalidated(&path)?;
    config.validate_runtime()?;
    let protocol = if config.protocol_start_with_service
        && crate::protocol::connection_info(&path).await.state != "running"
    {
        Some(
            crate::protocol::start_http(
                path.clone(),
                Ipv4Addr::UNSPECIFIED.into(),
                config.protocol_service_port,
                None,
                true,
            )
            .await
            .context("cannot start MCP/A2A with the UI service")?,
        )
    } else {
        None
    };
    let port = listener
        .local_addr()
        .context("cannot identify web port")?
        .port();
    let ip = crate::network::local_ipv4().unwrap_or(Ipv4Addr::LOCALHOST);
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
        update: Mutex::new(UpdateJob::default()),
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
        protocol,
        url,
        task,
        shutdown,
        shared,
    })
}

#[derive(RustEmbed)]
#[folder = "$NL2SH_WEB_DIST/"]
pub(in crate::web) struct WebAssets;

pub(in crate::web) fn router(state: Arc<Shared>) -> Router {
    Router::new()
        .route("/healthz", get(|| async { Json(serde_json::json!({"status":"ok"})) }))
        .route("/api/info", get(get_info))
        .route("/api/connections", get(get_connections))
        .route("/api/update", get(get_update).post(start_update))
        .route("/api/update/progress", get(update_progress))
        .route("/api/version", get(|| async { env!("CARGO_PKG_VERSION") }))
        .route("/api/state", get(get_state))
        .route("/api/config", get(get_config).post(save_config))
        .route("/api/config/validate", post(validate_config))
        .route("/api/config/render", post(render_config))
        .route("/api/quick-settings", get(get_quick_settings).post(save_quick_settings))
        .route("/api/models", get(get_models).post(get_draft_models))
        .route("/api/model-check", post(check_model))
        .route("/api/device-overview", get(get_device_overview))
        .route("/api/tailcat", get(get_tailcat).post(start_tailcat))
        .route("/api/tailcat/ports", get(get_tailcat_ports))
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

pub(in crate::web) async fn static_asset(uri: Uri) -> Response {
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
