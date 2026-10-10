use super::*;

pub(in crate::web) async fn run_message(
    state: Arc<Shared>,
    current: Arc<WebSession>,
    text: String,
) {
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

pub(in crate::web) async fn run_agent(
    state: Arc<Shared>,
    current: Arc<WebSession>,
    text: String,
) -> Result<()> {
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

pub(in crate::web) async fn generate_title_after_reply(
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
            cfg.protocol_token,
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
