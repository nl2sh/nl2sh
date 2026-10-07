use super::super::*;

#[derive(Serialize)]
pub(in crate::web) struct QuickSettings {
    pub(in crate::web) endpoint: String,
    pub(in crate::web) model: String,
    pub(in crate::web) provider_ready: bool,
    pub(in crate::web) confirm_policy: ConfirmPolicy,
    pub(in crate::web) max_context_turns: usize,
    pub(in crate::web) max_agent_steps: usize,
    pub(in crate::web) max_tool_calls: usize,
    pub(in crate::web) context_window: Option<u64>,
    pub(in crate::web) root: String,
}

#[derive(Deserialize)]
pub(in crate::web) struct QuickSettingsUpdate {
    pub(in crate::web) endpoint: String,
    pub(in crate::web) model: String,
    pub(in crate::web) confirm_policy: ConfirmPolicy,
}

pub(in crate::web) async fn get_config(State(state): State<Arc<Shared>>) -> ApiResult<String> {
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

pub(in crate::web) async fn save_config(
    State(state): State<Arc<Shared>>,
    body: String,
) -> ApiResult<String> {
    let path = state.path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let cfg: Config = toml::from_str(&body).context("invalid TOML configuration")?;
        config::save_config(&path, &cfg)
    })
    .await??;
    Ok("配置已保存；后续 Web 任务自动使用新配置，当前 TUI 会话重启后生效。".into())
}

#[derive(Serialize)]
pub(in crate::web) struct ConfigPreview {
    pub(in crate::web) valid: bool,
    pub(in crate::web) error: Option<String>,
    pub(in crate::web) config: Option<Config>,
}

pub(in crate::web) async fn validate_config(body: String) -> Json<ConfigPreview> {
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
pub(in crate::web) struct RenderedConfig {
    pub(in crate::web) toml: String,
    pub(in crate::web) valid: bool,
    pub(in crate::web) error: Option<String>,
}

pub(in crate::web) async fn render_config(
    Json(config): Json<Config>,
) -> ApiResult<Json<RenderedConfig>> {
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

pub(in crate::web) async fn load_config(path: PathBuf) -> Result<Config> {
    tokio::task::spawn_blocking(move || -> Result<Config> {
        let cfg = config::load_or_default_unvalidated(&path)?;
        cfg.validate_runtime()?;
        Ok(cfg)
    })
    .await?
}

pub(in crate::web) async fn get_quick_settings(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<QuickSettings>> {
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

pub(in crate::web) async fn save_quick_settings(
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

pub(in crate::web) async fn get_models(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    let cfg = load_config(state.path.clone()).await?;
    list_models(state.path.clone(), cfg, "saved").await
}

#[derive(Deserialize)]
pub(in crate::web) struct ModelListDraft {
    pub(in crate::web) endpoint: String,
    pub(in crate::web) api_key: String,
}

pub(in crate::web) async fn get_draft_models(
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

pub(in crate::web) async fn record_model_list_event(
    path: PathBuf,
    cfg: &Config,
    event: &'static str,
    data: String,
) {
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

pub(in crate::web) async fn list_models(
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
pub(in crate::web) struct ModelCheck {
    pub(in crate::web) ok: bool,
}

pub(in crate::web) async fn check_model(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<ModelCheck>> {
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
