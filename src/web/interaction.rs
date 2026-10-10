use super::*;

// Browser clients cannot drive a terminal PTY. Keep every web execution in
// the captured pipeline while preserving the runner's assessment and approval.
pub(in crate::web) struct CapturedExecutor(pub(in crate::web) ShellExecutor);

#[async_trait]
impl CommandExecutor for CapturedExecutor {
    async fn execute_probe(&self, command: &str) -> Result<ExecutionResult> {
        self.0.execute_probe(command).await
    }
    async fn execute_readonly(&self, command: &str) -> Result<ExecutionResult> {
        self.0.execute_readonly(command).await
    }
    async fn execute_machine(&self, command: &str, needs_root: bool) -> Result<ExecutionResult> {
        self.0.execute_machine(command, needs_root).await
    }

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

pub(in crate::web) struct WebOutput {
    pub(in crate::web) session: Arc<WebSession>,
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
    pub(in crate::web) fn add(&self, prefix: &str, text: &str) {
        if let Ok(mut inner) = self.session.inner.lock() {
            push(
                &mut inner,
                format!("{prefix} {}", crate::limits::truncate_text(text, 16 * 1024)),
            );
        }
        notify(&self.session, "output");
    }
}

pub(in crate::web) struct WebTextSink {
    pub(in crate::web) session: Arc<WebSession>,
    pub(in crate::web) state: Option<Arc<Shared>>,
}

impl WebTextSink {
    pub(in crate::web) fn record_generation(&self) {
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

    pub(in crate::web) fn schedule_checkpoint(&self) {
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

pub(in crate::web) struct WebConfirmer {
    pub(in crate::web) session: Arc<WebSession>,
    pub(in crate::web) state: Option<Arc<Shared>>,
}

pub(in crate::web) fn approval_explanation(assessment: &SecurityAssessment) -> String {
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
    fn approval_cancelled(&self) -> bool {
        *self.session.cancel.borrow()
    }
    fn audit_session(&self) -> Option<String> {
        self.session
            .inner
            .lock()
            .ok()
            .map(|session| session.id.clone())
    }
    fn audit_source(&self) -> &'static str {
        "web"
    }
    async fn confirm(
        &self,
        command: &crate::agent::ConfirmationRequest<'_>,
        assessment: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        let command = command.preview;
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
    pub(in crate::web) async fn wait(&self, pending: Pending) -> Result<Reply> {
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
