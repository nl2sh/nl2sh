use super::*;

pub(in crate::web) struct Shared {
    pub(in crate::web) path: PathBuf,
    pub(in crate::web) started: Instant,
    pub(in crate::web) port: u16,
    pub(in crate::web) sessions: Mutex<BTreeMap<String, Arc<WebSession>>>,
    pub(in crate::web) update: Mutex<UpdateJob>,
}

pub(in crate::web) struct WebSession {
    pub(in crate::web) inner: Mutex<SessionState>,
    pub(in crate::web) persist: AsyncMutex<()>,
    pub(in crate::web) events: broadcast::Sender<String>,
    pub(in crate::web) cancel: watch::Sender<bool>,
    pub(in crate::web) terminal_clients: AtomicUsize,
    pub(in crate::web) audit: Mutex<Option<tokio::sync::mpsc::Sender<AuditEvent>>>,
}

pub(in crate::web) struct SessionState {
    pub(in crate::web) id: String,
    pub(in crate::web) title: String,
    pub(in crate::web) history: Vec<String>,
    pub(in crate::web) turns: Vec<Vec<ConversationItem>>,
    pub(in crate::web) busy: bool,
    pub(in crate::web) pending: Option<Pending>,
    pub(in crate::web) reply: Option<oneshot::Sender<Reply>>,
    pub(in crate::web) title_generated: bool,
    pub(in crate::web) created: u64,
    pub(in crate::web) updated: u64,
    pub(in crate::web) steps: usize,
    pub(in crate::web) tool_calls: usize,
    pub(in crate::web) accepted_turns: usize,
    pub(in crate::web) task: WebTaskMetrics,
    pub(in crate::web) task_started: Option<Instant>,
    pub(in crate::web) phase_started: Instant,
    pub(in crate::web) token_base: (u64, u64),
    pub(in crate::web) input_tokens: u64,
    pub(in crate::web) output_tokens: u64,
    pub(in crate::web) final_input_tokens: Option<u64>,
    pub(in crate::web) activity: &'static str,
    pub(in crate::web) activity_detail: Option<String>,
    pub(in crate::web) activity_since_ms: u64,
    pub(in crate::web) checkpoint: Option<WebCheckpoint>,
    pub(in crate::web) checkpoint_start: Option<usize>,
    pub(in crate::web) redaction_secrets: Vec<String>,
}

