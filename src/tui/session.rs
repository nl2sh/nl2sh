use super::{
    app::{App, PopupView},
    i18n,
    input::Input,
    output::{
        advance_llm_gradient, append_llm_delta, append_transcript, begin_llm_stream,
        discard_llm_stream, finalize_live_output, push_history, snapshot, SessionOutput,
    },
    terminal::{best_effort_restore, TerminalGuard},
    ui,
};
use crate::{
    agent::{can_remember_approval, AgentOutcome, AgentRunner, ConfirmationDecision, Confirmer},
    config::{Config, UiLanguage},
    history::HistoryLog,
    llm::{ConversationItem, LlmClient, TextDeltaSink},
    provider_account::{build_account_client, AccountBalance},
    provider_metadata::{build_metadata_client, ModelMetadata},
    security::{RiskLevel, SecurityAssessment},
    shell::{OutputSink, ShellExecutor},
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
};
use nix::{
    sys::signal::{kill, Signal},
    unistd::getpid,
};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};

const BALANCE_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
type BalanceFuture = Pin<Box<dyn Future<Output = Result<Vec<AccountBalance>>> + Send>>;
type UpdateFuture =
    Pin<Box<dyn Future<Output = Result<Option<crate::update::UpdateRelease>>> + Send>>;
type ModelListFuture = Pin<Box<dyn Future<Output = Result<Vec<ModelMetadata>>> + Send>>;

/// Reason a live Agent TUI session returned to its caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionExit {
    /// The user requested a normal exit.
    Quit,
    /// The user requested a configuration flow.
    Configure(ConfigTarget),
    /// Install a release selected from the startup update prompt.
    Update(crate::update::UpdateRelease),
}

/// Configuration section selected by a local slash command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigTarget {
    /// Provider, model, interface, and execution defaults.
    All,
    /// API endpoint, credential, and wire protocol.
    Provider,
    /// Model identifier only.
    Model,
    /// Fetch models from the configured provider, with manual fallback.
    Models,
    /// Query a documented provider balance endpoint without persisting the result.
    Balance,
    /// Reload proxy settings already saved by the in-TUI editor.
    Proxy,
}

/// Runs the default Agent as a live single-frame TUI session.
pub async fn run_agent_session(
    config: &Config,
    llm: &dyn LlmClient,
    root: String,
    log: HistoryLog,
    provider_configured: bool,
) -> Result<SessionExit> {
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        best_effort_restore();
        eprintln!("{info}");
    }));
    let result = run_inner(config, llm, root, log, provider_configured).await;
    std::panic::set_hook(old_hook);
    result
}

