use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ApiType;
    use crate::llm::{FinishReason, LlmResponse, ToolCall, ToolResult, Usage};
    use std::io::Read;
    use tempfile::tempdir;
    use wiremock::{
        matchers::{body_string_contains, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    async fn update_status_survives_browser_reconnect_and_rejects_untrusted_requests() -> Result<()>
    {
        let directory = tempdir()?;
        let current = state(directory.path().join("config.toml"));
        {
            let mut job = current.update.lock().map_err(|_| anyhow!("poisoned"))?;
            job.busy = true;
            job.version = Some("1.2.0".into());
            job.progress = Some(crate::update::UpdateProgress {
                stage: "downloading",
                downloaded: 25,
                total: 100,
            });
        }
        let Json(job) = update_progress(State(current.clone()))
            .await
            .map_err(|error| error.error)?;
        assert!(job.busy);
        assert_eq!(job.progress.context("missing progress")?.downloaded, 25);
        assert!(serde_json::from_value::<UpdateStart>(serde_json::json!({
            "version": "1.2.0", "url": "https://example.com/untrusted"
        }))
        .is_err());
        assert!(start_update(
            State(current.clone()),
            Json(UpdateStart {
                version: "../untrusted".into()
            })
        )
        .await
        .is_err());
        #[cfg(not(target_os = "android"))]
        assert!(start_update(
            State(current),
            Json(UpdateStart {
                version: "1.2.0".into()
            })
        )
        .await
        .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn browser_files_expose_metadata_and_bounded_inline_preview() -> Result<()> {
        let directory = tempdir()?;
        let text_path = directory.path().join("example.rs");
        std::fs::write(&text_path, b"hello browser")?;
        std::fs::create_dir(directory.path().join("folder"))?;
        let Json(files) = get_files(Query(FilePathQuery {
            path: directory.path().to_string_lossy().into_owned(),
        }))
        .await
        .map_err(|error| error.error)?;
        assert_eq!(files.len(), 2);
        assert!(files[0].is_dir);
        assert_eq!(files[1].preview_kind, Some("text"));
        assert_eq!(files[1].size, Some(13));
        assert!(files[1].modified_ms.is_some());

        let query = FilePathQuery {
            path: text_path.to_string_lossy().into_owned(),
        };
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(header::RANGE, HeaderValue::from_static("bytes=6-12"));
        let response = get_file_preview(Query(query), headers)
            .await
            .map_err(|error| error.error)?;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 6-12/13");
        let body = axum::body::to_bytes(response.into_body(), 100).await?;
        assert_eq!(&body[..], b"browser");
        assert_eq!(requested_range("bytes=-4", 13), Some((9, 12)));
        assert_eq!(requested_range("bytes=99-", 13), None);
        assert!(preview_type(Path::new("secret.bin")).is_none());
        assert_eq!(
            preview_type(Path::new("sound.wav")).map(|value| value.0),
            Some("audio")
        );
        assert_eq!(
            preview_type(Path::new("samples.pcm")).map(|value| value.0),
            Some("audio")
        );
        Ok(())
    }

    #[test]
    fn task_timing_separates_model_tools_and_waits_and_freezes_on_finish() {
        let mut inner = SessionState::empty();
        inner.task_started = Some(Instant::now() - std::time::Duration::from_secs(10));
        inner.activity = "thinking";
        inner.phase_started = Instant::now() - std::time::Duration::from_secs(2);
        set_activity(&mut inner, "tool", Some("read_file"));
        inner.phase_started = Instant::now() - std::time::Duration::from_secs(3);
        set_activity(&mut inner, "waiting", None);
        inner.phase_started = Instant::now() - std::time::Duration::from_secs(4);
        finish_task(&mut inner);
        let task = task_metrics(&inner);
        assert!((2000..2200).contains(&task.model_ms));
        assert!((3000..3200).contains(&task.tool_ms));
        assert!((4000..4200).contains(&task.waiting_ms));
        assert!((10000..10200).contains(&task.total_ms));
        inner.phase_started = Instant::now() - std::time::Duration::from_secs(500);
        assert_eq!(task_metrics(&inner).total_ms, task.total_ms);
        assert_eq!(task_metrics(&inner).waiting_ms, task.waiting_ms);
    }

    #[tokio::test]
    async fn live_progress_and_reasoning_survive_failed_task_restart_without_model_history(
    ) -> Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let shared = state(path.clone());
        let current = session(&shared, "web-test")?;
        {
            let mut inner = current.inner.lock().map_err(|_| anyhow!("lock"))?;
            inner.busy = true;
            inner.accepted_turns = 2;
            inner.token_base = (10, 20);
            inner.task_started = Some(Instant::now());
            inner.checkpoint_start = Some(0);
            inner.redaction_secrets = vec!["private-test-key".into()];
            push(&mut inner, "> 原任务".into());
            inner.checkpoint = Some(WebCheckpoint {
                status: "failed".into(),
                activity: "thinking".into(),
                history: Vec::new(),
                events: Vec::new(),
            });
        }
        let sink = WebTextSink {
            session: current.clone(),
            state: None,
        };
        sink.agent_progress(
            3,
            2,
            &Usage {
                input_tokens: Some(100),
                output_tokens: Some(40),
            },
        );
        sink.reasoning_delta("检查 private-test-key");
        sink.reasoning_delta(" 的结果");
        sink.delta("中间说明");
        sink.tool_started("call", "read_file");
        sink.tool_finished("call", "失败证据", false);
        let snapshot = get_state(
            State(shared.clone()),
            Query(StateQuery {
                id: "web-test".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?
        .0;
        assert_eq!(
            (snapshot.turns, snapshot.steps, snapshot.tool_calls),
            (2, 3, 2)
        );
        assert_eq!((snapshot.input_tokens, snapshot.output_tokens), (110, 60));
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.kind == "reasoning"));
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.kind == "assistant" && entry.text == "中间说明"));
        {
            let mut inner = current.inner.lock().map_err(|_| anyhow!("lock"))?;
            finish_task(&mut inner);
            inner.busy = false;
        }
        persist_web_session(&shared, &current).await?;
        let raw = std::fs::read_to_string(directory.path().join("sessions/web-test.json"))?;
        assert!(!raw.contains("private-test-key"));
        let restored = state(path.clone());
        restored
            .sessions
            .lock()
            .map_err(|_| anyhow!("lock"))?
            .clear();
        let _ = load_session(
            State(restored.clone()),
            Json(SessionSelection {
                name: "web-test".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        let snapshot = get_state(
            State(restored),
            Query(StateQuery {
                id: "web-test".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?
        .0;
        assert_eq!(
            (snapshot.turns, snapshot.steps, snapshot.tool_calls),
            (2, 3, 2)
        );
        assert_eq!((snapshot.input_tokens, snapshot.output_tokens), (110, 60));
        assert!(snapshot.can_retry);
        assert!(!snapshot.busy);
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.kind == "reasoning"));
        assert!(SessionStore::open(&path)?
            .load("web-test", 10, 1024)?
            .is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn web_audit_identifies_sessions_and_redacts_model_and_tool_content() -> Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let shared = state(path.clone());
        let current = session(&shared, "web-test")?;
        let cfg = Config {
            api_key: "private-test-key".into(),
            ..Config::default()
        };
        {
            let mut inner = current.inner.lock().map_err(|_| anyhow!("lock"))?;
            remember_redaction_secrets(&mut inner, &cfg);
        }
        start_audit(path.clone(), &cfg, &current).await?;
        audit(&current, "web_user", "任务 private-test-key");
        let sink = WebTextSink {
            session: current.clone(),
            state: None,
        };
        sink.reasoning_delta("思考 private-test-key");
        sink.delta("说明");
        sink.end(true);
        sink.tool_requested(&ToolCall {
            id: "call".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({"path":"private-test-key"}),
        });
        sink.tool_started("call", "read_file");
        sink.tool_finished("call", "private-test-key 结果", true);
        flush_audit(&current).await;
        let raw = std::fs::read_to_string(directory.path().join(&cfg.history_log_file))?;
        assert!(!raw.contains("private-test-key"));
        assert!(raw.contains("CREDENTIAL REDACTED"));
        for line in raw.lines() {
            let record: serde_json::Value = serde_json::from_str(line)?;
            let payload: serde_json::Value =
                serde_json::from_str(record["message"].as_str().context("message")?)?;
            assert_eq!(payload["session_id"], "web-test");
        }
        assert!(raw.contains("web_tool_requested"));
        assert!(raw.contains("web_tool_finished"));
        assert!(raw.contains("思考"));
        Ok(())
    }

    #[tokio::test]
    async fn model_check_uses_real_provider_request_without_a_session() -> Result<()> {
        let provider = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "OK"}, "finish_reason": "stop"}],
                "usage": {}
            })))
            .expect(1)
            .mount(&provider)
            .await;
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let cfg = Config {
            endpoint: format!("{}/v1", provider.uri()),
            api_key: "test-key".into(),
            model: "test-model".into(),
            api_type: ApiType::ChatCompletions,
            ..Config::default()
        };
        config::save_config(&path, &cfg)?;
        let shared = state(path);
        let Json(result) = check_model(State(shared.clone()))
            .await
            .map_err(|error| error.error)?;
        assert!(result.ok);
        assert_eq!(
            shared
                .sessions
                .lock()
                .map_err(|_| anyhow!("web lock poisoned"))?
                .len(),
            1
        );
        Ok(())
    }

    #[tokio::test]
    async fn device_overview_works_without_model_configuration() -> Result<()> {
        let directory = tempdir()?;
        let shared = state(directory.path().join("missing-config.toml"));
        let Json(result) = get_device_overview(State(shared))
            .await
            .map_err(|error| error.error)?;
        assert_eq!(result["kind"], "android_environment");
        assert!(result["status"] == "complete" || result["status"] == "partial");
        Ok(())
    }

    #[tokio::test]
    async fn cancel_request_rejects_pending_approval_and_keeps_session_busy_until_cleanup(
    ) -> Result<()> {
        let directory = tempdir()?;
        let shared = state(directory.path().join("config.toml"));
        let current = session(&shared, "web-test")?;
        let (reply, waiting) = oneshot::channel();
        let request_id = PENDING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        {
            let mut inner = current
                .inner
                .lock()
                .map_err(|_| anyhow!("web lock poisoned"))?;
            inner.busy = true;
            inner.pending = Some(Pending::Approval {
                request_id,
                command: "touch /tmp/x".into(),
                risk: "Mutating".into(),
                explanation: "test".into(),
                root: false,
                strong: false,
                armed: false,
            });
            inner.reply = Some(reply);
        }
        cancel_message(
            State(shared),
            Json(SessionSelection {
                name: "web-test".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        assert!(*current.cancel.borrow());
        assert!(matches!(
            waiting.await?,
            Reply::Approval(ConfirmationDecision::Reject)
        ));
        let inner = current
            .inner
            .lock()
            .map_err(|_| anyhow!("web lock poisoned"))?;
        assert!(inner.busy);
        assert!(inner.pending.is_none());
        assert_eq!(inner.activity, "cancelling");
        Ok(())
    }

    #[tokio::test]
    async fn dangerous_web_approval_requires_two_explicit_decisions() -> Result<()> {
        let directory = tempdir()?;
        let shared = state(directory.path().join("config.toml"));
        let current = session(&shared, "web-test")?;
        let (reply, waiting) = oneshot::channel();
        let request_id = PENDING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        {
            let mut inner = current
                .inner
                .lock()
                .map_err(|_| anyhow!("web lock poisoned"))?;
            inner.busy = true;
            inner.pending = Some(Pending::Approval {
                request_id,
                command: "rm -rf /tmp/test".into(),
                risk: "Dangerous".into(),
                explanation: "test".into(),
                root: false,
                strong: true,
                armed: false,
            });
            inner.reply = Some(reply);
        }
        let decision = |action: &str| Decision {
            session_id: "web-test".into(),
            request_id,
            action: action.into(),
            text: None,
            answers: None,
        };
        assert!(post_decision(
            State(shared.clone()),
            Json(Decision {
                request_id: request_id.saturating_add(1),
                ..decision("arm")
            })
        )
        .await
        .is_err());
        assert!(
            post_decision(State(shared.clone()), Json(decision("approve")))
                .await
                .is_err()
        );
        post_decision(State(shared.clone()), Json(decision("arm")))
            .await
            .map_err(|error| error.error)?;
        {
            let inner = current
                .inner
                .lock()
                .map_err(|_| anyhow!("web lock poisoned"))?;
            assert!(matches!(
                inner.pending,
                Some(Pending::Approval { armed: true, .. })
            ));
            assert!(inner.reply.is_some());
        }
        post_decision(State(shared), Json(decision("approve")))
            .await
            .map_err(|error| error.error)?;
        assert!(matches!(
            waiting.await?,
            Reply::Approval(ConfirmationDecision::ApproveCaptured)
        ));
        Ok(())
    }

    #[test]
    fn approval_explanation_uses_local_rule_evidence() {
        let assessment = crate::security::assess("rm -rf /", &Config::default());
        assert_eq!(assessment.risk_level, crate::security::RiskLevel::Critical);
        assert_eq!(approval_explanation(&assessment), "检测到递归删除关键目录");
    }

    #[test]
    fn consecutive_pending_requests_have_distinct_frontend_identities() -> Result<()> {
        let first = Pending::Approval {
            request_id: PENDING_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            command: "same command".into(),
            risk: "Mutating".into(),
            explanation: "test".into(),
            root: false,
            strong: false,
            armed: false,
        };
        let second = Pending::Approval {
            request_id: PENDING_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            command: "same command".into(),
            risk: "Mutating".into(),
            explanation: "test".into(),
            root: false,
            strong: false,
            armed: false,
        };
        let first = serde_json::to_value(first)?;
        let second = serde_json::to_value(second)?;
        assert_ne!(first["request_id"], second["request_id"]);
        Ok(())
    }

    fn state(path: PathBuf) -> Arc<Shared> {
        let mut inner = SessionState::empty();
        inner.id = "web-test".into();
        let current = web_session(inner);
        Arc::new(Shared {
            update: Mutex::new(UpdateJob::default()),
            started: Instant::now(),
            port: 0,
            path,
            sessions: Mutex::new(BTreeMap::from([("web-test".into(), current)])),
        })
    }

    #[tokio::test]
    async fn memory_api_supports_crud_in_the_shared_sqlite_store() -> Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let shared = state(path.clone());

        let Json(initial) = get_memory(State(shared.clone()))
            .await
            .map_err(|error| error.error)?;
        assert!(initial.is_empty());

        let Json(created) = set_memory(
            State(shared.clone()),
            Json(MemorySetRequest {
                key: "user_name".into(),
                value: "ernest".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].key, "user_name");
        assert_eq!(created[0].value, "ernest");

        let Json(updated) = set_memory(
            State(shared.clone()),
            Json(MemorySetRequest {
                key: "user_name".into(),
                value: "Ernest".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        assert_eq!(updated[0].value, "Ernest");

        let Json(deleted) = delete_memory(
            State(shared.clone()),
            Json(MemoryKeyRequest {
                key: "user_name".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        assert!(deleted.is_empty());

        let _ = set_memory(
            State(shared.clone()),
            Json(MemorySetRequest {
                key: "language".into(),
                value: "zh-CN".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        let Json(cleared) = clear_memory(State(shared))
            .await
            .map_err(|error| error.error)?;
        assert!(cleared.is_empty());
        assert!(config::memory_dir(&path)?
            .join("agent-memory.sqlite3")
            .is_file());
        assert!(!directory.path().join(".nl2sh-agent-memory.json").exists());
        Ok(())
    }

    #[test]
    fn generated_titles_are_bounded_and_cleaned() {
        assert_eq!(
            crate::session_title::normalize_title("**\"检查网络连接\"**\nextra"),
            Some("检查网络连接".into())
        );
        assert_eq!(crate::session_title::normalize_title("   "), None);
        assert_eq!(
            crate::session_title::normalize_title(&"a".repeat(60))
                .map(|value| value.chars().count()),
            Some(48)
        );
        assert!(crate::session_title::normalize_title(&"😀".repeat(60))
            .is_some_and(|value| value.len() <= 150));
    }

    struct StaticTitle;

    #[async_trait]
    impl LlmClient for StaticTitle {
        async fn complete(&self, _: LlmRequest) -> Result<LlmResponse> {
            Ok(LlmResponse {
                text: Some("已完成任务".into()),
                tool_calls: Vec::new(),
                usage: Usage::default(),
                finish_reason: FinishReason::Stop,
            })
        }
    }

    #[tokio::test]
    async fn delayed_title_does_not_restore_a_deleted_session() -> Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let shared = state(path.clone());
        let current = session(&shared, "web-test")?;
        {
            let mut inner = current.inner.lock().map_err(|_| anyhow!("poisoned"))?;
            inner.turns.push(vec![
                ConversationItem::Message(ConversationMessage::new(Role::User, "问题")),
                ConversationItem::Message(ConversationMessage::new(Role::Assistant, "回答")),
            ]);
            inner.history = render_turns(&inner.turns);
        }
        persist_web_session(&shared, &current).await?;
        delete_session(
            State(shared.clone()),
            Json(SessionSelection {
                name: "web-test".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        generate_title_after_reply(
            shared,
            current,
            StaticTitle,
            Config::default(),
            "问题".into(),
            "回答".into(),
        )
        .await;
        assert!(SessionStore::open(&path)?.list()?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn reply_is_idle_and_saved_before_title_request_finishes() -> Result<()> {
        let provider = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("Generate a concise title"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_secs(1))
                    .set_body_json(serde_json::json!({
                        "choices": [{"message": {"content": "简短标题"}, "finish_reason": "stop"}]
                    })),
            )
            .with_priority(1)
            .expect(1)
            .mount(&provider)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(concat!(
                        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"先核对证据\"}}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{\"content\":\"任务完成\"},\"finish_reason\":null}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                        "data: [DONE]\n\n"
                    )),
            )
            .with_priority(10)
            .expect(1)
            .mount(&provider)
            .await;
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        config::save_config(
            &path,
            &Config {
                endpoint: format!("{}/v1", provider.uri()),
                api_key: "test-key".into(),
                model: "test-model".into(),
                api_type: ApiType::ChatCompletions,
                ..Config::default()
            },
        )?;
        let shared = state(path.clone());
        post_message(
            State(shared.clone()),
            Json(Message {
                session_id: "web-test".into(),
                text: "请简短回答".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        let current = session(&shared, "web-test")?;
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let ready = current
                    .inner
                    .lock()
                    .is_ok_and(|inner| !inner.busy && inner.turns.len() == 1);
                if ready {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await?;
        assert_eq!(
            current.inner.lock().map_err(|_| anyhow!("poisoned"))?.title,
            "新会话"
        );
        let store = SessionStore::open(&path)?;
        assert_eq!(store.load("web-test", 10, 1024)?.len(), 1);
        assert!(store.load_web_checkpoint("web-test")?.is_none());
        let display = store
            .load_web_presentation("web-test")?
            .context("display")?;
        assert_eq!(display.turns, 1);
        assert_eq!(display.task.steps, 1);
        assert_eq!(display.task.tool_calls, 0);
        assert!(display.task.timing_available);
        assert!(display
            .history
            .iter()
            .any(|line| line.starts_with("\u{1e}TASK:")));
        assert!(display
            .history
            .iter()
            .any(|line| line.contains("先核对证据")));
        let log = std::fs::read_to_string(directory.path().join("nl2sh.log"))?;
        assert!(log.contains("web_user"));
        assert!(log.contains("web_assistant"));
        assert!(log.contains("web_task_finished"));

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if current
                    .inner
                    .lock()
                    .is_ok_and(|inner| inner.title == "简短标题")
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await?;
        assert_eq!(store.list()?[0].title, "简短标题");
        Ok(())
    }

    #[tokio::test]
    async fn tool_start_is_checkpointed_before_a_result_exists() -> Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let shared = state(path.clone());
        let current = session(&shared, "web-test")?;
        {
            let mut inner = current.inner.lock().map_err(|_| anyhow!("poisoned"))?;
            inner.checkpoint = Some(WebCheckpoint {
                status: "running".into(),
                activity: "thinking".into(),
                history: Vec::new(),
                events: Vec::new(),
            });
        }
        let sink = WebTextSink {
            session: current,
            state: Some(shared),
        };
        sink.tool_started("pending", "read_file");
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Ok(Some(checkpoint)) = SessionStore::open(&path)
                    .and_then(|store| store.load_web_checkpoint("web-test"))
                {
                    if checkpoint
                        .events
                        .iter()
                        .any(|event| event.kind == "tool_started")
                    {
                        assert!(checkpoint
                            .history
                            .iter()
                            .any(|line| line.contains(LIVE_TOOL_PENDING_PREFIX)));
                        break;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn checkpoint_keeps_redacting_keys_after_provider_changes() -> Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let mut cfg = Config {
            api_key: "old-private-key".into(),
            ..Config::default()
        };
        config::save_config(&path, &cfg)?;
        let shared = state(path.clone());
        let current = session(&shared, "web-test")?;
        {
            let mut inner = current.inner.lock().map_err(|_| anyhow!("poisoned"))?;
            remember_redaction_secrets(&mut inner, &cfg);
            inner
                .history
                .push("> old-private-key new-private-key".into());
            inner.checkpoint = Some(WebCheckpoint {
                status: "running".into(),
                activity: "thinking".into(),
                history: Vec::new(),
                events: Vec::new(),
            });
        }
        cfg.api_key = "new-private-key".into();
        config::save_config(&path, &cfg)?;
        persist_web_session(&shared, &current).await?;
        let raw = std::fs::read_to_string(directory.path().join("sessions/web-test.json"))?;
        assert!(!raw.contains("old-private-key"));
        assert!(!raw.contains("new-private-key"));
        Ok(())
    }

    #[test]
    fn retry_uses_only_the_original_failed_input() {
        let mut inner = SessionState::empty();
        inner.history = vec![
            "> inspect the device".into(),
            "\u{1e}TOOL_OK:untrusted output".into(),
            "❌ provider failed".into(),
        ];
        inner.checkpoint_start = Some(0);
        inner.checkpoint = Some(WebCheckpoint {
            status: "failed".into(),
            activity: "thinking".into(),
            history: Vec::new(),
            events: Vec::new(),
        });
        assert_eq!(
            retryable_input(&inner).as_deref(),
            Some("inspect the device")
        );

        if let Some(checkpoint) = inner.checkpoint.as_mut() {
            checkpoint.history = vec!["> [NL2SH CREDENTIAL REDACTED]".into()];
        }
        assert!(retryable_input(&inner).is_none());
    }

    #[test]
    fn streaming_deltas_share_one_entry_and_tool_results_are_typed() -> Result<()> {
        let state = state(PathBuf::from("unused.toml"));
        let current = session(&state, "web-test")?;
        let sink = WebTextSink {
            session: current.clone(),
            state: None,
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
    fn tool_events_are_visible_immediately_and_match_results_by_call_id() -> Result<()> {
        let state = state(PathBuf::from("unused.toml"));
        let current = session(&state, "web-test")?;
        let sink = WebTextSink {
            session: current.clone(),
            state: None,
        };

        sink.tool_started("first", "analyze_audio");
        sink.tool_started("second", "analyze_audio");
        {
            let inner = current.inner.lock().map_err(|_| anyhow!("lock poisoned"))?;
            assert_eq!(
                web_entries(&inner.history),
                vec![
                    WebEntry {
                        kind: "tool_call",
                        text: "analyze_audio".into(),
                    },
                    WebEntry {
                        kind: "tool_call",
                        text: "analyze_audio".into(),
                    },
                ]
            );
        }

        sink.tool_finished("second", "second result", true);
        sink.tool_finished("first", "first result", false);
        let inner = current.inner.lock().map_err(|_| anyhow!("lock poisoned"))?;
        assert_eq!(
            web_entries(&inner.history),
            vec![
                WebEntry {
                    kind: "tool_call",
                    text: "analyze_audio".into(),
                },
                WebEntry {
                    kind: "tool_error",
                    text: "first result".into(),
                },
                WebEntry {
                    kind: "tool_call",
                    text: "analyze_audio".into(),
                },
                WebEntry {
                    kind: "tool_result",
                    text: "second result".into(),
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn live_shell_output_disappears_when_tool_result_is_ready() -> Result<()> {
        let state = state(PathBuf::from("unused.toml"));
        let current = session(&state, "web-test")?;
        let sink = WebTextSink {
            session: current.clone(),
            state: None,
        };
        let output = WebOutput {
            session: current.clone(),
        };

        sink.tool_started("shell", "execute_shell_command");
        output.stdout("14");
        output.stderr("warning");
        {
            let inner = current.inner.lock().map_err(|_| anyhow!("lock poisoned"))?;
            assert_eq!(
                web_entries(&inner.history)
                    .iter()
                    .filter(|entry| entry.kind == "tool_output")
                    .count(),
                2
            );
        }

        sink.tool_finished("shell", "stdout:\n14\nstderr:\nwarning", true);
        let inner = current.inner.lock().map_err(|_| anyhow!("lock poisoned"))?;
        assert_eq!(
            web_entries(&inner.history),
            vec![
                WebEntry {
                    kind: "tool_call",
                    text: "execute_shell_command".into(),
                },
                WebEntry {
                    kind: "tool_result",
                    text: "stdout:\n14\nstderr:\nwarning".into(),
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn restored_chart_keeps_matching_call_and_structured_result() {
        let chart = serde_json::json!({
            "chart_type": "bar",
            "title": "Storage",
            "source": "android_storage result",
            "unit": "GiB",
            "labels": ["apps", "media"],
            "values": [2.5, 4.0]
        })
        .to_string();
        let round = ToolRound {
            calls: vec![ToolCall {
                id: "chart-1".into(),
                name: "create_chart".into(),
                arguments: serde_json::json!({}),
            }],
            results: vec![ToolResult {
                call_id: "chart-1".into(),
                output: chart.clone(),
                success: true,
                attachments: Vec::new(),
            }],
        };
        let entries = web_entries(&render_turns(&[vec![ConversationItem::Tools(round)]]));
        assert_eq!(entries[0].kind, "tool_call");
        assert_eq!(entries[0].text, "create_chart");
        assert_eq!(entries[1].kind, "tool_result");
        assert_eq!(entries[1].text, chart);
    }

    #[test]
    fn tool_round_results_follow_their_matching_calls_in_live_and_restored_history() {
        let round = ToolRound {
            calls: vec![
                ToolCall {
                    id: "first".into(),
                    name: "read_file".into(),
                    arguments: serde_json::json!({}),
                },
                ToolCall {
                    id: "second".into(),
                    name: "list_dir".into(),
                    arguments: serde_json::json!({}),
                },
            ],
            results: vec![
                ToolResult {
                    call_id: "second".into(),
                    output: "second output".into(),
                    success: false,
                    attachments: Vec::new(),
                },
                ToolResult {
                    call_id: "first".into(),
                    output: "first output".into(),
                    success: true,
                    attachments: Vec::new(),
                },
            ],
        };
        let expected = vec![
            "🔧 read_file",
            "\u{1e}TOOL_OK:first output",
            "🔧 list_dir",
            "\u{1e}TOOL_ERR:second output",
        ];
        let mut live = Vec::new();
        append_tool_round(&mut live, &round);
        assert_eq!(live, expected);
        assert_eq!(
            render_turns(&[vec![ConversationItem::Tools(round)]]),
            expected
        );
    }

    #[test]
    fn export_keeps_selected_entries_when_log_does_not_exist() -> Result<()> {
        let dir = tempdir()?;
        let conversation = ExportConversation {
            id: "selected".into(),
            title: "当前会话".into(),
            exported_unix_secs: 1,
            task: WebTaskMetrics::default(),
            busy: true,
            entries: web_entries(&["> 本轮问题".into(), "🤖 本轮回答".into()]),
            checkpoint_status: None,
            checkpoint_events: Vec::new(),
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
    async fn model_list_uses_unsaved_quick_start_credentials() -> Result<()> {
        let provider_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let provider_port = provider_listener.local_addr()?.port();
        let provider = Router::new().route(
            "/v1/models",
            get(|headers: axum::http::HeaderMap| async move {
                let authorization = headers
                    .get(header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok());
                match authorization {
                    Some("Bearer draft-key") => (
                        StatusCode::OK,
                        Json(serde_json::json!({"data":[{"id":"draft-model"}]})),
                    ),
                    Some("Bearer gateway-test-key") => (
                        StatusCode::BAD_GATEWAY,
                        Json(serde_json::json!({"error":"upstream detail must stay private"})),
                    ),
                    _ => (
                        StatusCode::UNAUTHORIZED,
                        Json(serde_json::json!({"error":"unauthorized"})),
                    ),
                }
            }),
        );
        let provider_task = tokio::spawn(async move {
            let _ = axum::serve(provider_listener, provider).await;
        });
        let dir = tempdir()?;
        let path = dir.path().join("config.toml");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let server = start_with_listener(path.clone(), listener).await?;
        let web_port = url::Url::parse(server.url())?
            .port()
            .context("web URL lacks port")?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let response = client
            .post(format!("http://127.0.0.1:{web_port}/api/models"))
            .json(&serde_json::json!({
                "endpoint": format!("http://127.0.0.1:{provider_port}/v1"),
                "api_key": "draft-key"
            }))
            .send()
            .await?;
        assert!(response.status().is_success());
        let models: serde_json::Value = response.json().await?;
        assert_eq!(models[0]["id"], "draft-model");
        let failure = client
            .post(format!("http://127.0.0.1:{web_port}/api/models"))
            .json(&serde_json::json!({
                "endpoint": format!("http://127.0.0.1:{provider_port}/v1"),
                "api_key": "gateway-test-key"
            }))
            .send()
            .await?;
        assert_eq!(failure.status(), StatusCode::BAD_REQUEST);
        assert!(failure.text().await?.contains("502 Bad Gateway"));
        let log = std::fs::read_to_string(config::state_dir(&path)?.join("nl2sh.log"))?;
        assert!(log.contains("web_model_list_started"));
        assert!(log.contains("web_model_list_finished"));
        assert!(log.contains("502 Bad Gateway"));
        assert!(!log.contains("draft-key"));
        assert!(!log.contains("gateway-test-key"));
        assert!(!log.contains("upstream detail must stay private"));
        assert!(!path.exists());
        provider_task.abort();
        Ok(())
    }

    #[tokio::test]
    async fn health_and_info_identify_actual_listener_without_provider_or_sessions() -> Result<()> {
        let dir = tempdir()?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let port = listener.local_addr()?.port();
        let server = start_with_listener(dir.path().join("missing.toml"), listener).await?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let base = format!("http://127.0.0.1:{port}");
        let health: serde_json::Value = client
            .get(format!("{base}/healthz"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(health, serde_json::json!({"status":"ok"}));
        let info: serde_json::Value = client
            .get(format!("{base}/api/info"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(info["port"], port);
        assert_eq!(info["pid"], std::process::id());
        assert_eq!(info["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(info["protocol"], 1);
        assert!(info["uptime"].is_u64());
        assert!(info["capabilities"]["android_shell"].is_boolean());
        assert!(info["capabilities"]["jadx_provisionable"].is_boolean());
        assert!(!info.to_string().contains("api_key"));
        server.task.abort();
        Ok(())
    }

    #[tokio::test]
    async fn config_editor_validates_renders_and_applies_saved_settings() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("config.toml");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let server = start_with_listener(path.clone(), listener).await?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let port = url::Url::parse(server.url())?
            .port()
            .context("web URL lacks port")?;
        let base = format!("http://127.0.0.1:{port}");
        let version = client
            .get(format!("{base}/api/version"))
            .send()
            .await?
            .text()
            .await?;
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
        let initial_quick: serde_json::Value = client
            .get(format!("{base}/api/quick-settings"))
            .send()
            .await?
            .json()
            .await?;
        if std::env::var_os("NL2SH_API_KEY").is_none() {
            assert_eq!(initial_quick["provider_ready"], false);
        }
        let invalid: serde_json::Value = client
            .post(format!("{base}/api/config/validate"))
            .body("model = [")
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(invalid["valid"], false);
        let original = client
            .get(format!("{base}/api/config"))
            .send()
            .await?
            .text()
            .await?;
        let preview: serde_json::Value = client
            .post(format!("{base}/api/config/validate"))
            .body(original)
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(preview["valid"], true);
        let mut config = preview["config"].clone();
        config["model"] = serde_json::json!("web-editor-test");
        config["api_key"] = serde_json::json!("test-key");
        let rendered = client
            .post(format!("{base}/api/config/render"))
            .json(&config)
            .send()
            .await?;
        assert!(rendered.status().is_success());
        let rendered: serde_json::Value = rendered.json().await?;
        assert_eq!(rendered["valid"], true);
        let rendered = rendered["toml"].as_str().context("missing rendered TOML")?;
        assert!(rendered.contains("model = \"web-editor-test\""));
        let saved = client
            .post(format!("{base}/api/config"))
            .body(rendered.to_owned())
            .send()
            .await?;
        assert!(saved.status().is_success());
        assert_eq!(load_config(path).await?.model, "web-editor-test");
        let ready_quick: serde_json::Value = client
            .get(format!("{base}/api/quick-settings"))
            .send()
            .await?
            .json()
            .await?;
        if std::env::var_os("NL2SH_API_KEY").is_none() {
            assert_eq!(ready_quick["provider_ready"], true);
        }
        assert!(ready_quick.get("api_key").is_none());
        Ok(())
    }

    #[tokio::test]
    async fn deletes_single_and_all_web_sessions_without_touching_other_state() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("config.toml");
        let store = SessionStore::open(&path)?;
        store.save("saved-one", &[], 1024)?;
        store.save("saved-two", &[], 1024)?;
        let state_directory = config::state_dir(&path)?;
        std::fs::write(state_directory.join("keep.txt"), b"keep")?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let server = start_with_listener(path, listener).await?;
        let port = url::Url::parse(server.url())?
            .port()
            .context("web URL lacks port")?;
        let base = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::builder().no_proxy().build()?;
        let created: serde_json::Value = client
            .post(format!("{base}/api/sessions/new"))
            .send()
            .await?
            .json()
            .await?;
        let created_id = created["id"].as_str().context("missing session id")?;
        assert!(client
            .post(format!("{base}/api/sessions/delete"))
            .json(&serde_json::json!({"name": "saved-one"}))
            .send()
            .await?
            .status()
            .is_success());
        assert!(store
            .list()?
            .iter()
            .all(|session| session.name != "saved-one"));
        assert!(client
            .post(format!("{base}/api/sessions/delete"))
            .json(&serde_json::json!({"name": created_id}))
            .send()
            .await?
            .status()
            .is_success());
        assert_eq!(
            client
                .get(format!("{base}/api/state?id={created_id}"))
                .send()
                .await?
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert!(client
            .post(format!("{base}/api/sessions/delete-all"))
            .send()
            .await?
            .status()
            .is_success());
        let remaining: serde_json::Value = client
            .get(format!("{base}/api/sessions"))
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(remaining.as_array().map(Vec::len), Some(0));
        assert!(store.list()?.is_empty());
        assert_eq!(std::fs::read(state_directory.join("keep.txt"))?, b"keep");
        Ok(())
    }

    #[tokio::test]
    async fn refuses_to_delete_running_sessions() -> Result<()> {
        let dir = tempdir()?;
        let state = Arc::new(Shared {
            update: Mutex::new(UpdateJob::default()),
            started: Instant::now(),
            port: 0,
            path: dir.path().join("config.toml"),
            sessions: Mutex::new(BTreeMap::from([(
                "web-test".into(),
                web_session(SessionState {
                    id: "web-test".into(),
                    ..SessionState::empty()
                }),
            )])),
        });
        let current = session(&state, "web-test")?;
        current.inner.lock().map_err(|_| anyhow!("poisoned"))?.busy = true;
        assert!(matches!(
            delete_session(
                State(state.clone()),
                Json(SessionSelection {
                    name: "web-test".into()
                })
            )
            .await,
            Err(ApiError {
                status: StatusCode::CONFLICT,
                ..
            })
        ));
        assert!(matches!(
            delete_all_sessions(State(state)).await,
            Err(ApiError {
                status: StatusCode::CONFLICT,
                ..
            })
        ));
        Ok(())
    }

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
        let tools: serde_json::Value = client
            .get(format!("{base}/api/tools"))
            .send()
            .await?
            .json()
            .await?;
        assert!(tools
            .as_array()
            .is_some_and(|items| items.iter().any(|tool| {
                tool["name"] == "read_file" && tool["description"].as_str().is_some()
            })));
        assert!(tools.as_array().is_some_and(|items| items
            .iter()
            .any(|tool| tool["name"] == "tailcat_check" && tool["enabled"] == false)));
        let changed: serde_json::Value = client
            .post(format!("{base}/api/tools/toggle"))
            .json(&serde_json::json!({"group":"tailcat","enabled":true}))
            .send()
            .await?
            .json()
            .await?;
        assert!(changed.as_array().is_some_and(|items| items
            .iter()
            .any(|tool| tool["name"] == "tailcat_check" && tool["enabled"] == true)));
        let changed: serde_json::Value = client
            .post(format!("{base}/api/tools/toggle"))
            .json(&serde_json::json!({"tool":"tailcat_serve","enabled":false}))
            .send()
            .await?
            .json()
            .await?;
        assert!(changed.as_array().is_some_and(|items| items
            .iter()
            .any(|tool| tool["name"] == "tailcat_serve" && tool["enabled"] == false)));
        let persisted =
            crate::config::load_or_default_unvalidated(&dir.path().join("config.toml"))?;
        assert!(persisted.tool_groups["tailcat"]);
        assert_eq!(persisted.tool_overrides.get("tailcat_serve"), Some(&false));
        let apps: serde_json::Value = client
            .get(format!("{base}/api/apps"))
            .send()
            .await?
            .json()
            .await?;
        assert!(apps.as_array().is_some());
        std::fs::write(dir.path().join("web-reference.txt"), "example")?;
        let suggestions: Vec<String> = client
            .get(format!("{base}/api/file-suggestions"))
            .query(&[(
                "fragment",
                dir.path().join("web-ref").to_string_lossy().to_string(),
            )])
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(
            suggestions,
            vec![dir.path().join("web-reference.txt").to_string_lossy()]
        );
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

    #[tokio::test]
    async fn unfinished_web_request_recovers_as_diagnostic_history() -> Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("config.toml");
        let mut initial = SessionState::empty();
        initial.id = "web-test".into();
        let original = Arc::new(Shared {
            update: Mutex::new(UpdateJob::default()),
            started: Instant::now(),
            port: 0,
            path: path.clone(),
            sessions: Mutex::new(BTreeMap::from([("web-test".into(), web_session(initial))])),
        });
        let current = session(&original, "web-test")?;
        {
            let mut inner = current.inner.lock().map_err(|_| anyhow!("poisoned"))?;
            inner.busy = true;
            inner.turns.push(vec![
                ConversationItem::Message(ConversationMessage::new(Role::User, "旧问题")),
                ConversationItem::Message(ConversationMessage::new(Role::Assistant, "旧回答")),
            ]);
            inner.history = render_turns(&inner.turns);
            inner.checkpoint_start = Some(inner.history.len());
            inner.history.extend([
                "> 检查设备".into(),
                "🔧 inspect_android_environment".into(),
                "\u{1e}TOOL_OK:设备已检查".into(),
                format!("{LIVE_TOOL_CALL_PREFIX}pending\tread_file"),
                format!("{LIVE_TOOL_PENDING_PREFIX}pending"),
            ]);
            inner.checkpoint = Some(WebCheckpoint {
                status: "running".into(),
                activity: "thinking".into(),
                history: Vec::new(),
                events: vec![WebCheckpointEvent {
                    at: 1,
                    kind: "tool_completed".into(),
                    tool: None,
                }],
            });
        }
        persist_web_session(&original, &current).await?;
        let restarted = Arc::new(Shared {
            update: Mutex::new(UpdateJob::default()),
            started: Instant::now(),
            port: 0,
            path: path.clone(),
            sessions: Mutex::new(BTreeMap::new()),
        });
        let Json(_loaded) = load_session(
            State(restarted.clone()),
            Json(SessionSelection {
                name: "web-test".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        let Json(snapshot) = get_state(
            State(restarted),
            Query(StateQuery {
                id: "web-test".into(),
            }),
        )
        .await
        .map_err(|error| error.error)?;
        assert!(!snapshot.busy);
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.kind == "tool_result"));
        assert!(snapshot.entries.iter().any(|entry| {
            entry.kind == "tool_error" && entry.text.contains("执行状态未知")
        }));
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.text.contains("上次任务")));
        assert_eq!(
            SessionStore::open(&path)?.load("web-test", 10, 1024)?.len(),
            1
        );
        assert_eq!(
            snapshot
                .entries
                .iter()
                .filter(|entry| entry.text == "旧问题")
                .count(),
            1
        );
        assert_eq!(
            SessionStore::open(&path)?
                .load_web_checkpoint("web-test")?
                .context("missing checkpoint")?
                .status,
            "interrupted"
        );
        Ok(())
    }
}
