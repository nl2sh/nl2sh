use super::*;
use crate::agent::{ConfirmationDecision, ConfirmationRequest, Confirmer};
use crate::security::{RiskLevel, SecurityAssessment};
use std::os::unix::fs::PermissionsExt;
use wiremock::{
    matchers::{body_partial_json, header, method, path},
    Mock, MockServer, ResponseTemplate,
};

#[derive(Default)]
pub(super) struct MockChannel {
    sent: Mutex<Vec<String>>,
    formats: Mutex<Vec<String>>,
    files: Mutex<Vec<(String, Vec<u8>)>>,
    changed: tokio::sync::Notify,
}
#[async_trait]
impl MessageChannel for MockChannel {
    async fn send(&self, _: &Incoming, text: &str) -> Result<()> {
        self.sent
            .lock()
            .map_err(|_| anyhow!("poisoned"))?
            .push(text.into());
        self.changed.notify_one();
        Ok(())
    }
    async fn send_formatted(
        &self,
        message: &Incoming,
        text: &str,
        format: &str,
        buttons: &[outbound::MessageButton],
    ) -> Result<Value> {
        if !buttons.is_empty() {
            bail!("buttons unsupported");
        }
        self.formats
            .lock()
            .map_err(|_| anyhow!("poisoned"))?
            .push(format.into());
        self.send(message, text).await?;
        Ok(json!({"status":"submitted"}))
    }
    async fn send_media(
        &self,
        _: &Incoming,
        name: &str,
        _: &str,
        bytes: Vec<u8>,
        _: &str,
    ) -> Result<()> {
        self.files
            .lock()
            .map_err(|_| anyhow!("poisoned"))?
            .push((name.into(), bytes));
        Ok(())
    }
    async fn receive(
        &self,
        _: Arc<ChannelManager>,
        _: &mpsc::Sender<Incoming>,
        _: watch::Receiver<bool>,
    ) -> Result<()> {
        Ok(())
    }
    fn secrets(&self) -> Vec<String> {
        vec!["channel-secret".into()]
    }
}
fn message(sender: &str, text: &str) -> Incoming {
    Incoming {
        route: Route {
            kind: "qq".into(),
            account: "1".into(),
            peer: "group".into(),
            sender: sender.into(),
            group: true,
        },
        id: uuid::Uuid::new_v4().to_string(),
        text: text.into(),
        context: "context".into(),
        media: Vec::new(),
    }
}
pub(super) async fn manager() -> Result<(tempfile::TempDir, Arc<ChannelManager>)> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    crate::config::save_config(&config, &crate::config::Config::default())?;
    let manager = ChannelManager::start(config).await?;
    Ok((dir, manager))
}
fn assessment(strong: bool) -> SecurityAssessment {
    SecurityAssessment::from_policy(
        if strong {
            RiskLevel::Dangerous
        } else {
            RiskLevel::Mutating
        },
        vec![],
        false,
        "test".into(),
        false,
    )
}

