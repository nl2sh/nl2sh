//! One-shot channel decisions scoped to the exact inbound route and request ID.
use super::*;
use crate::{
    agent::{ConfirmationDecision, ConfirmationRequest, Confirmer, QuestionAnswers, UserQuestion},
    security::SecurityAssessment,
};
use tokio::sync::oneshot;

/// An assessed channel operation pattern: trusted media scope or exact preview; no shell wildcard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ApprovalPattern {
    tool: String,
    pattern: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    media_scope: Option<crate::agent::MediaApprovalScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    media_origin: Option<String>,
    risk: crate::security::RiskLevel,
    strong: bool,
    rules: Vec<String>,
}
impl ApprovalPattern {
    fn assessed(
        request: &ConfirmationRequest<'_>,
        assessment: &SecurityAssessment,
    ) -> Option<Self> {
        if assessment.requires_root || assessment.risk_level == crate::security::RiskLevel::Critical
        {
            return None;
        }
        let tool = request.tool?;
        if request.preview.is_empty() || request.preview.len() > 8192 {
            return None;
        }
        let mut rules: Vec<_> = assessment
            .matched_rules
            .iter()
            .map(|r| r.id.clone())
            .collect();
        rules.sort();
        rules.dedup();
        let media_scope = request.media_scope.clone().filter(|scope| {
            matches!(tool, "send_channel_file" | "send_channel_message")
                && crate::agent::can_remember_approval(assessment)
                && matches!(scope.kind.as_str(), "image" | "audio" | "video" | "file")
                && !scope.destination.is_empty()
        });
        Some(Self {
            tool: tool.into(),
            pattern: if media_scope.is_some() {
                String::new()
            } else {
                request.preview.into()
            },
            media_scope,
            media_origin: None,
            risk: assessment.risk_level,
            strong: assessment.requires_double_confirmation,
            rules,
        })
    }
    pub(super) fn id(&self) -> String {
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(self).unwrap_or_default())
        )
    }
}
pub(super) struct Pending {
    pub route: Route,
    expected: String,
    remember: bool,
    reply: oneshot::Sender<Option<String>>,
    pub(super) expires: std::time::Instant,
}
impl Pending {
    pub(super) fn accepts_button(&self, route: &Route, mode: &str) -> bool {
        self.route == *route
            && self.expires > std::time::Instant::now()
            && self.expected.starts_with("/approve ")
            && !self.expected.contains(" CONFIRM ")
            && (matches!(mode, "once" | "deny")
                || self.remember && matches!(mode, "session" | "always"))
    }
}
#[derive(Clone)]
pub(super) struct ChannelConfirmer {
    manager: Arc<ChannelManager>,
    channel: Arc<dyn MessageChannel>,
    message: Incoming,
    cancel: watch::Receiver<bool>,
    images: Vec<crate::llm::ToolAttachment>,
    attachments: Arc<super::attachments::AttachmentStore>,
    refused: Arc<Mutex<std::collections::HashSet<String>>>,
}
impl ChannelConfirmer {
    pub fn new(
        manager: Arc<ChannelManager>,
        channel: Arc<dyn MessageChannel>,
        message: &Incoming,
        cancel: watch::Receiver<bool>,
    ) -> Self {
        Self {
            manager,
            channel,
            message: Incoming {
                route: message.route.clone(),
                id: message.id.clone(),
                text: String::new(),
                context: message.context.clone(),
                media: Vec::new(),
            },
            cancel,
            images: Vec::new(),
            attachments: Arc::new(super::attachments::AttachmentStore::default()),
            refused: Arc::new(Mutex::new(std::collections::HashSet::new())),
        }
    }
    pub fn with_images(mut self, images: Vec<crate::llm::ToolAttachment>) -> Self {
        self.images = images;
        self
    }
    async fn wait(
        &self,
        prompt: &str,
        expected: impl FnOnce(&str) -> String,
        remember: bool,
    ) -> Result<Option<String>> {
        if *self.cancel.borrow() {
            return Ok(None);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        let expected = expected(&id);
        {
            let mut pending = self
                .manager
                .pending
                .lock()
                .map_err(|_| anyhow!("channel approval unavailable"))?;
            if pending.len() >= 8 {
                bail!("too many pending channel requests");
            }
            pending.insert(
                id.clone(),
                Pending {
                    route: self.message.route.clone(),
                    expected: expected.clone(),
                    remember,
                    reply: tx,
                    expires: std::time::Instant::now() + Duration::from_secs(120),
                },
            );
        }
        // Never truncate the operation preview: if it cannot fit, refuse the operation.
        let language = self.manager.language().await;
        let zh = language == crate::config::UiLanguage::ZhCn;
        let ordinary = expected.starts_with("/approve ") && !expected.contains(" CONFIRM ");
        let alternatives = if expected.starts_with("/approve ") && !expected.contains(" CONFIRM ") {
            if remember {
                if zh {
                    "\n- `/approve` = 仅允许一次\n- `/approve session` = 本会话全部普通操作（高危/Root仍询问）\n- `/approve always` = 永久允许所示范围"
                } else {
                    "\n- `/approve` = allow once\n- `/approve session` = all ordinary operations in this conversation (high risk / Root still prompt)\n- `/approve always` = the displayed scope permanently"
                }
            } else {
                if zh {
                    "\n- `/approve` = 仅允许一次；此操作不能保存授权"
                } else {
                    "\n- `/approve` = allow once; cannot remember this operation"
                }
            }
        } else {
            ""
        };
        let text = if zh {
            format!("{}\n\n**请求**: `{id}`\n\n**120 秒内回复**:\n{}\n{alternatives}\n\n拒绝: `/deny` 或 `/deny {id}`", super::markdown::literal(prompt), super::markdown::literal(&expected))
        } else {
            format!("{}\n\n**Request**: `{id}`\n\n**Reply within 120s**:\n{}\n{alternatives}\n\nReject: `/deny` or `/deny {id}`", super::markdown::literal(prompt), super::markdown::literal(&expected))
        };
        let text = self.manager.redact(&text).await;
        let mut cancel = self.cancel.clone();
        let result = if text.len() > 12000 {
            Ok(None)
        } else if if ordinary {
            self.channel
                .send_approval(&self.message, &text, &id, remember, language)
                .await
                .is_err()
        } else {
            self.channel
                .send_formatted(&self.message, &text, "markdown", &[])
                .await
                .is_err()
        } {
            Ok(None)
        } else {
            tokio::select! {biased;_=cancelled(&mut cancel)=>Ok(None),value=tokio::time::timeout(Duration::from_secs(120),rx)=>Ok(value.ok().and_then(|v|v.ok()).flatten())}
        };
        if let Ok(mut pending) = self.manager.pending.lock() {
            pending.remove(&id);
        }
        result
    }
}
#[async_trait]
impl Confirmer for ChannelConfirmer {
    fn input_images(&self) -> Vec<crate::llm::ToolAttachment> {
        self.images.clone()
    }
    fn file_delivery(&self) -> Option<Arc<dyn crate::agent::FileDelivery>> {
        Some(Arc::new(self.clone()))
    }
    fn audit_source(&self) -> &'static str {
        "message_channel"
    }
    fn audit_session(&self) -> Option<String> {
        Some(self.message.route.key())
    }
    fn approval_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }
    async fn confirm(
        &self,
        request: &ConfirmationRequest<'_>,
        assessment: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        let pattern = ApprovalPattern::assessed(request, assessment).map(|mut pattern| {
            if pattern.media_scope.is_some() {
                pattern.media_origin = Some(self.message.route.key());
            }
            pattern
        });
        // Tool aliases with the same assessed action must not reprompt after refusal.
        let refusal_key = format!(
            "{:?}:{}:{}:{}",
            assessment.risk_level,
            assessment.requires_root,
            assessment.requires_double_confirmation,
            request.preview
        );
        let refusal_key = format!("{:x}", Sha256::digest(refusal_key.as_bytes()));
        if self
            .refused
            .lock()
            .map_err(|_| anyhow!("channel refusal state unavailable"))?
            .contains(&refusal_key)
        {
            return Ok(ConfirmationDecision::Reject);
        }
        if self.approval_cancelled() {
            return Ok(ConfirmationDecision::Reject);
        }
        let session = crate::agent::can_remember_approval(assessment)
            && self
                .manager
                .session_grants
                .lock()
                .map_err(|_| anyhow!("channel session grant unavailable"))?
                .contains_key(&self.message.route.key());
        let permanent = if let Some(pattern) = &pattern {
            self.manager
                .saved
                .lock()
                .await
                .command_allowlist
                .iter()
                .any(|saved| {
                    if saved == pattern {
                        return true;
                    }
                    // Legacy records retain their exact-preview scope; never silently broaden them.
                    let mut exact = pattern.clone();
                    exact.media_scope = None;
                    exact.media_origin = None;
                    exact.pattern = request.preview.into();
                    saved == &exact
                })
        } else {
            false
        };
        if session || permanent {
            return Ok(if self.approval_cancelled() {
                ConfirmationDecision::Reject
            } else {
                ConfirmationDecision::ApproveByGrant(format!(
                    "channel:{}:{}",
                    if permanent { "always" } else { "session" },
                    if permanent {
                        pattern
                            .as_ref()
                            .map(ApprovalPattern::id)
                            .unwrap_or_default()
                    } else {
                        format!("{:x}", Sha256::digest(self.message.route.key().as_bytes()))
                    }
                ))
            });
        }
        let zh = self.manager.language().await == crate::config::UiLanguage::ZhCn;
        let labels = if zh {
            [
                "操作审批",
                "工具",
                "风险",
                "规则",
                "永久授权范围",
                "不可保存",
            ]
        } else {
            [
                "Operation approval",
                "Tool",
                "Risk",
                "Rules",
                "Permanent approval scope",
                "Not eligible",
            ]
        };
        let risk = match (zh, assessment.risk_level) {
            (true, crate::security::RiskLevel::ReadOnly) => "只读",
            (true, crate::security::RiskLevel::Mutating) => "修改",
            (true, crate::security::RiskLevel::Dangerous) => "危险",
            (true, crate::security::RiskLevel::Critical) => "严重危险",
            (false, crate::security::RiskLevel::ReadOnly) => "Read-only",
            (false, crate::security::RiskLevel::Mutating) => "Mutating",
            (false, crate::security::RiskLevel::Dangerous) => "Dangerous",
            (false, crate::security::RiskLevel::Critical) => "Critical",
        };
        let prompt = format!(
            "{}\n{}: {}\n{}: {}{}\n{}\n{}: {}\n{}: {}",
            labels[0],
            labels[1],
            request.tool.unwrap_or("unknown"),
            labels[2],
            risk,
            if assessment.requires_root {
                " · ROOT"
            } else {
                ""
            },
            request.preview,
            labels[3],
            assessment
                .matched_rules
                .iter()
                .map(|r| r.message.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            labels[4],
            pattern
                .as_ref()
                .map(|p| if let Some(scope) = &p.media_scope {
                    format!(
                        "{} / {} / {} / {}\n{}: {}",
                        p.tool,
                        scope.destination,
                        scope.kind,
                        if zh {
                            "同类媒体；不限文件内容、路径及说明"
                        } else {
                            "same media kind; any file content, path and caption"
                        },
                        if zh { "授权编号" } else { "Approval ID" },
                        p.id()
                    )
                } else {
                    format!("{} / SHA-256 {}", p.tool, p.id())
                })
                .unwrap_or_else(|| labels[5].into())
        );
        let choice = self
            .wait(&prompt, |id| format!("/approve {id}"), pattern.is_some())
            .await?;
        let Some(choice) = choice else {
            self.refused
                .lock()
                .map_err(|_| anyhow!("channel refusal state unavailable"))?
                .insert(refusal_key.clone());
            return Ok(ConfirmationDecision::Reject);
        };
        if assessment.requires_double_confirmation {
            let strong_prompt = format!(
                "{}\n{}: {choice}\n{prompt}",
                if zh {
                    "危险操作第二次确认"
                } else {
                    "Second confirmation for dangerous operation"
                },
                if zh { "授权范围" } else { "Scope" }
            );
            if self
                .wait(
                    &strong_prompt,
                    |id| format!("/approve {id} CONFIRM {id}"),
                    false,
                )
                .await?
                .is_none()
            {
                self.refused
                    .lock()
                    .map_err(|_| anyhow!("channel refusal state unavailable"))?
                    .insert(refusal_key);
                return Ok(ConfirmationDecision::Reject);
            }
        }
        if self.approval_cancelled() {
            return Ok(ConfirmationDecision::Reject);
        }
        if choice == "session" {
            let mut list = self
                .manager
                .session_grants
                .lock()
                .map_err(|_| anyhow!("channel session grant unavailable"))?;
            if list.len() >= 512 && !list.contains_key(&self.message.route.key()) {
                bail!("channel session grant limit reached");
            }
            list.insert(self.message.route.key(), ());
        }
        if let Some(pattern) = pattern {
            if choice == "always" {
                let mut saved = self.manager.saved.lock().await;
                if !saved.command_allowlist.contains(&pattern) {
                    if saved.command_allowlist.len() >= 256 {
                        bail!("permanent channel pattern limit reached");
                    }
                    let mut next = saved.clone();
                    next.command_allowlist.push(pattern);
                    if serde_json::to_vec(&next.command_allowlist)?.len() > 512 * 1024 {
                        bail!("permanent channel patterns exceed storage limit");
                    }
                    self.manager.persist(&next).await?;
                    *saved = next;
                }
            }
        }
        if matches!(choice.as_str(), "session" | "always") {
            let notice = match (zh, choice.as_str()) {
                (true, "session") => "**已允许本会话全部普通操作。** 新会话或重启后失效，高危/Root操作仍需审批。",
                (true, _) => "**已保存永久授权。** 仅对上方展示的授权范围生效，其他操作仍需审批。",
                (false, "session") => "**All ordinary operations are allowed for this conversation.** Expires on /new or restart; high-risk/Root operations still require approval.",
                (false, _) => "**Permanent approval saved.** Applies only to the scope displayed above; other operations still require approval.",
            };
            // Persistence has succeeded. A failed status notice must not replay the operation.
            let _ = self
                .channel
                .send_formatted(&self.message, notice, "markdown", &[])
                .await;
        }
        Ok(ConfirmationDecision::Approve)
    }
    async fn ask_questions(&self, questions: &[UserQuestion]) -> Result<Option<QuestionAnswers>> {
        let mut answers = QuestionAnswers::new();
        for question in questions {
            let options = question
                .options
                .iter()
                .map(|o| format!("{} = {} ({})", o.label, o.value, o.description))
                .collect::<Vec<_>>()
                .join("\n");
            let zh = self.manager.language().await == crate::config::UiLanguage::ZhCn;
            let prompt = format!(
                "{}\n{}\n{}\n{}",
                if zh {
                    "补充信息"
                } else {
                    "Information needed"
                },
                question.header,
                question.prompt,
                options
            );
            let Some(answer) = self
                .wait(
                    &prompt,
                    |id| {
                        format!(
                            "/answer {id} <{}>",
                            if zh { "你的回答" } else { "your answer" }
                        )
                    },
                    false,
                )
                .await?
            else {
                return Ok(None);
            };
            answers.insert(question.id.clone(), answer);
        }
        Ok(Some(answers))
    }
}
/// Consume a validated callback atomically; ACK transport never grants additional rights.
pub(super) fn button(
    manager: &ChannelManager,
    route: &Route,
    id: &str,
    mode: &str,
) -> Result<bool> {
    let decision = {
        let mut pending = manager
            .pending
            .lock()
            .map_err(|_| anyhow!("channel approval unavailable"))?;
        if pending
            .get(id)
            .is_some_and(|p| p.accepts_button(route, mode))
        {
            pending.remove(id)
        } else {
            None
        }
    };
    if let Some(pending) = decision {
        let value = match mode {
            "deny" => None,
            "once" => Some("approved".into()),
            _ => Some(mode.into()),
        };
        Ok(pending.reply.send(value).is_ok())
    } else {
        Ok(false)
    }
}
pub(super) async fn control(
    manager: &ChannelManager,
    message: &Incoming,
    channel: &dyn MessageChannel,
) -> Result<bool> {
    let mut parts = message.text.trim().splitn(3, ' ');
    let command = parts.next().unwrap_or_default();
    if !matches!(command, "/approve" | "/deny" | "/answer") {
        return Ok(false);
    }
    let argument = parts.next().unwrap_or_default();
    let value = parts.next().unwrap_or_default();
    let implicit = value.is_empty()
        && (argument.is_empty()
            || command == "/approve" && matches!(argument, "session" | "always"));
    let decision = {
        let mut pending = manager
            .pending
            .lock()
            .map_err(|_| anyhow!("channel approval unavailable"))?;
        let id = if implicit {
            let eligible: Vec<_> = pending
                .iter()
                .filter(|(_, p)| p.route == message.route && p.expires > std::time::Instant::now())
                .map(|(id, _)| id.clone())
                .collect();
            if eligible.len() == 1 {
                eligible[0].clone()
            } else {
                String::new()
            }
        } else {
            argument.to_owned()
        };
        let valid = pending.get(&id).is_some_and(|p| {
            p.route == message.route
                && p.expires > std::time::Instant::now()
                && (command == "/deny" && value.is_empty()
                    || command == "/approve"
                        && (message.text.trim() == p.expected
                            || matches!(value, "session" | "always")
                                && p.remember
                                && p.expected == format!("/approve {id}")
                            || implicit
                                && p.expected.starts_with("/approve ")
                                && !p.expected.contains(" CONFIRM ")
                                && (argument.is_empty() || p.remember))
                    || command == "/answer"
                        && p.expected.starts_with("/answer ")
                        && !value.trim().is_empty())
        });
        if valid {
            pending.remove(&id)
        } else {
            None
        }
    };
    if let Some(pending) = decision {
        let _ = pending.reply.send(if command == "/deny" {
            None
        } else if command == "/answer" {
            Some(value.trim().into())
        } else {
            Some(if !implicit && matches!(value, "session" | "always") {
                value.into()
            } else if implicit && matches!(argument, "session" | "always") {
                argument.into()
            } else {
                "approved".into()
            })
        });
    } else {
        let zh = manager.language().await == crate::config::UiLanguage::ZhCn;
        channel
            .send_formatted(
                message,
                if zh {
                    "没有匹配的待审批请求，或编号、发送者、确认短语不匹配/请求已过期。请等待审批消息后单独回复 /approve、/approve session、/approve always 或 /deny；这些命令不会修改全局审批策略。"
                } else {
                    "No matching pending request, or the ID, sender or confirmation phrase does not match / has expired. Reply with /approve, /approve session, /approve always or /deny after an approval prompt; these commands do not change global approval policy."
                },
                "markdown",
                &[],
            )
            .await?;
    }
    Ok(true)
}

