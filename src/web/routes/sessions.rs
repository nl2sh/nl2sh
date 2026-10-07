use super::super::*;

#[derive(Serialize)]
pub(in crate::web) struct ExportConversation {
    pub(in crate::web) id: String,
    pub(in crate::web) title: String,
    pub(in crate::web) exported_unix_secs: u64,
    pub(in crate::web) busy: bool,
    pub(in crate::web) entries: Vec<WebEntry>,
    pub(in crate::web) task: WebTaskMetrics,
    pub(in crate::web) checkpoint_status: Option<String>,
    pub(in crate::web) checkpoint_events: Vec<WebCheckpointEvent>,
}

#[derive(Serialize)]
pub(in crate::web) struct SessionSummary {
    pub(in crate::web) id: String,
    pub(in crate::web) title: String,
    pub(in crate::web) turns: usize,
    pub(in crate::web) busy: bool,
    pub(in crate::web) pending: bool,
    pub(in crate::web) created: u64,
    pub(in crate::web) updated: u64,
}

#[derive(Deserialize)]
pub(in crate::web) struct Message {
    pub(in crate::web) session_id: String,
    pub(in crate::web) text: String,
}

#[derive(Deserialize)]
pub(in crate::web) struct Decision {
    pub(in crate::web) session_id: String,
    pub(in crate::web) request_id: u64,
    pub(in crate::web) action: String,
    pub(in crate::web) text: Option<String>,
    pub(in crate::web) answers: Option<QuestionAnswers>,
}

#[derive(Deserialize)]
pub(in crate::web) struct SessionSelection {
    pub(in crate::web) name: String,
}

#[derive(Deserialize)]
pub(in crate::web) struct StateQuery {
    pub(in crate::web) id: String,
}

pub(in crate::web) async fn get_state(
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

pub(in crate::web) async fn export_session(
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

pub(in crate::web) fn build_session_export(
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

pub(in crate::web) async fn new_session(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<SessionSummary>> {
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

pub(in crate::web) async fn get_sessions(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<Vec<SessionSummary>>> {
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

pub(in crate::web) async fn load_session(
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

pub(in crate::web) async fn delete_session(
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

pub(in crate::web) async fn delete_all_sessions(
    State(state): State<Arc<Shared>>,
) -> ApiResult<String> {
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

pub(in crate::web) async fn post_message(
    State(state): State<Arc<Shared>>,
    Json(message): Json<Message>,
) -> ApiResult<(StatusCode, String)> {
    let text = message.text.trim().to_owned();
    if text.is_empty() || text.len() > 16 * 1024 {
        return Err(ApiError::bad(anyhow!("message must contain 1–16384 bytes")));
    }
    start_message(state, &message.session_id, text).await
}

pub(in crate::web) async fn retry_message(
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

pub(in crate::web) async fn start_message(
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

pub(in crate::web) fn retryable_input(inner: &SessionState) -> Option<String> {
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

pub(in crate::web) async fn cancel_message(
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

pub(in crate::web) async fn post_decision(
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