#[tokio::test]
async fn private_state_and_replay_ledger_survive_restart() -> Result<()> {
    let (dir, manager) = manager().await?;
    let mut m = message("alice", "diagnose");
    m.id = "m".repeat(512);
    assert!(manager.admit(&m).await?);
    assert_eq!(
        manager.saved.lock().await.seen.front().map(String::len),
        Some(64)
    );
    assert!(!manager.admit(&m).await?);
    let config = manager.path.clone();
    manager.shutdown().await;
    drop(manager);
    let second = ChannelManager::start(config).await?;
    assert!(!second.admit(&m).await?);
    assert_eq!(
        std::fs::metadata(dir.path().join("channels/accounts.json"))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(dir.path().join("channels"))?
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert!(Store::open(&second.path).is_err());
    second.shutdown().await;
    Ok(())
}
#[tokio::test]
async fn approval_is_one_shot_and_bound_to_sender_chat_account_and_phrase() -> Result<()> {
    let (_dir, manager) = manager().await?;
    let channel = Arc::new(MockChannel::default());
    let incoming = message("alice", "diagnose");
    let (_cancel, rx) = watch::channel(false);
    let confirmer =
        approval::ChannelConfirmer::new(manager.clone(), channel.clone(), &incoming, rx);
    let task = tokio::spawn(async move {
        confirmer
            .confirm(
                &ConfirmationRequest::shell("dangerous preview"),
                &assessment(true),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), channel.changed.notified()).await?;
    let id = manager
        .pending
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .keys()
        .next()
        .context("missing approval")?
        .clone();
    approval::control(
        &manager,
        &message("alice", &format!("/approve {id}")),
        channel.as_ref(),
    )
    .await?;
    tokio::time::timeout(Duration::from_secs(2), channel.changed.notified()).await?;
    assert!(!task.is_finished());
    let id = manager
        .pending
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .keys()
        .next()
        .context("missing second approval")?
        .clone();
    let exact = format!("/approve {id} CONFIRM {id}");
    let wrong = message("mallory", &exact);
    assert!(approval::control(&manager, &wrong, channel.as_ref()).await?);
    assert!(!task.is_finished());
    let mut wrong_chat = message("alice", &exact);
    wrong_chat.route.peer = "other".into();
    approval::control(&manager, &wrong_chat, channel.as_ref()).await?;
    assert!(!task.is_finished());
    let mut wrong_account = message("alice", &exact);
    wrong_account.route.account = "other".into();
    approval::control(&manager, &wrong_account, channel.as_ref()).await?;
    assert!(!task.is_finished());
    approval::control(
        &manager,
        &message("alice", &format!("/approve {id}")),
        channel.as_ref(),
    )
    .await?;
    assert!(!task.is_finished());
    approval::control(&manager, &message("alice", &exact), channel.as_ref()).await?;
    assert_eq!(task.await??, ConfirmationDecision::Approve);
    assert!(manager
        .pending
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .is_empty());
    approval::control(&manager, &message("alice", &exact), channel.as_ref()).await?;
    assert!(channel
        .sent
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .last()
        .is_some_and(|s| s.contains("过期")));
    manager.shutdown().await;
    Ok(())
}
#[tokio::test]
async fn expired_approval_and_cancel_refuse() -> Result<()> {
    let (_dir, manager) = manager().await?;
    let channel = Arc::new(MockChannel::default());
    let incoming = message("alice", "diagnose");
    let (cancel, rx) = watch::channel(false);
    let confirmer =
        approval::ChannelConfirmer::new(manager.clone(), channel.clone(), &incoming, rx);
    let task = tokio::spawn(async move {
        confirmer
            .confirm(&ConfirmationRequest::shell("preview"), &assessment(false))
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), channel.changed.notified()).await?;
    let id = {
        let mut pending = manager.pending.lock().map_err(|_| anyhow!("poisoned"))?;
        let (id, p) = pending.iter_mut().next().context("missing approval")?;
        p.expires = std::time::Instant::now() - Duration::from_secs(1);
        id.clone()
    };
    approval::control(
        &manager,
        &message("alice", &format!("/approve {id}")),
        channel.as_ref(),
    )
    .await?;
    assert!(!task.is_finished());
    cancel.send_replace(true);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), task).await???,
        ConfirmationDecision::Reject
    );
    assert!(manager
        .pending
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .is_empty());
    manager.shutdown().await;
    Ok(())
}
#[tokio::test]
async fn oversized_preview_refuses_without_sending_truncated_operation() -> Result<()> {
    let (_dir, manager) = manager().await?;
    let channel = Arc::new(MockChannel::default());
    let (_cancel, rx) = watch::channel(false);
    let confirmer = approval::ChannelConfirmer::new(
        manager.clone(),
        channel.clone(),
        &message("alice", "hi"),
        rx,
    );
    assert_eq!(
        confirmer
            .confirm(
                &ConfirmationRequest::shell(&"x".repeat(16000)),
                &assessment(false)
            )
            .await?,
        ConfirmationDecision::Reject
    );
    assert!(channel
        .sent
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .is_empty());
    assert!(manager
        .pending
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .is_empty());
    manager.shutdown().await;
    Ok(())
}
#[tokio::test]
async fn weixin_reply_echoes_latest_context_and_authentication() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/ilink/bot/sendmessage")).and(header("authorization","Bearer bot-token")).and(header("AuthorizationType","ilink_bot_token"))
        .and(body_partial_json(json!({"base_info":{"channel_version":"2.2.0"},"msg":{"to_user_id":"group","context_token":"latest","message_type":2,"message_state":2,"item_list":[{"type":1,"text_item":{"text":"diagnosis"}}]}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ret":0}))).expect(1).mount(&server).await;
    let channel = weixin::Weixin::new(Account {
        id: "bot".into(),
        secret: "bot-token".into(),
        base_url: server.uri(),
        enabled: true,
    });
    let original = message("alice", "hi");
    let mut newer = message("alice", "approval");
    newer.context = "latest".into();
    channel.observe(&newer);
    channel.send(&original, "diagnosis").await?;
    Ok(())
}
#[tokio::test]
async fn protocol_errors_and_reply_size_are_bounded() -> Result<()> {
    let server = MockServer::start().await;
    for value in [
        json!({"ret":-14}),
        json!({"code":100016}),
        json!({"err_code":400}),
        json!({"biz_code":"40093002"}),
    ] {
        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(value))
            .mount(&server)
            .await;
        assert!(response_json(client().get(server.uri()).send().await?)
            .await
            .is_err());
    }
    server.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(1024 * 1024 + 1)))
        .mount(&server)
        .await;
    assert!(response_json(client().get(server.uri()).send().await?)
        .await
        .is_err());
    let text = "中文🙂".repeat(1000);
    let parts = chunks(&text, 3500);
    assert_eq!(parts.concat(), text);
    assert!(parts.iter().all(|p| p.len() <= 3500));
    Ok(())
}
fn stream_response(mut value: Value) -> ResponseTemplate {
    let choice = &mut value["choices"][0];
    let mut delta = choice["message"].take();
    if let Some(calls) = delta["tool_calls"].as_array_mut() {
        for (index, call) in calls.iter_mut().enumerate() {
            call["index"] = json!(index);
        }
    }
    choice
        .as_object_mut()
        .expect("valid fixture")
        .remove("message");
    choice["delta"] = delta;
    choice["index"] = json!(0);
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(format!("data: {value}\n\ndata: [DONE]\n\n"))
}