#[async_trait]
impl crate::agent::FileDelivery for ChannelConfirmer {
    fn supports_buttons(&self) -> bool {
        self.message.route.kind == "qq"
    }
    fn destination(&self) -> String {
        format!(
            "{} / {} / {} (sender {})",
            self.message.route.kind,
            self.message.route.account,
            self.message.route.peer,
            self.message.route.sender
        )
    }
    async fn stage_attachment(
        &self,
        attachment: &crate::llm::ToolAttachment,
    ) -> Result<Option<String>> {
        if attachment.base64_data.len() > media::MAX_FILE_BYTES.div_ceil(3) * 4 {
            bail!("tool attachment exceeds delivery limit");
        }
        let attachment = attachment.clone();
        let store = self.attachments.clone();
        tokio::task::spawn_blocking(move || store.stage(&attachment).map(Some)).await?
    }
    fn attachment(&self, id: &str) -> Result<(String, Vec<u8>)> {
        self.attachments.snapshot(id)
    }
    async fn send_message(
        &self,
        text: &str,
        format: &str,
        buttons: &[outbound::MessageButton],
    ) -> Result<Value> {
        let text = self.manager.redact(text).await;
        let mut cancel = self.cancel.clone();
        if *cancel.borrow() {
            bail!("message delivery cancelled");
        }
        tokio::select! {biased;_=cancelled(&mut cancel)=>bail!("message delivery cancelled; do not automatically retry"), result=self.channel.send_formatted(&self.message,&text,format,buttons)=>result}
    }
    async fn send_file(&self, name: &str, kind: &str, bytes: Vec<u8>, caption: &str) -> Result<()> {
        let caption = self.manager.redact(caption).await;
        let mut cancel = self.cancel.clone();
        if *cancel.borrow() {
            bail!("file delivery cancelled");
        }
        tokio::select! {biased;_=cancelled(&mut cancel)=>bail!("file delivery cancelled; do not automatically retry"),result=self.channel.send_media(&self.message,name,kind,bytes,&caption)=>result}
    }
}