async fn run_inner(
    config: &Config,
    llm: &dyn LlmClient,
    root: String,
    log: HistoryLog,
    provider_configured: bool,
) -> Result<SessionExit> {
    log.record("session_start", "Agent TUI started")?;
    let history = Arc::new(Mutex::new(i18n::startup_history(
        config.ui_language,
        config.ascii_symbols,
    )));
    let output: Arc<dyn OutputSink> = Arc::new(SessionOutput {
        history: history.clone(),
        ascii: config.ascii_symbols,
        log: log.clone(),
        max_bytes: config.ui_live_output_max_bytes,
    });
    let suspended = Arc::new(AtomicBool::new(false));
    let executor = ShellExecutor::new(config.clone())
        .with_output(output)
        .with_tui_active(true)
        .with_tui_suspend_flag(suspended.clone());
    let (confirm_tx, mut confirm_rx) = mpsc::unbounded_channel();
    let confirmer = SessionConfirmer { tx: confirm_tx };
    let runner = AgentRunner {
        config,
        llm,
        executor: &executor,
        confirmer: &confirmer,
    };
    let stream_sink = SessionTextSink {
        history: history.clone(),
        max_bytes: config.ui_live_output_max_bytes,
        needs_full_redraw: Arc::new(AtomicBool::new(false)),
    };
    let stream_redraw = stream_sink.needs_full_redraw.clone();
    let mut terminal = TerminalGuard::enter()?;
    let mut app = App {
        input: Input::default(),
        input_history: Vec::new(),
        input_history_index: None,
        input_history_draft: String::new(),
        cursor_visible: true,
        command_selection: 0,
        history: snapshot(&history)?,
        conversation_scroll: 0,
        tool_results_expanded: false,
        welcome_train_frame: Some(0),
        model: config.model.clone(),
        root,
        ascii: config.ascii_symbols,
        language: config.ui_language,
        api_type: format!("{:?}", config.api_type),
        mode: i18n::mode_agent(config.ui_language).into(),
        turn: 0,
        max_context: config.max_context_turns,
        status: if provider_configured {
            i18n::idle(config.ui_language).into()
        } else {
            localized_status(
                config.ui_language,
                "尚未配置模型服务；使用 /config 或 /setting",
                "provider not configured; use /config or /setting",
            )
        },
        provider_balance: None,
        popup: None,
    };
    let mut model_history: Vec<Vec<ConversationItem>> = Vec::new();
    let mut active: Option<Pin<Box<dyn Future<Output = Result<AgentOutcome>> + '_>>> = None;
    let mut balance_active: Option<BalanceFuture> = None;
    let balance_supported = provider_configured && build_account_client(config).is_ok();
    let mut balance_manual = false;
    let mut last_balance_refresh: Option<Instant> = None;
    let mut confirmation: Option<ConfirmationUi> = None;
    let mut settings_editor: Option<SettingsEditor> = None;
    let update_config = config.clone();
    let mut update_check: Option<UpdateFuture> = Some(Box::pin(async move {
        crate::update::check(&update_config).await
    }));
    let mut update_prompt: Option<UpdatePrompt> = None;
    let mut update_manual = false;
    let mut model_list_active: Option<ModelListFuture> = None;
    let mut quit_after_cancel = false;
    let mut cancel_signal_pending = false;
    let mut was_suspended = false;
    let mut last_cursor_blink = Instant::now();
    let mut last_train_frame = Instant::now();
    let mut last_gradient_frame = Instant::now();
    let mut fragmented_arrow = super::events::FragmentedArrowFilter::default();

    loop {
        if settings_editor.is_some()
            && fragmented_arrow.take_expired_escape(Duration::from_millis(35))
        {
            settings_editor = None;
            app.status = localized_status(config.ui_language, "设置未修改", "settings unchanged");
        }
        if balance_supported
            && active.is_none()
            && balance_active.is_none()
            && last_balance_refresh.is_none_or(|last| last.elapsed() >= BALANCE_REFRESH_INTERVAL)
        {
            let account_config = config.clone();
            balance_active = Some(Box::pin(async move {
                let client = build_account_client(&account_config)?;
                client.balances(&account_config).await
            }));
            last_balance_refresh = Some(Instant::now());
        }
        if last_cursor_blink.elapsed() >= Duration::from_millis(500) {
            app.cursor_visible = !app.cursor_visible;
            last_cursor_blink = Instant::now();
        }
        if last_train_frame.elapsed() >= super::app::WELCOME_TRAIN_FRAME_INTERVAL {
            let viewport_width = terminal.terminal().size()?.width.saturating_sub(2) as usize;
            app.advance_welcome_train(viewport_width);
            last_train_frame = Instant::now();
        }
        if active.is_some() && last_gradient_frame.elapsed() >= Duration::from_millis(70) {
            advance_llm_gradient(&history)?;
            last_gradient_frame = Instant::now();
        }
        let is_suspended = suspended.load(Ordering::Acquire);
        if !is_suspended && stream_redraw.swap(false, Ordering::AcqRel) {
            terminal
                .terminal()
                .clear()
                .context("clear terminal after LLM stream")?;
        }
        if was_suspended && !is_suspended {
            // The interactive child returned to a fresh alternate screen.
            // Invalidate ratatui's retained buffer so unchanged frame regions
            // are repainted instead of remaining blank.
            terminal
                .terminal()
                .clear()
                .context("clear terminal after interactive command")?;
        }
        was_suspended = is_suspended;
        if !is_suspended {
            app.history = snapshot(&history)?;
            app.turn = model_history.len();
            app.popup = confirmation
                .as_ref()
                .map(ConfirmationUi::view)
                .or_else(|| update_prompt.as_ref().map(UpdatePrompt::view))
                .or_else(|| {
                    settings_editor
                        .as_ref()
                        .map(|editor| editor.view(app.cursor_visible))
                });
            terminal.terminal().draw(|frame| ui::draw(frame, &app))?;
        }

        let mut completed = None;
        let mut balance_completed = None;
        if let (Some(balance_future), Some(agent_future)) =
            (balance_active.as_mut(), active.as_mut())
        {
            tokio::select! {
                result = balance_future.as_mut() => balance_completed = Some(result),
                result = agent_future.as_mut() => completed = Some(result),
                request = confirm_rx.recv(), if confirmation.is_none() => {
                    if let Some(request) = request {
                        app.status = localized_status(
                            config.ui_language,
                            "等待安全确认",
                            "waiting for confirmation",
                        );
                        confirmation = Some(ConfirmationUi::new(request, config.ui_language));
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(11)) => {}
            }
        } else if let Some(future) = balance_active.as_mut() {
            tokio::select! {
                result = future.as_mut() => balance_completed = Some(result),
                _ = tokio::time::sleep(Duration::from_millis(11)) => {}
            }
        } else if let Some(future) = active.as_mut() {
            tokio::select! {
                result = future.as_mut() => completed = Some(result),
                request = confirm_rx.recv(), if confirmation.is_none() => {
                    if let Some(request) = request {
                        app.status = localized_status(
                            config.ui_language,
                            "等待安全确认",
                            "waiting for confirmation",
                        );
                        confirmation = Some(ConfirmationUi::new(request, config.ui_language));
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(11)) => {}
            }
        } else {
            tokio::time::sleep(Duration::from_millis(11)).await;
        }

        if let Some(result) = balance_completed {
            balance_active = None;
            match result {
                Ok(balances) if !balances.is_empty() => {
                    app.provider_balance = Some(format_balances(&balances));
                    if balance_manual {
                        app.status =
                            localized_status(config.ui_language, "余额已刷新", "balance refreshed");
                    }
                }
                Ok(_) if balance_manual => {
                    app.status = localized_status(
                        config.ui_language,
                        "Provider 未返回余额",
                        "provider returned no balance",
                    );
                }
                Err(_) if balance_manual => {
                    app.status = localized_status(
                        config.ui_language,
                        "余额刷新失败，保留上次结果",
                        "balance refresh failed; keeping last value",
                    );
                }
                Ok(_) | Err(_) => {}
            }
            balance_manual = false;
        }

        if let Some(check) = update_check.as_mut() {
            if let Ok(result) = tokio::time::timeout(Duration::ZERO, check.as_mut()).await {
                update_check = None;
                match result {
                    Ok(Some(release))
                        if update_manual
                            || config.skipped_update_version.as_deref()
                                != Some(&release.version) =>
                    {
                        update_prompt = Some(UpdatePrompt {
                            release,
                            selected: 0,
                            language: config.ui_language,
                        });
                    }
                    Ok(_) if update_manual => {
                        app.status = localized_status(
                            config.ui_language,
                            "已是最新版本",
                            "already up to date",
                        );
                    }
                    Err(_) if update_manual => {
                        app.status = localized_status(
                            config.ui_language,
                            "更新检查失败",
                            "update check failed",
                        );
                    }
                    Ok(_) | Err(_) => {}
                }
                update_manual = false;
            }
        }

        if let Some(future) = model_list_active.as_mut() {
            if let Ok(result) = tokio::time::timeout(Duration::ZERO, future.as_mut()).await {
                model_list_active = None;
                if let Some(editor) = settings_editor.as_mut() {
                    editor.loading_models = false;
                    match result {
                        Ok(models) if !models.is_empty() => {
                            editor.models = models;
                            editor.model_pick = Some(0);
                            app.status = localized_status(
                                config.ui_language,
                                "请选择在线模型",
                                "select an online model",
                            );
                        }
                        Ok(_) => {
                            editor.model_error = Some(localized_status(
                                config.ui_language,
                                "Provider 未返回模型",
                                "provider returned no models",
                            ));
                        }
                        Err(_) => {
                            editor.model_error = Some(localized_status(
                                config.ui_language,
                                "模型列表拉取失败，可继续手工输入",
                                "model discovery failed; manual input remains available",
                            ));
                        }
                    }
                }
            }
        }

        if let Some(result) = completed {
            active = None;
            confirmation = None;
            match result {
                Ok(outcome) => {
                    append_transcript(&history, &outcome, config.ascii_symbols, &log)?;
                    for _ in 0..outcome.history_turns_evicted.min(model_history.len()) {
                        model_history.remove(0);
                    }
                    model_history.push(outcome.transcript);
                    while model_history.len() > config.max_context_turns {
                        model_history.remove(0);
                    }
                    let context = context_usage(
                        outcome.final_input_tokens,
                        config.effective_context_window(),
                    );
                    app.status = match config.ui_language {
                        UiLanguage::ZhCn => format!(
                            "空闲；上次 {} 步，Token 输入 {} / 输出 {} / 总计 {}，上下文 {}",
                            outcome.steps,
                            usage_value(outcome.usage.input_tokens),
                            usage_value(outcome.usage.output_tokens),
                            usage_value(outcome.usage.total_tokens()),
                            context
                        ),
                        UiLanguage::En => format!(
                            "idle; last {} steps, tokens in {} / out {} / total {}, context {}",
                            outcome.steps,
                            usage_value(outcome.usage.input_tokens),
                            usage_value(outcome.usage.output_tokens),
                            usage_value(outcome.usage.total_tokens()),
                            context
                        ),
                    };
                }
                Err(error) => {
                    discard_llm_stream(&history)?;
                    finalize_live_output(&history)?;
                    push_history(
                        &history,
                        format!(
                            "{} {error:#}",
                            if config.ascii_symbols {
                                "[ERROR]"
                            } else {
                                "❌"
                            }
                        ),
                        &log,
                        "error",
                    )?;
                    app.status =
                        localized_status(config.ui_language, "出错后空闲", "idle after error");
                }
            }
            if quit_after_cancel {
                return Ok(SessionExit::Quit);
            }
        }

        if cancel_signal_pending && active.is_some() && confirmation.is_none() {
            signal_cancel()?;
            cancel_signal_pending = false;
            app.status = localized_status(config.ui_language, "正在取消", "cancelling");
        }

        if is_suspended {
            continue;
        }
        while event::poll(Duration::ZERO)? {
            let event = event::read()?;
            if let Event::Mouse(mouse) = event {
                match mouse.kind {
                    MouseEventKind::ScrollUp => app.scroll_conversation_up(3),
                    MouseEventKind::ScrollDown => app.scroll_conversation_down(3),
                    _ => {}
                }
                continue;
            }
            let Event::Key(key) = event else { continue };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            app.cursor_visible = true;
            last_cursor_blink = Instant::now();
            if key.code == KeyCode::Char('q') && key.modifiers == KeyModifiers::CONTROL {
                if active.is_some() {
                    if let Some(pending) = confirmation.as_mut() {
                        pending.finish(ConfirmationDecision::Reject);
                        confirmation = None;
                        cancel_signal_pending = true;
                    } else {
                        signal_cancel()?;
                    }
                    quit_after_cancel = true;
                    app.status = localized_status(
                        config.ui_language,
                        "退出前正在取消任务",
                        "cancelling before quit",
                    );
                } else {
                    return Ok(SessionExit::Quit);
                }
                continue;
            }
            if active.is_some()
                && key.code == KeyCode::Char('c')
                && key.modifiers == KeyModifiers::CONTROL
            {
                if let Some(pending) = confirmation.as_mut() {
                    pending.finish(ConfirmationDecision::Reject);
                    confirmation = None;
                    cancel_signal_pending = true;
                } else {
                    signal_cancel()?;
                }
                app.status = localized_status(config.ui_language, "正在取消", "cancelling");
                continue;
            }
            if let Some(pending) = confirmation.as_mut() {
                if pending.handle_key(key) {
                    confirmation = None;
                    app.status =
                        localized_status(config.ui_language, "智能体运行中", "agent running");
                }
                continue;
            }
            if let Some(prompt) = update_prompt.as_mut() {
                let Some(key) = fragmented_arrow.normalize(key) else {
                    continue;
                };
                match prompt.handle_key(key) {
                    UpdateAction::Continue => {}
                    UpdateAction::No => {
                        update_prompt = None;
                        app.status =
                            localized_status(config.ui_language, "本次暂不更新", "update deferred");
                    }
                    UpdateAction::Yes => return Ok(SessionExit::Update(prompt.release.clone())),
                    UpdateAction::Skip => {
                        let mut updated = config.clone();
                        updated.skipped_update_version = Some(prompt.release.version.clone());
                        if let Some(path) = updated.source.clone() {
                            crate::config::save_config(&path, &updated)?;
                        }
                        update_prompt = None;
                        app.status = localized_status(
                            config.ui_language,
                            "已跳过此版本",
                            "this version will be skipped",
                        );
                    }
                }
                continue;
            }
            if let Some(editor) = settings_editor.as_mut() {
                let Some(key) = fragmented_arrow.normalize(key) else {
                    continue;
                };
                match editor.handle_key(key) {
                    SettingsAction::Continue => {}
                    SettingsAction::FetchModels => {
                        let metadata_config = editor.config.clone();
                        editor.loading_models = true;
                        editor.model_error = None;
                        model_list_active = Some(Box::pin(async move {
                            build_metadata_client(&metadata_config)
                                .list_models(&metadata_config)
                                .await
                        }));
                        app.status = localized_status(
                            config.ui_language,
                            "正在后台拉取模型列表",
                            "fetching model list in background",
                        );
                    }
                    SettingsAction::Cancel => {
                        settings_editor = None;
                        app.status = localized_status(
                            config.ui_language,
                            "设置未修改",
                            "settings unchanged",
                        );
                    }
                    SettingsAction::Save => {
                        let updated = editor.config.clone();
                        let Some(path) = updated.source.clone() else {
                            app.status = localized_status(
                                config.ui_language,
                                "无法定位配置文件",
                                "cannot locate configuration file",
                            );
                            continue;
                        };
                        match crate::config::save_config(&path, &updated) {
                            Ok(()) => return Ok(SessionExit::Configure(ConfigTarget::Proxy)),
                            Err(_) => {
                                app.status = localized_status(
                                    config.ui_language,
                                    "设置无效或保存失败",
                                    "invalid settings or save failed",
                                );
                            }
                        }
                    }
                }
                continue;
            }
            if active.is_some() {
                continue;
            }
            let Some(key) = fragmented_arrow.normalize(key) else {
                continue;
            };
            app.welcome_train_frame = None;
            match (key.code, key.modifiers) {
                (KeyCode::PageUp, _) => app.scroll_conversation_up(10),
                (KeyCode::PageDown, _) => app.scroll_conversation_down(10),
                (KeyCode::F(2), _) => app.tool_results_expanded = !app.tool_results_expanded,
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => app.input.clear(),
                (KeyCode::Esc, _) => {}
                (KeyCode::Enter, _) => {
                    if app.complete_selected_command() {
                        continue;
                    }
                    let input = app.take_input();
                    match input.trim() {
                        "/exit" => {
                            log.record("local_command", "/exit")?;
                            return Ok(SessionExit::Quit);
                        }
                        "/config" | "/setting" => {
                            log.record("local_command", input.trim())?;
                            settings_editor = Some(SettingsEditor::new(config, 0));
                            app.status = localized_status(
                                config.ui_language,
                                "配置设置",
                                "configuring settings",
                            );
                            continue;
                        }
                        "/balance" => {
                            if !balance_supported {
                                app.status = localized_status(
                                    config.ui_language,
                                    "当前 Provider 不支持余额查询",
                                    "current provider does not support balance lookup",
                                );
                                continue;
                            }
                            let account_config = config.clone();
                            balance_active = Some(Box::pin(async move {
                                let client = build_account_client(&account_config)?;
                                client.balances(&account_config).await
                            }));
                            balance_manual = true;
                            last_balance_refresh = Some(Instant::now());
                            app.status = localized_status(
                                config.ui_language,
                                "正在网络查询 Provider 余额",
                                "fetching provider balance",
                            );
                            continue;
                        }
                        "/update" => {
                            log.record("local_command", "/update")?;
                            let update_config = config.clone();
                            update_check = Some(Box::pin(async move {
                                crate::update::check(&update_config).await
                            }));
                            update_manual = true;
                            app.status = localized_status(
                                config.ui_language,
                                "正在检查更新",
                                "checking for updates",
                            );
                            continue;
                        }
                        "/help" => {
                            log.record("local_command", "/help")?;
                            history
                                .lock()
                                .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
                                .extend(i18n::help_history(
                                    config.ui_language,
                                    config.ascii_symbols,
                                ));
                            app.conversation_scroll = 0;
                            continue;
                        }
                        "/clear" => {
                            log.record("local_command", "/clear")?;
                            history
                                .lock()
                                .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
                                .clear();
                            model_history.clear();
                            app.clear_session_state();
                            app.status = localized_status(
                                config.ui_language,
                                "当前会话历史已清空",
                                "current session history cleared",
                            );
                            continue;
                        }
                        _ => {}
                    }
                    if input.trim_start().starts_with('/') {
                        log.record("unknown_local_command", input.trim())?;
                        history
                            .lock()
                            .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
                            .push(match config.ui_language {
                                UiLanguage::ZhCn => format!(
                                    "⚠️ 未知本地命令：{}。输入 /help 查看可用命令。",
                                    input.trim()
                                ),
                                UiLanguage::En => format!(
                                    "[WARN] Unknown local command: {}. Use /help to list commands.",
                                    input.trim()
                                ),
                            });
                        app.status = localized_status(
                            config.ui_language,
                            "未知本地命令",
                            "unknown local command",
                        );
                        continue;
                    }
                    if !input.trim().is_empty() {
                        if !provider_configured {
                            log.record("local_rejection", "provider is not configured")?;
                            history
                                .lock()
                                .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
                                .push(match config.ui_language {
                            UiLanguage::ZhCn => "⚠️ 尚未配置模型服务，请使用 /config 或 /setting 打开设置面板。".into(),
                            UiLanguage::En => "[WARN] Provider is not configured. Use /config or /setting to open Settings.".into(),
                                });
                            app.status = localized_status(
                                config.ui_language,
                                "等待配置模型服务",
                                "waiting for provider configuration",
                            );
                            continue;
                        }
                        push_history(&history, format!("> {input}"), &log, "user")?;
                        app.status = localized_status(
                            config.ui_language,
                            "正在请求模型 / 执行工具",
                            "requesting LLM / executing tools",
                        );
                        active = Some(Box::pin(runner.run_with_history_streaming_owned(
                            input,
                            model_history.clone(),
                            &stream_sink,
                        )));
                    }
                }
                (KeyCode::Up, _) if app.command_menu_visible() => app.select_previous_command(),
                (KeyCode::Down, _) if app.command_menu_visible() => app.select_next_command(),
                (KeyCode::Up, _) => app.previous_input(),
                (KeyCode::Down, _) => app.next_input(),
                (KeyCode::Left, _) => app.input.move_left(),
                (KeyCode::Right, _) => app.input.move_right(),
                (KeyCode::Home, _) => app.input.move_home(),
                (KeyCode::End, _) => app.input.move_end(),
                (KeyCode::Backspace, _) => app.input.backspace(),
                (KeyCode::Delete, _) => app.input.delete(),
                (KeyCode::Char(character), _) => app.input.push(character),
                _ => {}
            }
        }
    }
}

fn usage_value(value: Option<u64>) -> String {
    value.map_or_else(|| "?".into(), |value| value.to_string())
}

fn context_usage(input: Option<u64>, window: Option<u64>) -> String {
    match (input, window) {
        (Some(input), Some(window)) if window > 0 => {
            format!("{:.1}%", input as f64 * 100.0 / window as f64)
        }
        _ => "?".into(),
    }
}

fn format_balances(balances: &[AccountBalance]) -> String {
    balances
        .iter()
        .map(|balance| format!("{} {}", balance.currency, balance.amount))
        .collect::<Vec<_>>()
        .join(" / ")
}

const SETTINGS_TABS_ZH: [&str; 5] = ["服务", "模型与智能体", "执行与安全", "界面", "网络"];
const SETTINGS_TABS_EN: [&str; 5] = [
    "Provider",
    "Model & Agent",
    "Execution",
    "Interface",
    "Network",
];

struct UpdatePrompt {
    release: crate::update::UpdateRelease,
    selected: usize,
    language: UiLanguage,
}

enum UpdateAction {
    Continue,
    Yes,
    No,
    Skip,
}

impl UpdatePrompt {
    fn view(&self) -> PopupView {
        let choices = match self.language {
            UiLanguage::ZhCn => ["Yes  立即更新", "No   暂不更新", "跳过本次版本"],
            UiLanguage::En => ["Yes  Update now", "No   Not now", "Skip this version"],
        };
        let mut lines = vec![
            match self.language {
                UiLanguage::ZhCn => format!(
                    "发现新版本 v{}（当前 v{}）",
                    self.release.version,
                    env!("CARGO_PKG_VERSION")
                ),
                UiLanguage::En => format!(
                    "Version v{} is available (current v{})",
                    self.release.version,
                    env!("CARGO_PKG_VERSION")
                ),
            },
            String::new(),
        ];
        lines.extend(choices.into_iter().enumerate().map(|(index, choice)| {
            format!(
                "{} {choice}",
                if index == self.selected { ">" } else { " " }
            )
        }));
        lines.push(String::new());
        lines.push(
            match self.language {
                UiLanguage::ZhCn => "↑/↓ 选择，Enter 确认",
                UiLanguage::En => "Up/Down select, Enter confirms",
            }
            .into(),
        );
        PopupView {
            title: match self.language {
                UiLanguage::ZhCn => "版本更新",
                UiLanguage::En => "Update available",
            }
            .into(),
            lines,
            dangerous: false,
            informational: true,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> UpdateAction {
        match key.code {
            KeyCode::Up => {
                self.selected = self.selected.checked_sub(1).unwrap_or(2);
                UpdateAction::Continue
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1) % 3;
                UpdateAction::Continue
            }
            KeyCode::Char('y' | 'Y') => UpdateAction::Yes,
            KeyCode::Char('n' | 'N') | KeyCode::Esc => UpdateAction::No,
            KeyCode::Enter => match self.selected {
                0 => UpdateAction::Yes,
                1 => UpdateAction::No,
                _ => UpdateAction::Skip,
            },
            _ => UpdateAction::Continue,
        }
    }
}

struct SettingsEditor {
    config: Config,
    tab: usize,
    selected: usize,
    text_cursor: usize,
    models: Vec<ModelMetadata>,
    model_pick: Option<usize>,
    loading_models: bool,
    model_error: Option<String>,
}

enum SettingsAction {
    Continue,
    FetchModels,
    Cancel,
    Save,
}

impl SettingsEditor {
    fn new(config: &Config, tab: usize) -> Self {
        let mut editor = Self {
            config: config.clone(),
            tab: tab.min(4),
            selected: 0,
            text_cursor: 0,
            models: Vec::new(),
            model_pick: None,
            loading_models: false,
            model_error: None,
        };
        editor.sync_text_cursor();
        editor
    }

    fn field_count(&self) -> usize {
        [3, 6, 6, 4, 6][self.tab]
    }

    fn view(&self, cursor_visible: bool) -> PopupView {
        if let Some(selected) = self.model_pick {
            let start = selected
                .saturating_sub(4)
                .min(self.models.len().saturating_sub(8));
            let mut lines = self
                .models
                .iter()
                .enumerate()
                .skip(start)
                .take(8)
                .map(|(index, model)| {
                    let marker = if index == selected { ">" } else { " " };
                    let context = model
                        .context_window
                        .map_or_else(|| "?".into(), |value| value.to_string());
                    format!("{marker} {}  context={context}", model.id)
                })
                .collect::<Vec<_>>();
            lines.push(format!("{}/{}", selected + 1, self.models.len()));
            lines.push(String::new());
            lines.push(match self.config.ui_language {
                UiLanguage::ZhCn => "↑/↓ 选择；Enter 回填；Esc 返回设置".into(),
                UiLanguage::En => "Up/Down select; Enter applies; Esc returns to settings".into(),
            });
            return PopupView {
                title: match self.config.ui_language {
                    UiLanguage::ZhCn => "在线模型列表",
                    UiLanguage::En => "Online models",
                }
                .into(),
                lines,
                dangerous: false,
                informational: true,
            };
        }
        let tabs = match self.config.ui_language {
            UiLanguage::ZhCn => SETTINGS_TABS_ZH,
            UiLanguage::En => SETTINGS_TABS_EN,
        };
        let tab_line = tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                if index == self.tab {
                    format!("[{tab}]")
                } else {
                    tab.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("  ");
        let mut lines = vec![tab_line, String::new()];
        for (index, (label, value)) in self.fields().into_iter().enumerate() {
            let marker = if index == self.selected { ">" } else { " " };
            if index == self.selected && self.is_text_field() {
                let value = self.input_with_cursor(&value, cursor_visible);
                lines.push(format!("{marker} {label}: ┌ {value} ┐"));
            } else {
                lines.push(format!("{marker} {label}: {value}"));
            }
        }
        if let Some(error) = &self.model_error {
            lines.push(format!("⚠ {error}"));
        }
        lines.push(String::new());
        lines.push(match self.config.ui_language {
            UiLanguage::ZhCn => "Tab/Shift+Tab 分类；↑/↓ 字段；←/→ 调整；输入编辑；Ctrl+S 保存；Esc 取消".into(),
            UiLanguage::En => "Tab/Shift+Tab category; Up/Down field; Left/Right adjust; type to edit; Ctrl+S save; Esc cancel".into(),
        });
        PopupView {
            title: match self.config.ui_language {
                UiLanguage::ZhCn => "设置",
                UiLanguage::En => "Settings",
            }
            .into(),
            lines,
            dangerous: false,
            informational: true,
        }
    }

    fn fields(&self) -> Vec<(&'static str, String)> {
        let zh = self.config.ui_language == UiLanguage::ZhCn;
        match self.tab {
            0 => vec![
                (
                    if zh { "API 地址" } else { "API endpoint" },
                    self.config.endpoint.clone(),
                ),
                (
                    "API Key",
                    if self.config.api_key.is_empty() {
                        String::new()
                    } else {
                        "*".repeat(self.config.api_key.chars().count())
                    },
                ),
                (
                    if zh { "协议" } else { "Protocol" },
                    format!("{:?}", self.config.api_type),
                ),
            ],
            1 => vec![
                (if zh { "模型" } else { "Model" }, self.config.model.clone()),
                (
                    if zh {
                        "上下文窗口"
                    } else {
                        "Context window"
                    },
                    optional_number(self.config.model_context_window),
                ),
                (
                    if zh {
                        "最大输出 Token"
                    } else {
                        "Max output tokens"
                    },
                    optional_number(self.config.model_max_output_tokens),
                ),
                (
                    if zh {
                        "最大步骤（推荐 24）"
                    } else {
                        "Max steps (recommended 24)"
                    },
                    self.config.max_agent_steps.to_string(),
                ),
                (
                    if zh {
                        "最大轮次（推荐 16）"
                    } else {
                        "Max turns (recommended 16)"
                    },
                    self.config.max_context_turns.to_string(),
                ),
                (
                    if zh {
                        "在线模型列表"
                    } else {
                        "Online model list"
                    },
                    if self.loading_models {
                        if zh {
                            "正在拉取…"
                        } else {
                            "fetching…"
                        }
                    } else if zh {
                        "Enter 拉取"
                    } else {
                        "Enter to fetch"
                    }
                    .into(),
                ),
            ],
            2 => vec![
                (
                    if zh { "确认策略" } else { "Confirmation" },
                    format!("{:?}", self.config.execute_confirm_policy),
                ),
                (
                    if zh { "安全级别" } else { "Security" },
                    format!("{:?}", self.config.security_level),
                ),
                (
                    if zh { "执行用户" } else { "Execution user" },
                    format!("{:?}", self.config.execute_user_mode),
                ),
                (
                    if zh {
                        "命令超时秒"
                    } else {
                        "Command timeout sec"
                    },
                    self.config.execute_timeout_secs.to_string(),
                ),
                (
                    if zh {
                        "交互超时秒（0=关闭）"
                    } else {
                        "Interactive timeout (0=off)"
                    },
                    self.config.interactive_execute_timeout_secs.to_string(),
                ),
                ("PTY", self.config.enable_pty.to_string()),
            ],
            3 => vec![
                (
                    if zh { "语言" } else { "Language" },
                    format!("{:?}", self.config.ui_language),
                ),
                (
                    if zh { "ASCII 符号" } else { "ASCII symbols" },
                    self.config.ascii_symbols.to_string(),
                ),
                (
                    if zh {
                        "实时输出上限 bytes"
                    } else {
                        "Live output bytes"
                    },
                    self.config.ui_live_output_max_bytes.to_string(),
                ),
                (
                    if zh {
                        "工具输出上限 bytes"
                    } else {
                        "Tool output bytes"
                    },
                    self.config.tool_output_max_bytes.to_string(),
                ),
            ],
            _ => vec![
                (
                    if zh { "代理开关" } else { "Proxy enabled" },
                    self.config.proxy_enabled.to_string(),
                ),
                (
                    if zh { "代理类型" } else { "Proxy type" },
                    format!("{:?}", self.config.proxy_type),
                ),
                (
                    if zh {
                        "地址 host:port"
                    } else {
                        "Address host:port"
                    },
                    self.config.proxy_address.clone(),
                ),
                (
                    if zh { "用户名" } else { "Username" },
                    self.config.proxy_username.clone(),
                ),
                (
                    if zh { "密码" } else { "Password" },
                    if self.config.proxy_password.is_empty() {
                        String::new()
                    } else {
                        "*".repeat(self.config.proxy_password.chars().count())
                    },
                ),
                (
                    if zh { "绕过列表" } else { "Bypass list" },
                    self.config.proxy_bypass.clone(),
                ),
            ],
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> SettingsAction {
        if let Some(selected) = self.model_pick.as_mut() {
            match key.code {
                KeyCode::Esc => self.model_pick = None,
                KeyCode::Up => {
                    *selected = selected.checked_sub(1).unwrap_or(self.models.len() - 1);
                }
                KeyCode::Down => *selected = (*selected + 1) % self.models.len(),
                KeyCode::Enter => {
                    if let Some(model) = self.models.get(*selected) {
                        self.config.model = model.id.clone();
                        if model.context_window.is_some() {
                            self.config.model_context_window = model.context_window;
                        }
                        if model.max_output_tokens.is_some() {
                            self.config.model_max_output_tokens = model.max_output_tokens;
                        }
                    }
                    self.model_pick = None;
                    self.selected = 0;
                    self.sync_text_cursor();
                }
                _ => {}
            }
            return SettingsAction::Continue;
        }
        match key.code {
            KeyCode::Esc => SettingsAction::Cancel,
            KeyCode::Tab => {
                self.tab = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.tab.checked_sub(1).unwrap_or(4)
                } else {
                    (self.tab + 1) % 5
                };
                self.selected = 0;
                self.sync_text_cursor();
                SettingsAction::Continue
            }
            KeyCode::BackTab => {
                self.tab = self.tab.checked_sub(1).unwrap_or(4);
                self.selected = 0;
                self.sync_text_cursor();
                SettingsAction::Continue
            }
            KeyCode::Up => {
                self.selected = self
                    .selected
                    .checked_sub(1)
                    .unwrap_or(self.field_count() - 1);
                self.sync_text_cursor();
                SettingsAction::Continue
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1) % self.field_count();
                self.sync_text_cursor();
                SettingsAction::Continue
            }
            KeyCode::Left => {
                if self.is_text_field() {
                    self.move_text_cursor_left();
                } else {
                    self.adjust(-1);
                }
                SettingsAction::Continue
            }
            KeyCode::Right if self.is_text_field() => {
                self.move_text_cursor_right();
                SettingsAction::Continue
            }
            KeyCode::Right | KeyCode::Char(' ') => {
                self.adjust(1);
                SettingsAction::Continue
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                SettingsAction::Save
            }
            KeyCode::Enter if self.tab == 1 && self.selected == 5 && !self.loading_models => {
                SettingsAction::FetchModels
            }
            KeyCode::Backspace if self.is_text_field() => {
                if let Some((previous, _)) = self.selected_text()[..self.text_cursor]
                    .char_indices()
                    .next_back()
                {
                    let cursor = self.text_cursor;
                    self.text_mut().drain(previous..cursor);
                    self.text_cursor = previous;
                }
                SettingsAction::Continue
            }
            KeyCode::Delete if self.is_text_field() => {
                if let Some(character) = self.selected_text()[self.text_cursor..].chars().next() {
                    let cursor = self.text_cursor;
                    self.text_mut().drain(cursor..cursor + character.len_utf8());
                }
                SettingsAction::Continue
            }
            KeyCode::Home if self.is_text_field() => {
                self.text_cursor = 0;
                SettingsAction::Continue
            }
            KeyCode::End if self.is_text_field() => {
                self.text_cursor = self.selected_text().len();
                SettingsAction::Continue
            }
            KeyCode::Char(character)
                if self.is_text_field() && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                let cursor = self.text_cursor;
                let text = self.text_mut();
                text.insert(cursor, character);
                let stripped = matches!(character, 'M' | 'm')
                    && super::input::strip_trailing_sgr_mouse_report(text);
                self.text_cursor = if stripped {
                    text.len()
                } else {
                    cursor + character.len_utf8()
                };
                SettingsAction::Continue
            }
            _ => SettingsAction::Continue,
        }
    }

    fn is_text_field(&self) -> bool {
        matches!((self.tab, self.selected), (0, 0 | 1) | (1, 0) | (4, 2..=5))
    }
    fn text_mut(&mut self) -> &mut String {
        match (self.tab, self.selected) {
            (0, 0) => &mut self.config.endpoint,
            (0, 1) => &mut self.config.api_key,
            (1, 0) => &mut self.config.model,
            (4, 2) => &mut self.config.proxy_address,
            (4, 3) => &mut self.config.proxy_username,
            (4, 4) => &mut self.config.proxy_password,
            _ => &mut self.config.proxy_bypass,
        }
    }

    fn selected_text(&self) -> &str {
        match (self.tab, self.selected) {
            (0, 0) => &self.config.endpoint,
            (0, 1) => &self.config.api_key,
            (1, 0) => &self.config.model,
            (4, 2) => &self.config.proxy_address,
            (4, 3) => &self.config.proxy_username,
            (4, 4) => &self.config.proxy_password,
            _ => &self.config.proxy_bypass,
        }
    }

    fn sync_text_cursor(&mut self) {
        self.text_cursor = if self.is_text_field() {
            self.selected_text().len()
        } else {
            0
        };
    }

    fn move_text_cursor_left(&mut self) {
        if let Some((previous, _)) = self.selected_text()[..self.text_cursor]
            .char_indices()
            .next_back()
        {
            self.text_cursor = previous;
        }
    }

    fn move_text_cursor_right(&mut self) {
        if let Some(character) = self.selected_text()[self.text_cursor..].chars().next() {
            self.text_cursor += character.len_utf8();
        }
    }

    fn input_with_cursor(&self, displayed: &str, cursor_visible: bool) -> String {
        let selected = self.selected_text();
        let mut cursor = self.text_cursor.min(selected.len());
        while !selected.is_char_boundary(cursor) {
            cursor = cursor.saturating_sub(1);
        }
        let character_index = selected[..cursor].chars().count();
        let mut characters = displayed.chars().collect::<Vec<_>>();
        characters.insert(
            character_index.min(characters.len()),
            if cursor_visible { '│' } else { ' ' },
        );
        characters.into_iter().collect()
    }

    fn adjust(&mut self, direction: i8) {
        use crate::config::{ApiType, ConfirmPolicy, ExecuteUserMode, ProxyType, SecurityLevel};
        match (self.tab, self.selected) {
            (0, 2) => {
                self.config.api_type = if self.config.api_type == ApiType::Responses {
                    ApiType::ChatCompletions
                } else {
                    ApiType::Responses
                }
            }
            (1, 1) => adjust_optional(&mut self.config.model_context_window, direction, 1024),
            (1, 2) => adjust_optional(&mut self.config.model_max_output_tokens, direction, 1024),
            (1, 3) => adjust_usize(&mut self.config.max_agent_steps, direction),
            (1, 4) => adjust_usize(&mut self.config.max_context_turns, direction),
            (2, 0) => {
                self.config.execute_confirm_policy = cycle3(
                    self.config.execute_confirm_policy,
                    [
                        ConfirmPolicy::Always,
                        ConfirmPolicy::RiskOnly,
                        ConfirmPolicy::Never,
                    ],
                    direction,
                )
            }
            (2, 1) => {
                self.config.security_level = cycle3(
                    self.config.security_level,
                    [
                        SecurityLevel::Strict,
                        SecurityLevel::Balanced,
                        SecurityLevel::Unsafe,
                    ],
                    direction,
                )
            }
            (2, 2) => {
                self.config.execute_user_mode = cycle3(
                    self.config.execute_user_mode,
                    [
                        ExecuteUserMode::Auto,
                        ExecuteUserMode::Normal,
                        ExecuteUserMode::Root,
                    ],
                    direction,
                )
            }
            (2, 3) => adjust_u64(&mut self.config.execute_timeout_secs, direction, 1),
            (2, 4) => adjust_u64_allow_zero(
                &mut self.config.interactive_execute_timeout_secs,
                direction,
                1,
            ),
            (2, 5) => self.config.enable_pty = !self.config.enable_pty,
            (3, 0) => {
                self.config.ui_language = if self.config.ui_language == UiLanguage::ZhCn {
                    UiLanguage::En
                } else {
                    UiLanguage::ZhCn
                }
            }
            (3, 1) => self.config.ascii_symbols = !self.config.ascii_symbols,
            (3, 2) => adjust_usize_step(
                &mut self.config.ui_live_output_max_bytes,
                direction,
                1024,
                256,
            ),
            (3, 3) => {
                adjust_usize_step(&mut self.config.tool_output_max_bytes, direction, 1024, 256)
            }
            (4, 0) => self.config.proxy_enabled = !self.config.proxy_enabled,
            (4, 1) => {
                self.config.proxy_type = cycle3(
                    self.config.proxy_type,
                    [ProxyType::Http, ProxyType::Socks5, ProxyType::Socks5h],
                    direction,
                )
            }
            _ => {}
        }
    }
}

fn optional_number(value: Option<u64>) -> String {
    value.map_or_else(|| "auto".into(), |value| value.to_string())
}
fn adjust_optional(value: &mut Option<u64>, direction: i8, step: u64) {
    *value = if direction > 0 {
        Some(value.unwrap_or(0).saturating_add(step).max(step))
    } else {
        value.and_then(|v| (v > step).then_some(v - step))
    };
}
fn adjust_usize(value: &mut usize, direction: i8) {
    *value = if direction > 0 {
        value.saturating_add(1)
    } else {
        value.saturating_sub(1).max(1)
    };
}
fn adjust_u64(value: &mut u64, direction: i8, step: u64) {
    *value = if direction > 0 {
        value.saturating_add(step)
    } else {
        value.saturating_sub(step).max(1)
    };
}
fn adjust_u64_allow_zero(value: &mut u64, direction: i8, step: u64) {
    *value = if direction > 0 {
        value.saturating_add(step)
    } else {
        value.saturating_sub(step)
    };
}
fn adjust_usize_step(value: &mut usize, direction: i8, step: usize, minimum: usize) {
    *value = if direction > 0 {
        value.saturating_add(step)
    } else {
        value.saturating_sub(step).max(minimum)
    };
}
fn cycle3<T: Copy + PartialEq>(value: T, values: [T; 3], direction: i8) -> T {
    let index = values.iter().position(|item| *item == value).unwrap_or(0);
    values[if direction > 0 {
        (index + 1) % 3
    } else {
        index.checked_sub(1).unwrap_or(2)
    }]
}

struct SessionTextSink {
    history: Arc<Mutex<Vec<String>>>,
    max_bytes: usize,
    needs_full_redraw: Arc<AtomicBool>,
}

impl TextDeltaSink for SessionTextSink {
    fn begin(&self) {
        let _ = begin_llm_stream(&self.history);
    }

    fn delta(&self, text: &str) {
        let _ = append_llm_delta(&self.history, text, self.max_bytes);
    }

    fn end(&self, completed: bool) {
        if !completed {
            let _ = discard_llm_stream(&self.history);
        }
        self.needs_full_redraw.store(true, Ordering::Release);
    }
}

fn signal_cancel() -> Result<()> {
    kill(getpid(), Signal::SIGINT).context("failed to deliver cancellation signal")?;
    Ok(())
}

fn localized_status(language: UiLanguage, zh_cn: &str, en: &str) -> String {
    match language {
        UiLanguage::ZhCn => zh_cn,
        UiLanguage::En => en,
    }
    .into()
}

struct ConfirmRequest {
    command: String,
    assessment: SecurityAssessment,
    reply: oneshot::Sender<ConfirmationDecision>,
}

struct SessionConfirmer {
    tx: mpsc::UnboundedSender<ConfirmRequest>,
}

#[async_trait]
impl Confirmer for SessionConfirmer {
    async fn confirm(
        &self,
        command: &str,
        assessment: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        let (reply, response) = oneshot::channel();
        self.tx
            .send(ConfirmRequest {
                command: command.into(),
                assessment: assessment.clone(),
                reply,
            })
            .map_err(|_| anyhow::anyhow!("TUI confirmation channel closed"))?;
        response.await.context("TUI confirmation was cancelled")
    }
}

enum ConfirmationStage {
    Initial,
    Double,
    Edit,
}

struct ConfirmationUi {
    request: Option<ConfirmRequest>,
    stage: ConfirmationStage,
    text: String,
    approval: ConfirmationDecision,
    selection: usize,
    language: UiLanguage,
}

#[derive(Clone, Copy)]
enum InitialAction {
    AllowOnce,
    AllowForTask,
    Reject,
    Edit,
    Interactive,
    Captured,
}

const INITIAL_ACTIONS: [InitialAction; 6] = [
    InitialAction::AllowOnce,
    InitialAction::AllowForTask,
    InitialAction::Reject,
    InitialAction::Edit,
    InitialAction::Interactive,
    InitialAction::Captured,
];

impl ConfirmationUi {
    fn new(request: ConfirmRequest, language: UiLanguage) -> Self {
        Self {
            request: Some(request),
            stage: ConfirmationStage::Initial,
            text: String::new(),
            approval: ConfirmationDecision::Approve,
            selection: 0,
            language,
        }
    }

    fn view(&self) -> PopupView {
        let Some(request) = &self.request else {
            return match self.language {
                UiLanguage::ZhCn => PopupView {
                    title: "安全确认".into(),
                    lines: vec!["正在关闭…".into()],
                    dangerous: false,
                    informational: false,
                },
                UiLanguage::En => PopupView {
                    title: "Confirmation".into(),
                    lines: vec!["Closing…".into()],
                    dangerous: false,
                    informational: false,
                },
            };
        };
        let mut lines = match self.language {
            UiLanguage::ZhCn => vec![
                format!("命令：{}", request.command),
                format!(
                    "风险：{}{}",
                    i18n::risk(self.language, request.assessment.risk_level),
                    if request.assessment.requires_root {
                        " | ROOT"
                    } else {
                        ""
                    }
                ),
                "命令已由本地安全规则重新分类。".into(),
            ],
            UiLanguage::En => vec![
                format!("Command: {}", request.command),
                format!(
                    "Risk: {}{}",
                    i18n::risk(self.language, request.assessment.risk_level),
                    if request.assessment.requires_root {
                        " | ROOT"
                    } else {
                        ""
                    }
                ),
                request.assessment.explanation.clone(),
            ],
        };
        match self.stage {
            ConfirmationStage::Initial => lines.extend(self.initial_option_lines(request)),
            ConfirmationStage::Double => {
                lines.push(match self.language {
                    UiLanguage::ZhCn => "高风险操作：输入 YES 后按 Enter：".into(),
                    UiLanguage::En => "High risk: type YES, then Enter:".into(),
                });
                lines.push(self.text.clone());
            }
            ConfirmationStage::Edit => {
                lines.push(match self.language {
                    UiLanguage::ZhCn => "编辑命令后按 Enter（执行前会重新分类）：".into(),
                    UiLanguage::En => {
                        "Edit command, then Enter (reclassified before execution):".into()
                    }
                });
                lines.push(self.text.clone());
            }
        }
        PopupView {
            title: match self.language {
                UiLanguage::ZhCn => "安全确认",
                UiLanguage::En => "Security confirmation",
            }
            .into(),
            lines,
            dangerous: matches!(
                request.assessment.risk_level,
                RiskLevel::Dangerous | RiskLevel::Critical
            ),
            informational: false,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match self.stage {
            ConfirmationStage::Initial => match key.code {
                KeyCode::Up => {
                    self.move_selection(-1);
                    false
                }
                KeyCode::Down => {
                    self.move_selection(1);
                    false
                }
                KeyCode::Enter => self.choose_initial(INITIAL_ACTIONS[self.selection]),
                KeyCode::Char('1' | 'y') => self.choose_initial(InitialAction::AllowOnce),
                KeyCode::Char('2' | 'a') => self.choose_initial(InitialAction::AllowForTask),
                KeyCode::Char('3' | 'n') => self.choose_initial(InitialAction::Reject),
                KeyCode::Char('4' | 'e') => self.choose_initial(InitialAction::Edit),
                KeyCode::Char('5' | 'i') => self.choose_initial(InitialAction::Interactive),
                KeyCode::Char('6' | 't') => self.choose_initial(InitialAction::Captured),
                _ => false,
            },
            ConfirmationStage::Double => match key.code {
                KeyCode::Enter => {
                    let decision = if self.text == "YES" {
                        self.approval.clone()
                    } else {
                        ConfirmationDecision::Reject
                    };
                    self.finish(decision)
                }
                KeyCode::Esc => self.finish(ConfirmationDecision::Reject),
                KeyCode::Backspace => {
                    self.text.pop();
                    false
                }
                KeyCode::Char(character) => {
                    self.text.push(character);
                    false
                }
                _ => false,
            },
            ConfirmationStage::Edit => match key.code {
                KeyCode::Enter => {
                    let edited = self.text.trim().to_owned();
                    if edited.is_empty() {
                        self.finish(ConfirmationDecision::Reject)
                    } else {
                        self.finish(ConfirmationDecision::Edit(edited))
                    }
                }
                KeyCode::Esc => self.finish(ConfirmationDecision::Reject),
                KeyCode::Backspace => {
                    self.text.pop();
                    false
                }
                KeyCode::Char(character) => {
                    self.text.push(character);
                    false
                }
                _ => false,
            },
        }
    }

    fn initial_option_lines(&self, request: &ConfirmRequest) -> Vec<String> {
        INITIAL_ACTIONS
            .iter()
            .enumerate()
            .map(|(index, action)| {
                let selected = if index == self.selection { ">" } else { " " };
                let disabled = matches!(action, InitialAction::AllowForTask)
                    && !can_remember_approval(&request.assessment);
                let label = match (self.language, action, disabled) {
                    (UiLanguage::ZhCn, InitialAction::AllowOnce, _) => "仅允许本次 [y]",
                    (UiLanguage::ZhCn, InitialAction::AllowForTask, false) => {
                        "本次任务总是允许完全相同的命令 [a]"
                    }
                    (UiLanguage::ZhCn, InitialAction::AllowForTask, true) => {
                        "总是允许（Root 或高风险命令不可用）"
                    }
                    (UiLanguage::ZhCn, InitialAction::Reject, _) => "拒绝 [n]",
                    (UiLanguage::ZhCn, InitialAction::Edit, _) => "编辑并重新分类 [e]",
                    (UiLanguage::ZhCn, InitialAction::Interactive, _) => "交互终端执行 [i]",
                    (UiLanguage::ZhCn, InitialAction::Captured, _) => "捕获输出执行 [t]",
                    (UiLanguage::En, InitialAction::AllowOnce, _) => "Allow once [y]",
                    (UiLanguage::En, InitialAction::AllowForTask, false) => {
                        "Always allow the exact command for this task [a]"
                    }
                    (UiLanguage::En, InitialAction::AllowForTask, true) => {
                        "Always allow (unavailable for root or high risk)"
                    }
                    (UiLanguage::En, InitialAction::Reject, _) => "Reject [n]",
                    (UiLanguage::En, InitialAction::Edit, _) => "Edit and reassess [e]",
                    (UiLanguage::En, InitialAction::Interactive, _) => {
                        "Run in interactive terminal [i]"
                    }
                    (UiLanguage::En, InitialAction::Captured, _) => "Run with captured output [t]",
                };
                format!("{selected} {}. {label}", index + 1)
            })
            .collect()
    }

    fn move_selection(&mut self, direction: isize) {
        for _ in 0..INITIAL_ACTIONS.len() {
            self.selection = if direction < 0 {
                self.selection
                    .checked_sub(1)
                    .unwrap_or(INITIAL_ACTIONS.len() - 1)
            } else {
                (self.selection + 1) % INITIAL_ACTIONS.len()
            };
            if self.selection != 1 || self.can_remember_current() {
                break;
            }
        }
    }

    fn can_remember_current(&self) -> bool {
        self.request
            .as_ref()
            .is_some_and(|request| can_remember_approval(&request.assessment))
    }

    fn choose_initial(&mut self, action: InitialAction) -> bool {
        match action {
            InitialAction::Reject => self.finish(ConfirmationDecision::Reject),
            InitialAction::Edit => {
                self.text = self
                    .request
                    .as_ref()
                    .map(|request| request.command.clone())
                    .unwrap_or_default();
                self.stage = ConfirmationStage::Edit;
                false
            }
            InitialAction::AllowForTask if !self.can_remember_current() => false,
            InitialAction::AllowOnce
            | InitialAction::AllowForTask
            | InitialAction::Interactive
            | InitialAction::Captured => {
                self.approval = match action {
                    InitialAction::AllowOnce => ConfirmationDecision::Approve,
                    InitialAction::AllowForTask => ConfirmationDecision::ApproveForTask,
                    InitialAction::Interactive => ConfirmationDecision::ApproveInteractive,
                    InitialAction::Captured => ConfirmationDecision::ApproveCaptured,
                    InitialAction::Reject | InitialAction::Edit => ConfirmationDecision::Reject,
                };
                if self
                    .request
                    .as_ref()
                    .is_some_and(|request| request.assessment.requires_double_confirmation)
                {
                    self.stage = ConfirmationStage::Double;
                    false
                } else {
                    self.finish(self.approval.clone())
                }
            }
        }
    }

    fn finish(&mut self, decision: ConfirmationDecision) -> bool {
        if let Some(request) = self.request.take() {
            let _ = request.reply.send(decision);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balance_format_contains_only_display_values() {
        let text = format_balances(&[AccountBalance {
            currency: "CNY".into(),
            amount: "12.34".into(),
        }]);
        assert_eq!(text, "CNY 12.34");
    }

    #[test]
    fn settings_editor_masks_password_and_preserves_fields_when_disabled() {
        let config = Config {
            proxy_enabled: true,
            proxy_address: "proxy.example:1080".into(),
            proxy_username: "user".into(),
            proxy_password: "secret".into(),
            ..Config::default()
        };
        let mut editor = SettingsEditor::new(&config, 4);
        editor.config.proxy_enabled = false;
        editor.selected = 4;
        let view = editor.view(true);
        assert!(view.lines.iter().any(|line| line.contains("******")));
        assert!(!view.lines.iter().any(|line| line.contains("secret")));
        assert!(view
            .lines
            .iter()
            .any(|line| line.contains("┌") && line.contains('│') && line.contains("┐")));
        assert!(!editor.config.proxy_enabled);
        assert_eq!(editor.config.proxy_address, "proxy.example:1080");
        assert_eq!(editor.config.proxy_password, "secret");
    }

    #[test]
    fn settings_tabs_and_arrows_have_separate_navigation_roles() {
        let mut editor = SettingsEditor::new(&Config::default(), 1);
        editor.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(editor.tab, 1);
        assert_eq!(editor.selected, 1);
        editor.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(editor.tab, 2);
        assert_eq!(editor.selected, 0);
    }

    #[test]
    fn settings_text_fields_strip_degraded_sgr_mouse_reports() {
        let config = Config::default();
        let original = config.endpoint.clone();
        let mut editor = SettingsEditor::new(&config, 0);
        for character in "<35;46;8M".chars() {
            editor.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(editor.config.endpoint, original);
    }

    #[test]
    fn settings_model_tab_fetches_and_applies_discovered_metadata() {
        let mut editor = SettingsEditor::new(&Config::default(), 1);
        editor.selected = 5;
        assert!(matches!(
            editor.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            SettingsAction::FetchModels
        ));
        editor.models = vec![ModelMetadata {
            id: "online-model".into(),
            context_window: Some(65_536),
            max_output_tokens: Some(4096),
        }];
        editor.model_pick = Some(0);
        editor.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(editor.config.model, "online-model");
        assert_eq!(editor.config.model_context_window, Some(65_536));
        assert_eq!(editor.config.model_max_output_tokens, Some(4096));
    }

    #[test]
    fn settings_text_cursor_supports_navigation_insertion_and_deletion() {
        let config = Config {
            endpoint: "你a好".into(),
            ..Config::default()
        };
        let mut editor = SettingsEditor::new(&config, 0);
        editor.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        editor.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        editor.handle_key(KeyEvent::new(KeyCode::Char('中'), KeyModifiers::NONE));
        assert_eq!(editor.config.endpoint, "你中a好");
        editor.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(editor.config.endpoint, "你中好");
        editor.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(editor.config.endpoint, "你好");
    }
    use crate::{config::Config, security::assess};

    fn request(command: &str) -> (ConfirmationUi, oneshot::Receiver<ConfirmationDecision>) {
        let (reply, response) = oneshot::channel();
        let assessment = assess(command, &Config::default());
        (
            ConfirmationUi::new(
                ConfirmRequest {
                    command: command.into(),
                    assessment,
                    reply,
                },
                UiLanguage::ZhCn,
            ),
            response,
        )
    }

    #[test]
    fn dangerous_confirmation_requires_exact_yes() {
        let (mut ui, mut response) = request("rm -rf /");
        assert!(!ui.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)));
        for character in ['Y', 'E', 'S'] {
            assert!(!ui.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE,)));
        }
        assert!(ui.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        assert_eq!(response.try_recv(), Ok(ConfirmationDecision::Approve));
    }

    #[test]
    fn edited_command_is_returned_for_agent_reclassification() {
        let (mut ui, mut response) = request("touch /tmp/old");
        assert!(!ui.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)));
        ui.text = "rm -rf /".into();
        assert!(ui.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        assert_eq!(
            response.try_recv(),
            Ok(ConfirmationDecision::Edit("rm -rf /".into()))
        );
    }

    #[test]
    fn user_can_force_interactive_execution() {
        let (mut ui, mut response) = request("touch /tmp/file");
        assert!(ui.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE,)));
        assert_eq!(
            response.try_recv(),
            Ok(ConfirmationDecision::ApproveInteractive)
        );
    }

    #[test]
    fn numbered_menu_supports_task_approval_and_reject_aliases() {
        let (mut ui, mut response) = request("touch /tmp/file");
        let view = ui.view();
        assert!(view.lines.iter().any(|line| line.starts_with("> 1.")));
        assert!(view.lines.iter().any(|line| line.contains("[a]")));
        assert!(ui.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE,)));
        assert_eq!(
            response.try_recv(),
            Ok(ConfirmationDecision::ApproveForTask)
        );

        let (mut rejected, mut rejected_response) = request("touch /tmp/file");
        assert!(rejected.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE,)));
        assert_eq!(
            rejected_response.try_recv(),
            Ok(ConfirmationDecision::Reject)
        );
    }

    #[test]
    fn high_risk_menu_disables_always_allow() {
        let (mut ui, mut response) = request("rm -rf /");
        let view = ui.view();
        assert!(view.lines.iter().any(|line| line.contains("不可用")));
        assert!(!ui.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE,)));
        assert!(matches!(
            response.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));

        assert!(!ui.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
        assert_eq!(ui.selection, 2);
    }

    #[test]
    fn arrow_navigation_and_fragmented_escape_sequence_keep_approval_open() {
        let (mut ui, mut response) = request("touch /tmp/file");
        for code in [KeyCode::Down, KeyCode::Up, KeyCode::Down, KeyCode::Up] {
            assert!(!ui.handle_key(KeyEvent::new(code, KeyModifiers::NONE)));
        }
        for code in [KeyCode::Esc, KeyCode::Char('['), KeyCode::Char('A')] {
            assert!(!ui.handle_key(KeyEvent::new(code, KeyModifiers::NONE)));
        }
        assert!(matches!(
            response.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        assert!(ui.request.is_some());
    }
}