#[tokio::test]
async fn channel_diagnosis_uses_agent_tools_and_saves_isolated_history() -> Result<()> {
    let (dir, manager) = manager().await?;
    let server = MockServer::start().await;
    let target = dir.path().join("evidence.txt");
    std::fs::write(&target, "real diagnostic evidence")?;
    Mock::given(method("POST")).and(path("/v1/chat/completions"))
        .respond_with(stream_response(json!({"choices":[{"message":{"content":null,"tool_calls":[{"id":"read","type":"function","function":{"name":"read_file","arguments":json!({"path":target}).to_string()}}]},"finish_reason":"tool_calls"}]}))).up_to_n_times(1).mount(&server).await;
    Mock::given(method("POST")).and(path("/v1/chat/completions"))
        .respond_with(stream_response(json!({"choices":[{"message":{"content":"diagnosis completed"},"finish_reason":"stop"}]}))).with_priority(6).mount(&server).await;
    let cfg = crate::config::Config {
        endpoint: format!("{}/v1", server.uri()),
        api_type: crate::config::ApiType::ChatCompletions,
        api_key: "model-key".into(),
        ..crate::config::Config::default()
    };
    crate::config::save_config(&manager.path, &cfg)?;
    let channel = Arc::new(MockChannel::default());
    let (tx, rx) = mpsc::channel(32);
    let (cancel, cancel_rx) = watch::channel(false);
    let worker = tokio::spawn(worker(manager.clone(), channel.clone(), rx, cancel_rx));
    let incoming = message("alice", "read the diagnostic evidence file");
    let key = incoming.route.key();
    manager.intake(incoming, &tx, channel.as_ref()).await?;
    tokio::time::timeout(Duration::from_secs(10), channel.changed.notified()).await?;
    assert!(channel
        .sent
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .iter()
        .any(|s| s == "diagnosis completed"));
    let store = crate::sessions::SessionStore::open(&manager.path)?;
    let history = store.load(&session_id(&key, ""), 16, 64000)?;
    assert!(serde_json::to_string(&history)?.contains("real diagnostic evidence"));
    cancel.send_replace(true);
    drop(tx);
    worker.await?;
    manager.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn channel_mutation_waits_for_remote_approval_even_when_protocol_auto_approve_is_on(
) -> Result<()> {
    let (dir, manager) = manager().await?;
    let server = MockServer::start().await;
    let target = dir.path().join("approved.txt");
    Mock::given(method("POST")).and(path("/v1/chat/completions"))
        .respond_with(stream_response(json!({"choices":[{"message":{"content":null,"tool_calls":[{"id":"patch","type":"function","function":{"name":"apply_patch","arguments":json!({"path":target,"old_text":"","new_text":"approved"}).to_string()}}]},"finish_reason":"tool_calls"}]}))).up_to_n_times(1).mount(&server).await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(stream_response(
            json!({"choices":[{"message":{"content":"finished"},"finish_reason":"stop"}]}),
        ))
        .with_priority(6)
        .mount(&server)
        .await;
    let cfg = crate::config::Config {
        endpoint: format!("{}/v1", server.uri()),
        api_type: crate::config::ApiType::ChatCompletions,
        api_key: "model-key".into(),
        protocol_auto_approve: true,
        ..crate::config::Config::default()
    };
    crate::config::save_config(&manager.path, &cfg)?;
    let channel = Arc::new(MockChannel::default());
    let (tx, rx) = mpsc::channel(32);
    let (cancel, cancel_rx) = watch::channel(false);
    let worker = tokio::spawn(worker(manager.clone(), channel.clone(), rx, cancel_rx));
    manager
        .intake(message("alice", "create the file"), &tx, channel.as_ref())
        .await?;
    tokio::time::timeout(Duration::from_secs(10), channel.changed.notified()).await?;
    assert!(!target.exists());
    let id = manager
        .pending
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .keys()
        .next()
        .context("missing approval")?
        .clone();
    manager
        .intake(
            message("alice", &format!("/approve {id}")),
            &tx,
            channel.as_ref(),
        )
        .await?;
    tokio::time::timeout(Duration::from_secs(10), channel.changed.notified()).await?;
    assert_eq!(std::fs::read_to_string(&target)?, "approved");
    let log = std::fs::read_to_string(dir.path().join("nl2sh.log"))?;
    assert!(log.contains("message_channel"));
    cancel.send_replace(true);
    drop(tx);
    worker.await?;
    manager.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn same_uid_control_shares_status_and_cannot_read_credentials() -> Result<()> {
    let (_dir, manager) = manager().await?;
    {
        let mut saved = manager.saved.lock().await;
        saved.qq = Some(Account {
            id: "123".into(),
            secret: "private-secret".into(),
            base_url: "https://api.bot.qq.com".into(),
            enabled: false,
        });
        manager.persist(&saved).await?;
    }
    let snapshot = control::snapshot(&manager.path).await?;
    let serialized = serde_json::to_string(&snapshot)?;
    assert!(!serialized.contains("private-secret"));
    assert!(serialized.contains("123"));
    control::apply(&manager.path, &ChannelAction::Stop { kind: "qq".into() }).await?;
    assert!(control::apply(
        &manager.path,
        &ChannelAction::ConfigureQq {
            robot_uin: String::new(),
            app_id: "456".into(),
            secret: String::new()
        }
    )
    .await
    .is_err());
    assert!(manager.redact("private-secret").await.contains("REDACTED"));
    control::apply(&manager.path, &ChannelAction::Forget { kind: "qq".into() }).await?;
    assert!(manager.account("qq").await.is_none());
    manager.shutdown().await;
    assert!(control::snapshot(&manager.path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn sending_files_waits_for_chat_approval_and_delivers_preapproval_snapshot() -> Result<()> {
    let (dir, manager) = manager().await?;
    let server = MockServer::start().await;
    let target = dir.path().join("report.bin");
    std::fs::write(&target, b"original bytes")?;
    Mock::given(method("POST")).and(path("/v1/chat/completions"))
        .respond_with(stream_response(json!({"choices":[{"message":{"content":null,"tool_calls":[{"id":"send","type":"function","function":{"name":"send_channel_file","arguments":json!({"path":target,"kind":"file"}).to_string()}}]},"finish_reason":"tool_calls"}]}))).up_to_n_times(1).mount(&server).await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(stream_response(
            json!({"choices":[{"message":{"content":"file delivered"},"finish_reason":"stop"}]}),
        ))
        .with_priority(6)
        .mount(&server)
        .await;
    let cfg = crate::config::Config {
        endpoint: format!("{}/v1", server.uri()),
        api_type: crate::config::ApiType::ChatCompletions,
        api_key: "model-key".into(),
        protocol_auto_approve: true,
        ..crate::config::Config::default()
    };
    crate::config::save_config(&manager.path, &cfg)?;
    let channel = Arc::new(MockChannel::default());
    let (tx, rx) = mpsc::channel(32);
    let (cancel, signal) = watch::channel(false);
    let task = tokio::spawn(worker(manager.clone(), channel.clone(), rx, signal));
    manager
        .intake(message("alice", "send the report"), &tx, channel.as_ref())
        .await?;
    tokio::time::timeout(Duration::from_secs(10), channel.changed.notified()).await?;
    assert!(channel
        .files
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .is_empty());
    assert!(channel
        .sent
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .iter()
        .any(|text| text.contains("SHA-256") && text.contains("report.bin")));
    let id = manager
        .pending
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .keys()
        .next()
        .context("approval missing")?
        .clone();
    std::fs::write(&target, b"changed after preview")?;
    manager
        .intake(
            message("bob", &format!("/approve {id}")),
            &tx,
            channel.as_ref(),
        )
        .await?;
    assert!(channel
        .files
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .is_empty());
    manager
        .intake(
            message("alice", &format!("/approve {id} always")),
            &tx,
            channel.as_ref(),
        )
        .await?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if !channel
                .files
                .lock()
                .map_err(|_| anyhow!("poisoned"))?
                .is_empty()
            {
                break Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    assert_eq!(
        channel.files.lock().map_err(|_| anyhow!("poisoned"))?[0].1,
        b"original bytes"
    );
    let grants = serde_json::to_value(&manager.saved.lock().await.command_allowlist)?;
    assert_eq!(grants[0]["media_scope"]["kind"], "file");
    assert!(grants[0]["media_origin"].as_str().is_some());
    assert_eq!(grants[0]["pattern"], "");
    cancel.send_replace(true);
    drop(tx);
    task.await?;
    manager.shutdown().await;
    Ok(())
}

async fn wait_mode_pending(manager: &ChannelManager) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if !manager
                .pending
                .lock()
                .map_err(|_| anyhow!("poisoned"))?
                .is_empty()
            {
                return Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await??;
    Ok(())
}
async fn choose_mode(
    manager: Arc<ChannelManager>,
    channel: Arc<MockChannel>,
    sender: &str,
    preview: &str,
    command: &str,
    strong: bool,
) -> Result<ConfirmationDecision> {
    let (_cancel, rx) = watch::channel(false);
    let confirmer = approval::ChannelConfirmer::new(
        manager.clone(),
        channel.clone(),
        &message(sender, "hi"),
        rx,
    );
    let preview = preview.to_owned();
    let task = tokio::spawn(async move {
        confirmer
            .confirm(&ConfirmationRequest::shell(&preview), &assessment(strong))
            .await
    });
    wait_mode_pending(&manager).await?;
    approval::control(&manager, &message(sender, command), channel.as_ref()).await?;
    if strong && command != "/deny" {
        wait_mode_pending(&manager).await?;
        assert!(manager.saved.lock().await.command_allowlist.is_empty());
        let id = manager
            .pending
            .lock()
            .map_err(|_| anyhow!("poisoned"))?
            .keys()
            .next()
            .context("strong pending missing")?
            .clone();
        approval::control(&manager, &message(sender, "/approve"), channel.as_ref()).await?;
        assert!(!task.is_finished());
        approval::control(
            &manager,
            &message(sender, &format!("/approve {id} CONFIRM {id}")),
            channel.as_ref(),
        )
        .await?;
    }
    Ok(tokio::time::timeout(Duration::from_secs(2), task).await???)
}

#[tokio::test]
async fn bare_approval_and_denial_consume_only_the_current_operation() -> Result<()> {
    let (_dir, manager) = manager().await?;
    let channel = Arc::new(MockChannel::default());
    for command in ["/approve", "/approve", "/deny"] {
        assert_eq!(
            choose_mode(
                manager.clone(),
                channel.clone(),
                "alice",
                "touch /tmp/a",
                command,
                false
            )
            .await?,
            if command == "/deny" {
                ConfirmationDecision::Reject
            } else {
                ConfirmationDecision::Approve
            }
        );
        assert!(manager
            .pending
            .lock()
            .map_err(|_| anyhow!("poisoned"))?
            .is_empty());
        assert!(manager.saved.lock().await.command_allowlist.is_empty());
        assert!(manager
            .session_grants
            .lock()
            .map_err(|_| anyhow!("poisoned"))?
            .is_empty());
    }
    manager.shutdown().await;
    Ok(())
}
#[tokio::test]
async fn session_grants_cover_different_ordinary_operations_and_expire_on_new_or_restart(
) -> Result<()> {
    let (_dir, manager) = manager().await?;
    let channel = Arc::new(MockChannel::default());
    assert_eq!(
        choose_mode(
            manager.clone(),
            channel.clone(),
            "alice",
            "touch /tmp/a",
            "/approve session",
            false
        )
        .await?,
        ConfirmationDecision::Approve
    );
    let (_cancel, rx) = watch::channel(false);
    let confirmer = approval::ChannelConfirmer::new(
        manager.clone(),
        channel.clone(),
        &message("alice", "hi"),
        rx.clone(),
    );
    assert!(
        matches!(confirmer.confirm(&ConfirmationRequest::shell("touch /tmp/a"),&assessment(false)).await?,ConfirmationDecision::ApproveByGrant(id) if id.starts_with("channel:session:"))
    );
    let other = approval::ChannelConfirmer::new(
        manager.clone(),
        channel.clone(),
        &message("bob", "hi"),
        rx,
    );
    let task = tokio::spawn(async move {
        other
            .confirm(
                &ConfirmationRequest::shell("touch /tmp/a"),
                &assessment(false),
            )
            .await
    });
    wait_mode_pending(&manager).await?;
    assert!(!task.is_finished());
    approval::control(&manager, &message("alice", "/approve"), channel.as_ref()).await?;
    assert!(!task.is_finished());
    approval::control(&manager, &message("bob", "/deny"), channel.as_ref()).await?;
    assert_eq!(task.await??, ConfirmationDecision::Reject);
    assert!(matches!(
        confirmer
            .confirm(
                &ConfirmationRequest {
                    preview: "Send a different file with a new digest",
                    tool: Some("send_channel_file"),
                    package: None,
                    media_scope: None,
                },
                &assessment(false)
            )
            .await?,
        ConfirmationDecision::ApproveByGrant(_)
    ));
    let mut dangerous = assessment(true);
    dangerous.requires_double_confirmation = true;
    let pending_confirmer = confirmer.clone();
    let pending = tokio::spawn(async move {
        pending_confirmer
            .confirm(
                &ConfirmationRequest::shell("dangerous other action"),
                &dangerous,
            )
            .await
    });
    wait_mode_pending(&manager).await?;
    approval::control(&manager, &message("alice", "/deny"), channel.as_ref()).await?;
    assert_eq!(pending.await??, ConfirmationDecision::Reject);
    let mut root = assessment(false);
    root.requires_root = true;
    let root_confirmer = confirmer.clone();
    let pending = tokio::spawn(async move {
        root_confirmer
            .confirm(&ConfirmationRequest::shell("root action"), &root)
            .await
    });
    wait_mode_pending(&manager).await?;
    approval::control(&manager, &message("alice", "/deny"), channel.as_ref()).await?;
    assert_eq!(pending.await??, ConfirmationDecision::Reject);
    let (tx, recv) = mpsc::channel(2);
    tx.send(message("alice", "/new")).await?;
    drop(tx);
    let (_worker_cancel, worker_rx) = watch::channel(false);
    worker(manager.clone(), channel.clone(), recv, worker_rx).await;
    assert!(manager
        .session_grants
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .is_empty());
    let config = manager.path.clone();
    manager.shutdown().await;
    drop(confirmer);
    drop(manager);
    let restarted = ChannelManager::start(config).await?;
    assert!(restarted
        .session_grants
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .is_empty());
    restarted.shutdown().await;
    Ok(())
}
#[tokio::test]
async fn permanent_patterns_survive_restart_and_strong_confirmation_precedes_storage() -> Result<()>
{
    let (_dir, manager) = manager().await?;
    let channel = Arc::new(MockChannel::default());
    assert_eq!(
        choose_mode(
            manager.clone(),
            channel.clone(),
            "alice",
            "dangerous exact command",
            "/approve always",
            true
        )
        .await?,
        ConfirmationDecision::Approve
    );
    assert_eq!(manager.saved.lock().await.command_allowlist.len(), 1);
    let config = manager.path.clone();
    manager.shutdown().await;
    drop(manager);
    let restarted = ChannelManager::start(config).await?;
    let (_cancel, rx) = watch::channel(false);
    let mut incoming = message("bob", "hi");
    incoming.route.kind = "weixin".into();
    incoming.route.peer = "different".into();
    let confirmer =
        approval::ChannelConfirmer::new(restarted.clone(), channel.clone(), &incoming, rx);
    assert!(
        matches!(confirmer.confirm(&ConfirmationRequest::shell("dangerous exact command"),&assessment(true)).await?,ConfirmationDecision::ApproveByGrant(id) if id.starts_with("channel:always:"))
    );
    assert_eq!(
        choose_mode(
            restarted.clone(),
            channel.clone(),
            "alice",
            "another dangerous command",
            "/deny",
            true
        )
        .await?,
        ConfirmationDecision::Reject
    );
    restarted.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn bare_decisions_refuse_ambiguous_pending_and_risk_escalation_reprompts() -> Result<()> {
    let (_dir, manager) = manager().await?;
    let channel = Arc::new(MockChannel::default());
    let (_cancel, rx) = watch::channel(false);
    let mut tasks = Vec::new();
    for cmd in ["touch /tmp/a", "touch /tmp/b"] {
        let confirmer = approval::ChannelConfirmer::new(
            manager.clone(),
            channel.clone(),
            &message("alice", "hi"),
            rx.clone(),
        );
        tasks.push(tokio::spawn(async move {
            confirmer
                .confirm(&ConfirmationRequest::shell(cmd), &assessment(false))
                .await
        }));
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while manager.pending.lock().map(|p| p.len()).unwrap_or_default() != 2 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?;
    for cmd in ["/approve", "/approve session", "/approve always", "/deny"] {
        approval::control(&manager, &message("alice", cmd), channel.as_ref()).await?;
        assert_eq!(
            manager
                .pending
                .lock()
                .map_err(|_| anyhow!("poisoned"))?
                .len(),
            2
        );
    }
    let ids: Vec<_> = manager
        .pending
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .keys()
        .cloned()
        .collect();
    for id in ids {
        approval::control(
            &manager,
            &message("alice", &format!("/deny {id}")),
            channel.as_ref(),
        )
        .await?;
    }
    for task in tasks {
        assert_eq!(task.await??, ConfirmationDecision::Reject);
    }
    choose_mode(
        manager.clone(),
        channel.clone(),
        "alice",
        "touch /tmp/a",
        "/approve session",
        false,
    )
    .await?;
    assert_eq!(
        choose_mode(
            manager.clone(),
            channel.clone(),
            "alice",
            "touch /tmp/a",
            "/deny",
            true
        )
        .await?,
        ConfirmationDecision::Reject
    );
    manager.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn denied_action_alias_does_not_reprompt_until_next_user_task() -> Result<()> {
    let (_dir, manager) = manager().await?;
    let channel = Arc::new(MockChannel::default());
    let (_cancel, rx) = watch::channel(false);
    let confirmer = approval::ChannelConfirmer::new(
        manager.clone(),
        channel.clone(),
        &message("alice", "capture"),
        rx.clone(),
    );
    let first = confirmer.clone();
    let task = tokio::spawn(async move {
        first
            .confirm(
                &ConfirmationRequest {
                    preview: "screencap -p /data/local/tmp/screen.png",
                    tool: Some("android.screenshot"),
                    package: None,
                    media_scope: None,
                },
                &assessment(false),
            )
            .await
    });
    wait_mode_pending(&manager).await?;
    approval::control(&manager, &message("alice", "/deny"), channel.as_ref()).await?;
    assert_eq!(task.await??, ConfirmationDecision::Reject);
    let count = channel.sent.lock().map_err(|_| anyhow!("poisoned"))?.len();
    assert_eq!(
        confirmer
            .confirm(
                &ConfirmationRequest {
                    preview: "screencap -p /data/local/tmp/screen.png",
                    tool: Some("capture_android_screen"),
                    package: None,
                    media_scope: None,
                },
                &assessment(false)
            )
            .await?,
        ConfirmationDecision::Reject
    );
    assert_eq!(
        count,
        channel.sent.lock().map_err(|_| anyhow!("poisoned"))?.len()
    );
    let next = approval::ChannelConfirmer::new(
        manager.clone(),
        channel.clone(),
        &message("alice", "try again"),
        rx,
    );
    let task = tokio::spawn(async move {
        next.confirm(
            &ConfirmationRequest::shell("screencap -p /data/local/tmp/screen.png"),
            &assessment(false),
        )
        .await
    });
    wait_mode_pending(&manager).await?;
    approval::control(&manager, &message("alice", "/approve"), channel.as_ref()).await?;
    assert_eq!(task.await??, ConfirmationDecision::Approve);
    manager.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn tool_attachment_references_are_task_scoped_and_bounded_for_all_media() -> Result<()> {
    use crate::agent::FileDelivery;
    use base64::Engine;
    let (_dir, manager) = manager().await?;
    let channel = Arc::new(MockChannel::default());
    let (_cancel, rx) = watch::channel(false);
    let first = approval::ChannelConfirmer::new(
        manager.clone(),
        channel.clone(),
        &message("alice", "inspect"),
        rx.clone(),
    );
    let next = approval::ChannelConfirmer::new(
        manager.clone(),
        channel.clone(),
        &message("alice", "next task"),
        rx.clone(),
    );
    let other = approval::ChannelConfirmer::new(
        manager.clone(),
        channel.clone(),
        &message("bob", "other sender"),
        rx,
    );
    for (mime, name) in [
        ("image/png", "attachment.png"),
        ("audio/wav", "attachment.wav"),
        ("video/mp4", "attachment.mp4"),
        ("application/octet-stream", "attachment.bin"),
    ] {
        let attachment = crate::llm::ToolAttachment {
            media_type: mime.into(),
            base64_data: base64::engine::general_purpose::STANDARD.encode(b"immutable bytes"),
        };
        let id = first
            .stage_attachment(&attachment)
            .await?
            .context("reference missing")?;
        assert_eq!(
            first.clone().attachment(&id)?,
            (name.into(), b"immutable bytes".to_vec())
        );
        assert!(next.attachment(&id).is_err());
        assert!(other.attachment(&id).is_err());
        assert!(first.attachment("invented-id").is_err());
    }
    let attachment = crate::llm::ToolAttachment {
        media_type: "image/png".into(),
        base64_data: base64::engine::general_purpose::STANDARD.encode(b"bytes"),
    };
    for _ in 0..4 {
        first.stage_attachment(&attachment).await?;
    }
    assert!(first.stage_attachment(&attachment).await.is_err());
    manager.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn approvals_use_only_configured_language_defaulting_to_chinese() -> Result<()> {
    for (language, expected, excluded) in [
        (
            crate::config::UiLanguage::ZhCn,
            "操作审批",
            "Operation approval",
        ),
        (
            crate::config::UiLanguage::En,
            "Operation approval",
            "操作审批",
        ),
    ] {
        let (_dir, manager) = manager().await?;
        let mut cfg = crate::config::load_or_default_unvalidated(&manager.path)?;
        cfg.ui_language = language;
        crate::config::save_config(&manager.path, &cfg)?;
        let channel = Arc::new(MockChannel::default());
        let (_cancel, rx) = watch::channel(false);
        let confirmer = approval::ChannelConfirmer::new(
            manager.clone(),
            channel.clone(),
            &message("alice", "hi"),
            rx,
        );
        let task = tokio::spawn(async move {
            confirmer
                .confirm(
                    &ConfirmationRequest::shell("touch example"),
                    &assessment(true),
                )
                .await
        });
        wait_mode_pending(&manager).await?;
        tokio::time::timeout(Duration::from_secs(2), channel.changed.notified()).await?;
        let prompt = channel
            .sent
            .lock()
            .map_err(|_| anyhow!("poisoned"))?
            .last()
            .cloned()
            .context("missing prompt")?;
        assert!(prompt.contains(expected));
        assert!(!prompt.contains(excluded));
        assert!(!prompt.contains(" / once"));
        approval::control(&manager, &message("alice", "/approve"), channel.as_ref()).await?;
        wait_mode_pending(&manager).await?;
        tokio::time::timeout(Duration::from_secs(2), channel.changed.notified()).await?;
        let prompt = channel
            .sent
            .lock()
            .map_err(|_| anyhow!("poisoned"))?
            .last()
            .cloned()
            .context("missing prompt")?;
        assert!(
            prompt.contains(if language == crate::config::UiLanguage::ZhCn {
                "第二次确认"
            } else {
                "Second confirmation"
            })
        );
        assert!(!prompt.contains(excluded));
        assert!(prompt.contains("```\n/approve "));
        assert!(prompt.contains(" CONFIRM "));
        assert_eq!(
            *channel.formats.lock().map_err(|_| anyhow!("poisoned"))?,
            vec!["markdown", "markdown"]
        );
        approval::control(&manager, &message("alice", "/deny"), channel.as_ref()).await?;
        assert_eq!(task.await??, ConfirmationDecision::Reject);
        manager.shutdown().await;
    }
    let (_dir, manager) = manager().await?;
    std::fs::write(&manager.path, "")?;
    assert_eq!(manager.language().await, crate::config::UiLanguage::ZhCn);
    manager.shutdown().await;
    Ok(())
}

struct ToolDecision(bool);
#[async_trait]
impl Confirmer for ToolDecision {
    async fn confirm(
        &self,
        _: &ConfirmationRequest<'_>,
        _: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        Ok(if self.0 {
            ConfirmationDecision::Approve
        } else {
            ConfirmationDecision::Reject
        })
    }
}
#[tokio::test]
async fn shared_tools_and_same_uid_ipc_deliver_only_known_targets_after_caller_approval(
) -> Result<()> {
    use crate::shell::ShellExecutor;
    let (dir, manager) = manager().await?;
    let channel = Arc::new(MockChannel::default());
    let incoming = message("alice", "hello");
    manager.saved.lock().await.qq = Some(Account {
        id: "1".into(),
        secret: "secret".into(),
        base_url: "https://api.bot.qq.com".into(),
        enabled: true,
    });
    manager
        .adapters
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .insert("qq".into(), channel.clone());
    manager.remember_target(&incoming)?;
    let targets = control::targets(&manager.path).await?;
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].markdown, "native");
    let json = serde_json::to_string(&targets)?;
    assert!(!json.contains("context"));
    assert!(!json.contains("secret"));
    let cfg = crate::config::load_or_default_unvalidated(&manager.path)?;
    let executor = ShellExecutor::new(cfg.clone());
    let id = &targets[0].id;
    manager
        .session_grants
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .insert(incoming.route.key(), ());
    let result = crate::tools::runtime::invoke(
        &cfg,
        &executor,
        &ToolDecision(false),
        "send_channel_message",
        json!({"target":id,"kind":"text","text":"denied"}),
    )
    .await?;
    assert!(!result.success);
    assert!(channel
        .sent
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .is_empty());
    let result = crate::tools::runtime::invoke(
        &cfg,
        &executor,
        &ToolDecision(true),
        "send_channel_message",
        json!({"target":id,"kind":"markdown","text":"**diagnosis**"}),
    )
    .await?;
    assert!(result.success);
    for (kind, name) in [
        ("image", "test.png"),
        ("audio", "test.wav"),
        ("video", "test.mp4"),
        ("file", "test.bin"),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, b"immutable data")?;
        let result = crate::tools::runtime::invoke(
            &cfg,
            &executor,
            &ToolDecision(true),
            "send_channel_message",
            json!({"target":id,"kind":kind,"path":path}),
        )
        .await?;
        assert!(result.success);
    }
    assert_eq!(
        channel.files.lock().map_err(|_| anyhow!("poisoned"))?.len(),
        4
    );
    let receipt = control::deliver(
        &manager.path,
        outbound::Outbound {
            target: id.clone(),
            kind: "text".into(),
            text: "through IPC".into(),
            name: String::new(),
            data: None,
            buttons: Vec::new(),
        },
    )
    .await?;
    assert_eq!(receipt["status"], "submitted");
    assert!(control::deliver(
        &manager.path,
        outbound::Outbound {
            target: uuid::Uuid::new_v4().to_string(),
            kind: "text".into(),
            text: "unknown".into(),
            name: String::new(),
            data: None,
            buttons: Vec::new()
        }
    )
    .await
    .is_err());
    assert!(crate::tools::runtime::invoke(
        &cfg,
        &executor,
        &ToolDecision(true),
        "send_channel_message",
        json!({"kind":"text","text":"no target"})
    )
    .await
    .is_err());
    assert!(crate::tools::runtime::invoke(
        &cfg,
        &executor,
        &ToolDecision(true),
        "send_channel_file",
        json!({"target":id,"attachment_id":uuid::Uuid::new_v4().to_string()})
    )
    .await
    .is_err());
    manager.stop_job("qq").await;
    assert!(manager.targets().await?.is_empty());
    assert!(outbound::delivery(&manager.path, id).await.is_err());
    manager.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn callback_cannot_skip_strong_confirmation_or_use_expired_or_cross_route_requests(
) -> Result<()> {
    let (_dir, manager) = manager().await?;
    let channel = Arc::new(MockChannel::default());
    let (cancel, rx) = watch::channel(false);
    let original = message("alice", "diagnose");
    let confirmer =
        approval::ChannelConfirmer::new(manager.clone(), channel.clone(), &original, rx);
    let task = tokio::spawn(async move {
        confirmer
            .confirm(
                &ConfirmationRequest::shell("dangerous callback operation"),
                &assessment(true),
            )
            .await
    });
    wait_mode_pending(&manager).await?;
    tokio::time::timeout(Duration::from_secs(2), channel.changed.notified()).await?;
    let id = manager
        .pending
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .keys()
        .next()
        .cloned()
        .context("missing pending")?;
    let mut wrong_account = original.route.clone();
    wrong_account.account = "other".into();
    assert!(!approval::button(&manager, &wrong_account, &id, "once")?);
    let mut wrong_sender = original.route.clone();
    wrong_sender.sender = "bob".into();
    assert!(!approval::button(&manager, &wrong_sender, &id, "once")?);
    let mut wrong_peer = original.route.clone();
    wrong_peer.peer = "other".into();
    assert!(!approval::button(&manager, &wrong_peer, &id, "once")?);
    assert!(approval::button(&manager, &original.route, &id, "session")?);
    wait_mode_pending(&manager).await?;
    tokio::time::timeout(Duration::from_secs(2), channel.changed.notified()).await?;
    let strong = manager
        .pending
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .keys()
        .next()
        .cloned()
        .context("missing strong pending")?;
    for mode in ["once", "session", "always", "deny"] {
        assert!(!approval::button(&manager, &original.route, &strong, mode)?);
    }
    assert!(manager
        .session_grants
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .is_empty());
    manager
        .pending
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .get_mut(&strong)
        .context("missing pending")?
        .expires = std::time::Instant::now() - Duration::from_secs(1);
    assert!(!approval::button(
        &manager,
        &original.route,
        &strong,
        "once"
    )?);
    cancel.send_replace(true);
    assert_eq!(task.await??, ConfirmationDecision::Reject);
    manager.shutdown().await;
    Ok(())
}

#[test]
fn generic_buttons_cannot_carry_approval_commands_or_exceed_platform_limits() {
    for command in [
        "/approve",
        "/approve session",
        " /deny id",
        "/answer id yes",
    ] {
        assert!(outbound::validate_buttons(&[outbound::MessageButton {
            label: "Continue".into(),
            command: command.into()
        }])
        .is_err());
    }
    assert!(outbound::validate_buttons(&[outbound::MessageButton {
        label: "Continue".into(),
        command: "Inspect memory".into()
    }])
    .is_ok());
    assert!(outbound::validate_buttons(&[outbound::MessageButton {
        label: "Too long label".into(),
        command: "Inspect".into()
    }])
    .is_err());
}

#[tokio::test]
async fn media_always_uses_stable_scope_survives_restart_and_keeps_boundaries() -> Result<()> {
    use crate::agent::MediaApprovalScope;
    let (_dir, manager) = manager().await?;
    let channel = Arc::new(MockChannel::default());
    let scope = MediaApprovalScope {
        destination: "weixin / account / peer (sender alice)".into(),
        kind: "image".into(),
    };
    let (_cancel, rx) = watch::channel(false);
    let confirmer = approval::ChannelConfirmer::new(
        manager.clone(),
        channel.clone(),
        &message("alice", "send"),
        rx.clone(),
    );
    let first = confirmer.clone();
    let first_scope = scope.clone();
    let task = tokio::spawn(async move {
        first
            .confirm(
                &ConfirmationRequest {
                    preview: "image A: path A, digest A, caption A",
                    tool: Some("send_channel_file"),
                    package: None,
                    media_scope: Some(first_scope),
                },
                &assessment(false),
            )
            .await
    });
    wait_mode_pending(&manager).await?;
    tokio::time::timeout(Duration::from_secs(2), channel.changed.notified()).await?;
    assert!(channel
        .sent
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .last()
        .context("prompt")?
        .contains("不限文件内容、路径及说明"));
    approval::control(
        &manager,
        &message("alice", "/approve always"),
        channel.as_ref(),
    )
    .await?;
    assert_eq!(task.await??, ConfirmationDecision::Approve);
    assert!(channel
        .sent
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .iter()
        .any(|text| text.contains("已保存永久授权")));
    let path = manager.path.clone();
    manager.shutdown().await;
    drop(confirmer);
    drop(manager);
    let restarted = ChannelManager::start(path).await?;
    let confirmer = approval::ChannelConfirmer::new(
        restarted.clone(),
        channel.clone(),
        &message("alice", "send"),
        rx.clone(),
    );
    assert!(
        matches!(confirmer.confirm(&ConfirmationRequest { preview: "image B: new path, size, digest and caption", tool: Some("send_channel_file"), package: None, media_scope: Some(scope.clone()) }, &assessment(false)).await?, ConfirmationDecision::ApproveByGrant(id) if id.starts_with("channel:always:"))
    );
    for case in 0..6 {
        let mut changed = scope.clone();
        if case == 0 {
            changed.kind = "audio".into();
        }
        if case == 1 {
            changed.destination = "another destination".into();
        }
        let sender = if case == 2 { "bob" } else { "alice" };
        let confirmer = approval::ChannelConfirmer::new(
            restarted.clone(),
            channel.clone(),
            &message(sender, "send"),
            rx.clone(),
        );
        let tool = if case == 3 {
            "execute_shell_command"
        } else {
            "send_channel_file"
        };
        let mut risk = assessment(case == 4);
        if case == 5 {
            risk.requires_root = true;
        }
        let task = tokio::spawn(async move {
            confirmer
                .confirm(
                    &ConfirmationRequest {
                        preview: "image B: new path, size, digest and caption",
                        tool: Some(tool),
                        package: None,
                        media_scope: Some(changed),
                    },
                    &risk,
                )
                .await
        });
        wait_mode_pending(&restarted).await?;
        assert!(!task.is_finished());
        approval::control(&restarted, &message(sender, "/deny"), channel.as_ref()).await?;
        assert_eq!(task.await??, ConfirmationDecision::Reject);
    }
    restarted.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn media_session_grant_ignores_snapshot_changes_and_covers_all_ordinary_kinds() -> Result<()>
{
    let (_dir, manager) = manager().await?;
    let channel = Arc::new(MockChannel::default());
    assert_eq!(
        choose_mode(
            manager.clone(),
            channel.clone(),
            "alice",
            "first ordinary operation",
            "/approve session",
            false
        )
        .await?,
        ConfirmationDecision::Approve
    );
    assert!(channel
        .sent
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .iter()
        .any(|text| text.contains("已允许本会话全部普通操作")));
    let (_cancel, rx) = watch::channel(false);
    let confirmer = approval::ChannelConfirmer::new(
        manager.clone(),
        channel.clone(),
        &message("alice", "send"),
        rx,
    );
    for kind in ["image", "audio", "video", "file"] {
        for preview in [
            "original snapshot",
            "different path, bytes, digest, size and caption",
        ] {
            assert!(
                matches!(confirmer.confirm(&ConfirmationRequest { preview, tool: Some("send_channel_file"), package: None, media_scope: Some(crate::agent::MediaApprovalScope { destination: "same target".into(), kind: kind.into() }) }, &assessment(false)).await?, ConfirmationDecision::ApproveByGrant(id) if id.starts_with("channel:session:"))
            );
        }
    }
    manager.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn legacy_media_permanent_grant_stays_exact_after_scope_upgrade() -> Result<()> {
    let (_dir, manager) = manager().await?;
    let channel = Arc::new(MockChannel::default());
    let (_cancel, rx) = watch::channel(false);
    let confirmer = approval::ChannelConfirmer::new(
        manager.clone(),
        channel.clone(),
        &message("alice", "send"),
        rx,
    );
    let first = confirmer.clone();
    let task = tokio::spawn(async move {
        first
            .confirm(
                &ConfirmationRequest {
                    preview: "original snapshot",
                    tool: Some("send_channel_file"),
                    package: None,
                    media_scope: None,
                },
                &assessment(false),
            )
            .await
    });
    wait_mode_pending(&manager).await?;
    approval::control(
        &manager,
        &message("alice", "/approve always"),
        channel.as_ref(),
    )
    .await?;
    assert_eq!(task.await??, ConfirmationDecision::Approve);
    let scope = crate::agent::MediaApprovalScope {
        destination: "target".into(),
        kind: "image".into(),
    };
    assert!(matches!(
        confirmer
            .confirm(
                &ConfirmationRequest {
                    preview: "original snapshot",
                    tool: Some("send_channel_file"),
                    package: None,
                    media_scope: Some(scope.clone())
                },
                &assessment(false)
            )
            .await?,
        ConfirmationDecision::ApproveByGrant(_)
    ));
    let task = tokio::spawn(async move {
        confirmer
            .confirm(
                &ConfirmationRequest {
                    preview: "changed snapshot",
                    tool: Some("send_channel_file"),
                    package: None,
                    media_scope: Some(scope),
                },
                &assessment(false),
            )
            .await
    });
    wait_mode_pending(&manager).await?;
    assert!(!task.is_finished());
    approval::control(&manager, &message("alice", "/deny"), channel.as_ref()).await?;
    assert_eq!(task.await??, ConfirmationDecision::Reject);
    manager.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn approval_without_pending_explains_workflow_without_mutating_policy() -> Result<()> {
    let (_dir, manager) = manager().await?;
    let channel = MockChannel::default();
    let before = std::fs::read(&manager.path)?;
    assert!(approval::control(&manager, &message("alice", "/approve always"), &channel).await?);
    assert!(channel
        .sent
        .lock()
        .map_err(|_| anyhow!("poisoned"))?
        .last()
        .context("reply")?
        .contains("请等待审批消息后单独回复"));
    assert_eq!(std::fs::read(&manager.path)?, before);
    assert!(manager.saved.lock().await.command_allowlist.is_empty());
    manager.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn owner_can_revoke_one_permanent_approval_without_overwriting_other_state() -> Result<()> {
    let (_dir, manager) = manager().await?;
    let channel = Arc::new(MockChannel::default());
    for command in ["touch /tmp/a", "touch /tmp/b"] {
        assert_eq!(
            choose_mode(
                manager.clone(),
                channel.clone(),
                "alice",
                command,
                "/approve always",
                false
            )
            .await?,
            ConfirmationDecision::Approve
        );
    }
    let id = manager.saved.lock().await.command_allowlist[0].id();
    let other = manager.saved.lock().await.command_allowlist[1].id();
    control::apply(
        &manager.path,
        &ChannelAction::RevokePermanentApproval { id: id.clone() },
    )
    .await?;
    assert!(control::apply(
        &manager.path,
        &ChannelAction::RevokePermanentApproval { id }
    )
    .await
    .is_err());
    assert_eq!(manager.saved.lock().await.command_allowlist.len(), 1);
    assert_eq!(manager.saved.lock().await.command_allowlist[0].id(), other);
    let path = manager.path.clone();
    manager.shutdown().await;
    drop(manager);
    let restarted = ChannelManager::start(path).await?;
    assert_eq!(restarted.saved.lock().await.command_allowlist.len(), 1);
    assert_eq!(
        restarted.saved.lock().await.command_allowlist[0].id(),
        other
    );
    restarted.shutdown().await;
    Ok(())
}