impl SessionState {
    pub(in crate::web) fn empty() -> Self {
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

pub(in crate::web) fn web_session(inner: SessionState) -> Arc<WebSession> {
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

pub(in crate::web) fn notify(session: &WebSession, event: &str) {
    let _ = session.events.send(event.to_owned());
}

pub(in crate::web) fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

pub(in crate::web) fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

pub(in crate::web) fn task_metrics(inner: &SessionState) -> WebTaskMetrics {
    let mut task = inner.task.clone();
    task.steps = inner.steps;
    task.tool_calls = inner.tool_calls;
    if let Some(started) = inner.task_started {
        task.total_ms = started.elapsed().as_millis() as u64;
        let elapsed = inner.phase_started.elapsed().as_millis() as u64;
        match inner.activity {
            "thinking" => task.model_ms = task.model_ms.saturating_add(elapsed),
            "tool" => task.tool_ms = task.tool_ms.saturating_add(elapsed),
            "waiting" | "background" => task.waiting_ms = task.waiting_ms.saturating_add(elapsed),
            _ => {}
        }
    }
    task
}

pub(in crate::web) fn presentation(inner: &SessionState) -> WebPresentation {
    WebPresentation {
        history: inner.history.clone(),
        turns: inner.accepted_turns,
        task: task_metrics(inner),
        input_tokens: inner.input_tokens,
        output_tokens: inner.output_tokens,
        final_input_tokens: inner.final_input_tokens,
    }
}

pub(in crate::web) fn set_activity(
    inner: &mut SessionState,
    activity: &'static str,
    detail: Option<&str>,
) {
    if inner.activity != activity || inner.activity_detail.as_deref() != detail {
        inner.task = task_metrics(inner);
        inner.phase_started = Instant::now();
        inner.activity = activity;
        inner.activity_detail = detail.map(str::to_owned);
        inner.activity_since_ms = unix_millis();
    }
}

pub(in crate::web) fn task_summary(inner: &SessionState) -> String {
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

pub(in crate::web) fn finish_task(inner: &mut SessionState) {
    set_activity(inner, "idle", None);
    inner.task = task_metrics(inner);
    inner.task_started = None;
}

pub(in crate::web) enum AuditEvent {
    Record(String, String),
    Flush(oneshot::Sender<()>),
}

pub(in crate::web) async fn flush_audit(session: &WebSession) {
    let sender = session.audit.lock().ok().and_then(|value| value.clone());
    if let Some(sender) = sender {
        let (tx, rx) = oneshot::channel();
        if sender.send(AuditEvent::Flush(tx)).await.is_ok() {
            let _ = rx.await;
        }
    }
}

pub(in crate::web) fn audit(session: &WebSession, event: &str, message: &str) {
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

pub(in crate::web) async fn start_audit(
    path: PathBuf,
    cfg: &Config,
    session: &WebSession,
) -> Result<()> {
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

pub(in crate::web) fn checkpoint_event(inner: &mut SessionState, kind: &str, tool: Option<&str>) {
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

pub(in crate::web) fn remember_redaction_secrets(inner: &mut SessionState, cfg: &Config) {
    for secret in [
        &cfg.api_key,
        &cfg.proxy_password,
        &cfg.protocol_token,
        &cfg.ima_client_id,
        &cfg.ima_api_key,
        &cfg.jev_api_key,
    ] {
        if !secret.is_empty() && !inner.redaction_secrets.contains(secret) {
            inner.redaction_secrets.push(secret.clone());
        }
    }
}

pub(in crate::web) async fn persist_web_session(
    state: &Shared,
    current: &WebSession,
) -> Result<()> {
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
            cfg.protocol_token,
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
pub(in crate::web) enum Pending {
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
pub(in crate::web) struct WebQuestion {
    pub(in crate::web) id: String,
    pub(in crate::web) header: String,
    pub(in crate::web) prompt: String,
    pub(in crate::web) options: Vec<WebOption>,
}

#[derive(Clone, Serialize)]
pub(in crate::web) struct WebOption {
    pub(in crate::web) label: String,
    pub(in crate::web) value: String,
    pub(in crate::web) description: String,
}

pub(in crate::web) enum Reply {
    Approval(ConfirmationDecision),
    Questions(Option<QuestionAnswers>),
}

#[derive(Serialize)]
pub(in crate::web) struct Snapshot {
    pub(in crate::web) id: String,
    pub(in crate::web) title: String,
    pub(in crate::web) history: Vec<String>,
    pub(in crate::web) entries: Vec<WebEntry>,
    pub(in crate::web) busy: bool,
    pub(in crate::web) pending: Option<Pending>,
    pub(in crate::web) turns: usize,
    pub(in crate::web) steps: usize,
    pub(in crate::web) tool_calls: usize,
    pub(in crate::web) input_tokens: u64,
    pub(in crate::web) output_tokens: u64,
    pub(in crate::web) final_input_tokens: Option<u64>,
    pub(in crate::web) activity: &'static str,
    pub(in crate::web) activity_detail: Option<String>,
    pub(in crate::web) activity_elapsed_ms: u64,
    pub(in crate::web) task: WebTaskMetrics,
    pub(in crate::web) can_retry: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(in crate::web) struct WebEntry {
    pub(in crate::web) kind: &'static str,
    pub(in crate::web) text: String,
}

pub(in crate::web) fn session(state: &Shared, id: &str) -> Result<Arc<WebSession>> {
    state
        .sessions
        .lock()
        .map_err(|_| anyhow!("web session registry lock poisoned"))?
        .get(id)
        .cloned()
        .context("web session not found")
}

pub(in crate::web) fn web_entries(history: &[String]) -> Vec<WebEntry> {
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

pub(in crate::web) fn append_tool_round(lines: &mut Vec<String>, round: &ToolRound) {
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

pub(in crate::web) fn turns_tool_count(turns: &[Vec<ConversationItem>]) -> usize {
    turns
        .iter()
        .flatten()
        .map(|item| match item {
            ConversationItem::Tools(round) => round.calls.len(),
            _ => 0,
        })
        .sum()
}

pub(in crate::web) fn render_turns(turns: &[Vec<ConversationItem>]) -> Vec<String> {
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

pub(in crate::web) fn push(inner: &mut SessionState, line: String) {
    inner.history.push(line);
    if inner.history.len() > MAX_HISTORY {
        let excess = inner.history.len() - MAX_HISTORY;
        inner.history.drain(..excess);
        if let Some(start) = inner.checkpoint_start.as_mut() {
            *start = start.saturating_sub(excess);
        }
    }
}

pub(in crate::web) fn clear_live_tool_output(inner: &mut SessionState) {
    inner
        .history
        .retain(|line| !line.starts_with("[OUT]") && !line.starts_with("[ERR]"));
}

pub(in crate::web) fn mark_unresolved_tools(history: &mut [String]) {
    for line in history {
        if line.starts_with(LIVE_TOOL_PENDING_PREFIX) {
            *line = "\u{1e}TOOL_ERR:任务中断，工具结果未记录；执行状态未知".into();
        }
    }
}

pub(in crate::web) async fn wait_web_cancel(mut receiver: watch::Receiver<bool>) {
    while !*receiver.borrow() {
        if receiver.changed().await.is_err() {
            return;
        }
    }
}
