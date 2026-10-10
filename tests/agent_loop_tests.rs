use anyhow::Result;
use async_trait::async_trait;
use nl2sh::{
    agent::{
        AgentRunner, ConfirmationDecision, Confirmer, LimitType, QuestionAnswers, UserQuestion,
    },
    config::Config,
    llm::{
        ConversationItem, ConversationMessage, FinishReason, LlmClient, LlmRequest, LlmResponse,
        Role, TextDeltaSink, ToolCall, Usage,
    },
    security::SecurityAssessment,
    shell::{CommandExecutor, ExecutionResult},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
struct MockLlm {
    calls: AtomicUsize,
    command: &'static str,
}

struct VisibilityLlm {
    expect_tailcat: bool,
    expect_jadx: bool,
}

#[async_trait]
impl LlmClient for VisibilityLlm {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
        assert_eq!(
            request
                .tools
                .iter()
                .any(|tool| tool.name == "tailcat_check"),
            self.expect_tailcat
        );
        assert_eq!(
            request.tools.iter().any(|tool| tool.name == "inspect_apk"),
            self.expect_jadx
        );
        assert!(!request
            .tools
            .iter()
            .any(|tool| tool.name == "tailcat_serve"));
        Ok(LlmResponse {
            text: Some("done".into()),
            tool_calls: Vec::new(),
            usage: Usage::default(),
            finish_reason: FinishReason::Stop,
        })
    }
}

#[tokio::test]
async fn model_only_discovers_enabled_optional_tools() -> Result<()> {
    let mut config = Config::default();
    let executor = Exec {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    AgentRunner {
        config: &config,
        llm: &VisibilityLlm {
            expect_tailcat: false,
            expect_jadx: false,
        },
        executor: &executor,
        confirmer: &Confirm(false),
    }
    .run("inspect")
    .await?;
    config.tool_groups.insert("jadx".into(), true);
    config.tool_overrides.insert("tailcat_check".into(), true);
    AgentRunner {
        config: &config,
        llm: &VisibilityLlm {
            expect_tailcat: true,
            expect_jadx: true,
        },
        executor: &executor,
        confirmer: &Confirm(false),
    }
    .run("inspect")
    .await?;
    Ok(())
}

#[derive(Default)]
struct ToolEventSink {
    events: Mutex<Vec<String>>,
    progress: Mutex<Vec<(usize, usize)>>,
}

impl TextDeltaSink for ToolEventSink {
    fn agent_progress(&self, steps: usize, tools: usize, _: &Usage) {
        if let Ok(mut progress) = self.progress.lock() {
            progress.push((steps, tools));
        }
    }
    fn delta(&self, _: &str) {}

    fn tool_started(&self, call_id: &str, name: &str) {
        if let Ok(mut events) = self.events.lock() {
            events.push(format!("started:{call_id}:{name}"));
        }
    }

    fn tool_finished(&self, call_id: &str, output: &str, success: bool) {
        if let Ok(mut events) = self.events.lock() {
            events.push(format!(
                "finished:{call_id}:{success}:{}",
                output.contains("stdout:\nok")
            ));
        }
    }
}

struct AlwaysToolLlm {
    calls: AtomicUsize,
    command: &'static str,
}

struct RepairingArgumentsLlm {
    calls: AtomicUsize,
}

#[async_trait]
impl LlmClient for RepairingArgumentsLlm {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
        match self.calls.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(LlmResponse {
                text: None,
                tool_calls: vec![ToolCall::from_raw_arguments(
                    "broken",
                    "execute_shell_command",
                    r#"{"command":"id"#,
                )],
                usage: Usage::default(),
                finish_reason: FinishReason::ToolCalls,
            }),
            1 => {
                let rejected = request.items.iter().find_map(|item| match item {
                    ConversationItem::Tools(round) => round.results.first(),
                    _ => None,
                });
                assert!(rejected.is_some_and(|result| {
                    !result.success
                        && result.output.contains("invalid JSON")
                        && result.output.contains("not executed")
                }));
                Ok(LlmResponse {
                    text: None,
                    tool_calls: vec![ToolCall {
                        id: "fixed".into(),
                        name: "execute_shell_command".into(),
                        arguments: json!({"command":"id"}),
                    }],
                    usage: Usage::default(),
                    finish_reason: FinishReason::ToolCalls,
                })
            }
            _ => Ok(LlmResponse {
                text: Some("repaired".into()),
                tool_calls: Vec::new(),
                usage: Usage::default(),
                finish_reason: FinishReason::Stop,
            }),
        }
    }
}

struct AlwaysMalformedArgumentsLlm {
    calls: AtomicUsize,
}

#[async_trait]
impl LlmClient for AlwaysMalformedArgumentsLlm {
    async fn complete(&self, _: LlmRequest) -> Result<LlmResponse> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(LlmResponse {
            text: None,
            tool_calls: vec![ToolCall::from_raw_arguments(
                format!("broken-{call}"),
                "execute_shell_command",
                r#"{"command":"id"#,
            )],
            usage: Usage::default(),
            finish_reason: FinishReason::ToolCalls,
        })
    }
}

struct StructuredFileLlm {
    calls: AtomicUsize,
    path: String,
}

struct StructuredPatchLlm {
    calls: AtomicUsize,
    path: String,
    expected_success: bool,
}

#[async_trait]
impl LlmClient for StructuredPatchLlm {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(LlmResponse {
                text: None,
                tool_calls: vec![ToolCall {
                    id: "patch".into(),
                    name: "apply_patch".into(),
                    arguments: json!({"path":self.path,"old_text":"before","new_text":"after"}),
                }],
                usage: Usage::default(),
                finish_reason: FinishReason::ToolCalls,
            })
        } else {
            let result = request.items.iter().find_map(|item| match item {
                ConversationItem::Tools(round) => round.results.first(),
                _ => None,
            });
            assert_eq!(
                result.map(|result| result.success),
                Some(self.expected_success)
            );
            Ok(LlmResponse {
                text: Some("done".into()),
                tool_calls: vec![],
                usage: Usage::default(),
                finish_reason: FinishReason::Stop,
            })
        }
    }
}

struct PatchConfirm {
    decision: ConfirmationDecision,
    calls: AtomicUsize,
}

#[async_trait]
impl Confirmer for PatchConfirm {
    async fn confirm(
        &self,
        preview: &nl2sh::agent::ConfirmationRequest<'_>,
        assessment: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        let preview = preview.preview;
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(preview.contains("-before"));
        assert!(preview.contains("+after"));
        assert_eq!(assessment.risk_level, nl2sh::security::RiskLevel::Mutating);
        assert!(assessment.requires_confirmation);
        Ok(self.decision.clone())
    }
}

#[tokio::test]
async fn prepared_patch_needs_confirmation_before_writing() -> Result<()> {
    for decision in [
        ConfirmationDecision::Reject,
        ConfirmationDecision::Approve,
        ConfirmationDecision::Edit("different change".into()),
    ] {
        let approve = decision == ConfirmationDecision::Approve;
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("change.txt");
        std::fs::write(&path, "before")?;
        let llm = StructuredPatchLlm {
            calls: AtomicUsize::new(0),
            path: path.to_string_lossy().into_owned(),
            expected_success: approve,
        };
        let confirm = PatchConfirm {
            decision,
            calls: AtomicUsize::new(0),
        };
        let shell_calls = Arc::new(AtomicUsize::new(0));
        AgentRunner {
            config: &Config::default(),
            llm: &llm,
            executor: &Exec {
                calls: shell_calls.clone(),
            },
            confirmer: &confirm,
        }
        .run("change the file")
        .await?;
        assert_eq!(confirm.calls.load(Ordering::SeqCst), 1);
        assert_eq!(shell_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            std::fs::read_to_string(path)?,
            if approve { "after" } else { "before" }
        );
    }
    Ok(())
}

#[async_trait]
impl LlmClient for StructuredFileLlm {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            assert!(request.tools.iter().any(|tool| tool.name == "read_file"));
            Ok(LlmResponse {
                text: None,
                tool_calls: vec![ToolCall {
                    id: "file-read".into(),
                    name: "read_file".into(),
                    arguments: json!({"path": self.path}),
                }],
                usage: Usage::default(),
                finish_reason: FinishReason::ToolCalls,
            })
        } else {
            let output = request.items.iter().find_map(|item| match item {
                ConversationItem::Tools(round) => round.results.first(),
                _ => None,
            });
            assert!(output.is_some_and(|result| result.success && result.output == "file evidence"));
            Ok(LlmResponse {
                text: Some("done".into()),
                tool_calls: vec![],
                usage: Usage::default(),
                finish_reason: FinishReason::Stop,
            })
        }
    }
}

#[tokio::test]
async fn registry_dispatches_file_result_back_to_model() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("evidence.txt");
    std::fs::write(&path, "file evidence")?;
    let llm = StructuredFileLlm {
        calls: AtomicUsize::new(0),
        path: path.to_string_lossy().into_owned(),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let outcome = AgentRunner {
        config: &Config::default(),
        llm: &llm,
        executor: &Exec {
            calls: calls.clone(),
        },
        confirmer: &Confirm(false),
    }
    .run("read the file")
    .await?;
    assert_eq!(outcome.final_text, "done");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[async_trait]
impl LlmClient for AlwaysToolLlm {
    async fn complete(&self, _: LlmRequest) -> Result<LlmResponse> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(LlmResponse {
            text: None,
            tool_calls: vec![ToolCall {
                id: format!("always-{call}"),
                name: "execute_shell_command".into(),
                arguments: json!({"command": self.command}),
            }],
            usage: Usage::default(),
            finish_reason: FinishReason::ToolCalls,
        })
    }
}

#[tokio::test]
async fn tool_budget_blocks_the_next_tool_before_execution() {
    let cfg = Config {
        max_agent_steps: 10,
        max_tool_calls: 1,
        ..Config::default()
    };
    let llm = AlwaysToolLlm {
        calls: AtomicUsize::new(0),
        command: "id",
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let outcome = AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &Exec {
            calls: calls.clone(),
        },
        confirmer: &Confirm(true),
    }
    .run("loop")
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(outcome.tool_calls, 1);
    assert_eq!(outcome.stats.limit_reached, Some(LimitType::ToolCalls));
}

#[tokio::test]
async fn malformed_arguments_are_rejected_then_repaired_without_early_execution() -> Result<()> {
    let llm = RepairingArgumentsLlm {
        calls: AtomicUsize::new(0),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let outcome = AgentRunner {
        config: &Config::default(),
        llm: &llm,
        executor: &Exec {
            calls: calls.clone(),
        },
        confirmer: &Confirm(true),
    }
    .run("repair")
    .await?;
    assert_eq!(outcome.final_text, "repaired");
    assert_eq!(outcome.tool_calls, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn malformed_arguments_stop_after_two_repair_attempts_without_execution() {
    let llm = AlwaysMalformedArgumentsLlm {
        calls: AtomicUsize::new(0),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let error = AgentRunner {
        config: &Config::default(),
        llm: &llm,
        executor: &Exec {
            calls: calls.clone(),
        },
        confirmer: &Confirm(true),
    }
    .run("never execute")
    .await
    .expect_err("malformed arguments must stop after bounded repair attempts");
    assert!(error
        .to_string()
        .contains("remained invalid after 2 repair attempts"));
    assert_eq!(llm.calls.load(Ordering::SeqCst), 3);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn hard_step_limit_caps_user_step_configuration() {
    let cfg = Config {
        max_agent_steps: 10,
        hard_max_agent_steps: 2,
        max_tool_calls: 10,
        ..Config::default()
    };
    let llm = AlwaysToolLlm {
        calls: AtomicUsize::new(0),
        command: "id",
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let outcome = AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &Exec { calls },
        confirmer: &Confirm(true),
    }
    .run("loop")
    .await
    .unwrap();
    assert_eq!(outcome.steps, 2);
    assert_eq!(outcome.stats.limit_reached, Some(LimitType::SystemHardStep));
}

#[tokio::test]
async fn repeated_identical_action_is_blocked_before_fourth_execution() {
    let cfg = Config {
        max_agent_steps: 5,
        max_tool_calls: 5,
        max_same_action_retries: 3,
        ..Config::default()
    };
    let llm = AlwaysToolLlm {
        calls: AtomicUsize::new(0),
        command: "id   -u",
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let outcome = AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &Exec {
            calls: calls.clone(),
        },
        confirmer: &Confirm(true),
    }
    .run("repeat")
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(outcome.tool_calls, 5);
}
#[async_trait]
impl LlmClient for MockLlm {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            Ok(LlmResponse {
                text: None,
                tool_calls: vec![ToolCall {
                    id: "1".into(),
                    name: "execute_shell_command".into(),
                    arguments: json!({"command":self.command}),
                }],
                usage: Usage::default(),
                finish_reason: FinishReason::ToolCalls,
            })
        } else {
            let round = req
                .items
                .iter()
                .find_map(|item| match item {
                    nl2sh::llm::ConversationItem::Tools(round) => Some(round),
                    _ => None,
                })
                .expect("tool round should be retained");
            assert_eq!(round.calls.len(), round.results.len());
            Ok(LlmResponse {
                text: Some("done".into()),
                tool_calls: vec![],
                usage: Usage::default(),
                finish_reason: FinishReason::Stop,
            })
        }
    }
}
struct Exec {
    calls: Arc<AtomicUsize>,
}
#[async_trait]
impl CommandExecutor for Exec {
    async fn execute(&self, _: &str, _: bool, _: bool) -> Result<ExecutionResult> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ExecutionResult {
            stdout: "ok".into(),
            stderr: String::new(),
            exit_code: Some(0),
            timed_out: false,
            interrupted: false,
        })
    }
}
struct Confirm(bool);
#[async_trait]
impl Confirmer for Confirm {
    async fn confirm(
        &self,
        _: &nl2sh::agent::ConfirmationRequest<'_>,
        _: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        Ok(if self.0 {
            ConfirmationDecision::Approve
        } else {
            ConfirmationDecision::Reject
        })
    }
}
struct EditThenReject {
    calls: AtomicUsize,
}

struct RememberApproval {
    calls: AtomicUsize,
}

#[async_trait]
impl Confirmer for RememberApproval {
    async fn confirm(
        &self,
        _: &nl2sh::agent::ConfirmationRequest<'_>,
        _: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ConfirmationDecision::ApproveForTask)
    }
}
#[async_trait]
impl Confirmer for EditThenReject {
    async fn confirm(
        &self,
        _: &nl2sh::agent::ConfirmationRequest<'_>,
        assessment: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            Ok(ConfirmationDecision::Edit("rm -rf /".into()))
        } else {
            assert!(assessment.requires_double_confirmation);
            Ok(ConfirmationDecision::Reject)
        }
    }
}
#[tokio::test]
async fn readonly_auto_executes_and_result_returns() {
    let cfg = Config::default();
    let llm = MockLlm {
        calls: AtomicUsize::new(0),
        command: "id",
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let exec = Exec {
        calls: calls.clone(),
    };
    let out = AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &exec,
        confirmer: &Confirm(false),
    }
    .run("who")
    .await
    .unwrap();
    assert_eq!(out.final_text, "done");
    assert_eq!(calls.load(Ordering::SeqCst), 1)
}

#[tokio::test]
async fn streaming_sink_receives_tool_start_and_finish_in_order() -> Result<()> {
    let cfg = Config::default();
    let llm = MockLlm {
        calls: AtomicUsize::new(0),
        command: "id",
    };
    let exec = Exec {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let sink = ToolEventSink::default();
    AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &exec,
        confirmer: &Confirm(false),
    }
    .run_with_history_streaming_owned("who".into(), Vec::new(), &sink)
    .await?;
    assert_eq!(
        sink.events
            .lock()
            .map_err(|_| anyhow::anyhow!("tool event lock poisoned"))?
            .as_slice(),
        ["started:1:execute_shell_command", "finished:1:true:true"]
    );
    assert_eq!(
        sink.progress
            .lock()
            .map_err(|_| anyhow::anyhow!("lock"))?
            .as_slice(),
        [(1, 0), (1, 0), (1, 1), (2, 1), (2, 1)]
    );
    Ok(())
}
#[tokio::test]
async fn mutating_rejection_prevents_execution() {
    let cfg = Config::default();
    let llm = MockLlm {
        calls: AtomicUsize::new(0),
        command: "touch /tmp/x",
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let exec = Exec {
        calls: calls.clone(),
    };
    AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &exec,
        confirmer: &Confirm(false),
    }
    .run("change")
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0)
}
#[tokio::test]
async fn max_steps_stops_loop() {
    let cfg = Config {
        max_agent_steps: 1,
        ..Config::default()
    };
    let llm = MockLlm {
        calls: AtomicUsize::new(0),
        command: "id",
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let exec = Exec { calls };
    let outcome = AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &exec,
        confirmer: &Confirm(true),
    }
    .run("loop")
    .await
    .unwrap();
    assert!(outcome.final_text.contains("maximum"));
}

#[tokio::test]
async fn edited_command_is_reclassified_before_execution() {
    let cfg = Config::default();
    let llm = MockLlm {
        calls: AtomicUsize::new(0),
        command: "touch /tmp/x",
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let exec = Exec {
        calls: calls.clone(),
    };
    let confirmer = EditThenReject {
        calls: AtomicUsize::new(0),
    };
    AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &exec,
        confirmer: &confirmer,
    }
    .run("edit")
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(confirmer.calls.load(Ordering::SeqCst), 2);
}

struct HistoryLlm;
#[async_trait]
impl LlmClient for HistoryLlm {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse> {
        let text = req
            .items
            .iter()
            .filter_map(|item| match item {
                ConversationItem::Message(m) => Some(m.content.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(!text.contains(&"old-user"));
        assert!(text.contains(&"new-user"));
        assert!(text.contains(&"current-user"));
        Ok(LlmResponse {
            text: Some("history-ok".into()),
            tool_calls: vec![],
            usage: Usage::default(),
            finish_reason: FinishReason::Stop,
        })
    }
}

#[tokio::test]
async fn context_drops_only_oldest_complete_turns() {
    let cfg = Config {
        max_context_turns: 2,
        ..Config::default()
    };
    let history = vec![
        vec![
            ConversationItem::Message(ConversationMessage::new(Role::User, "old-user")),
            ConversationItem::Message(ConversationMessage::new(Role::Assistant, "old-answer")),
        ],
        vec![
            ConversationItem::Message(ConversationMessage::new(Role::User, "new-user")),
            ConversationItem::Message(ConversationMessage::new(Role::Assistant, "new-answer")),
        ],
    ];
    let calls = Arc::new(AtomicUsize::new(0));
    let exec = Exec { calls };
    let outcome = AgentRunner {
        config: &cfg,
        llm: &HistoryLlm,
        executor: &exec,
        confirmer: &Confirm(true),
    }
    .run_with_history("current-user", &history)
    .await
    .unwrap();
    assert_eq!(outcome.final_text, "history-ok");
}

struct MultiRoundLlm {
    calls: AtomicUsize,
}

struct RepeatedMutatingLlm {
    calls: AtomicUsize,
}

#[async_trait]
impl LlmClient for RepeatedMutatingLlm {
    async fn complete(&self, _: LlmRequest) -> Result<LlmResponse> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call < 2 {
            Ok(LlmResponse {
                text: None,
                tool_calls: vec![ToolCall {
                    id: format!("mutating-{call}"),
                    name: "execute_shell_command".into(),
                    arguments: json!({"command":"touch /tmp/repeated"}),
                }],
                usage: Usage::default(),
                finish_reason: FinishReason::ToolCalls,
            })
        } else {
            Ok(LlmResponse {
                text: Some("remembered".into()),
                tool_calls: Vec::new(),
                usage: Usage::default(),
                finish_reason: FinishReason::Stop,
            })
        }
    }
}
#[async_trait]
impl LlmClient for MultiRoundLlm {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n < 2 {
            return Ok(LlmResponse {
                text: None,
                tool_calls: vec![ToolCall {
                    id: format!("c{n}"),
                    name: "execute_shell_command".into(),
                    arguments: json!({"command":"id"}),
                }],
                usage: Usage::default(),
                finish_reason: FinishReason::ToolCalls,
            });
        }
        assert_eq!(
            req.items
                .iter()
                .filter(|item| matches!(item, ConversationItem::Tools(_)))
                .count(),
            2
        );
        Ok(LlmResponse {
            text: Some("multi-done".into()),
            tool_calls: vec![],
            usage: Usage::default(),
            finish_reason: FinishReason::Stop,
        })
    }
}

#[tokio::test]
async fn multiple_tool_rounds_remain_ordered() {
    let cfg = Config::default();
    let llm = MultiRoundLlm {
        calls: AtomicUsize::new(0),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let exec = Exec {
        calls: calls.clone(),
    };
    let outcome = AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &exec,
        confirmer: &Confirm(true),
    }
    .run("twice")
    .await
    .unwrap();
    assert_eq!(outcome.final_text, "multi-done");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn task_approval_remembers_only_the_exact_mutating_command() {
    let cfg = Config::default();
    let llm = RepeatedMutatingLlm {
        calls: AtomicUsize::new(0),
    };
    let execution_calls = Arc::new(AtomicUsize::new(0));
    let exec = Exec {
        calls: execution_calls.clone(),
    };
    let confirmer = RememberApproval {
        calls: AtomicUsize::new(0),
    };
    let outcome = AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &exec,
        confirmer: &confirmer,
    }
    .run("repeat")
    .await
    .unwrap();
    assert_eq!(outcome.final_text, "remembered");
    assert_eq!(execution_calls.load(Ordering::SeqCst), 2);
    assert_eq!(confirmer.calls.load(Ordering::SeqCst), 1);
}

struct ResultCheckingLlm {
    expected: &'static str,
    calls: AtomicUsize,
}
#[async_trait]
impl LlmClient for ResultCheckingLlm {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            return Ok(LlmResponse {
                text: None,
                tool_calls: vec![ToolCall {
                    id: "result".into(),
                    name: "execute_shell_command".into(),
                    arguments: json!({"command":"id"}),
                }],
                usage: Usage::default(),
                finish_reason: FinishReason::ToolCalls,
            });
        }
        let output = req
            .items
            .iter()
            .find_map(|item| match item {
                ConversationItem::Tools(round) => round.results.first().map(|r| r.output.as_str()),
                _ => None,
            })
            .unwrap_or("");
        assert!(output.contains(self.expected), "{output}");
        Ok(LlmResponse {
            text: Some("observed".into()),
            tool_calls: vec![],
            usage: Usage::default(),
            finish_reason: FinishReason::Stop,
        })
    }
}
struct FailingExec;
#[async_trait]
impl CommandExecutor for FailingExec {
    async fn execute(&self, _: &str, _: bool, _: bool) -> Result<ExecutionResult> {
        anyhow::bail!("mock execution error")
    }
}
struct TimeoutExec;
#[async_trait]
impl CommandExecutor for TimeoutExec {
    async fn execute(&self, _: &str, _: bool, _: bool) -> Result<ExecutionResult> {
        Ok(ExecutionResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            timed_out: true,
            interrupted: false,
        })
    }
}
struct InterruptedExec;
#[async_trait]
impl CommandExecutor for InterruptedExec {
    async fn execute(&self, _: &str, _: bool, _: bool) -> Result<ExecutionResult> {
        Ok(ExecutionResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            timed_out: false,
            interrupted: true,
        })
    }
}

#[tokio::test]
async fn execution_failure_is_returned_to_model() {
    let cfg = Config::default();
    let llm = ResultCheckingLlm {
        expected: "Execution failed",
        calls: AtomicUsize::new(0),
    };
    let out = AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &FailingExec,
        confirmer: &Confirm(true),
    }
    .run("fail")
    .await
    .unwrap();
    assert_eq!(out.final_text, "observed")
}
#[tokio::test]
async fn timeout_is_returned_to_model() {
    let cfg = Config::default();
    let llm = ResultCheckingLlm {
        expected: "timed_out=true",
        calls: AtomicUsize::new(0),
    };
    AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &TimeoutExec,
        confirmer: &Confirm(true),
    }
    .run("timeout")
    .await
    .unwrap();
}

struct ProviderFailsAfterTool {
    calls: AtomicUsize,
}

#[async_trait]
impl LlmClient for ProviderFailsAfterTool {
    async fn complete(&self, _: LlmRequest) -> Result<LlmResponse> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Ok(LlmResponse {
                text: None,
                tool_calls: vec![ToolCall {
                    id: "load-1".into(),
                    name: "execute_shell_command".into(),
                    arguments: serde_json::json!({"command": "uptime"}),
                }],
                usage: Usage::default(),
                finish_reason: FinishReason::ToolCalls,
            });
        }
        anyhow::bail!("provider stream timed out")
    }
}

#[tokio::test]
async fn provider_failure_retains_completed_tool_round() {
    let cfg = Config::default();
    let llm = ProviderFailsAfterTool {
        calls: AtomicUsize::new(0),
    };
    let executor = Exec {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let error = AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &executor,
        confirmer: &Confirm(true),
    }
    .run("inspect load")
    .await
    .expect_err("the second provider request should fail");
    let failure = error
        .downcast_ref::<nl2sh::agent::AgentRunFailure>()
        .expect("failure should carry the partial transcript");
    assert!(matches!(
        failure.transcript(),
        [ConversationItem::Message(_), ConversationItem::Tools(_)]
    ));
}
#[tokio::test]
async fn interruption_stops_agent_loop() {
    let cfg = Config::default();
    let llm = ResultCheckingLlm {
        expected: "unused",
        calls: AtomicUsize::new(0),
    };
    assert!(AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &InterruptedExec,
        confirmer: &Confirm(true)
    }
    .run("cancel")
    .await
    .is_err());
    assert_eq!(llm.calls.load(Ordering::SeqCst), 1)
}

struct AnalyzeAudioToolLlm {
    calls: AtomicUsize,
    path: String,
    omit_metadata: bool,
}

#[async_trait]
impl LlmClient for AnalyzeAudioToolLlm {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            assert!(req.tools.iter().any(|tool| tool.name == "analyze_audio"));
            return Ok(LlmResponse {
                text: None,
                tool_calls: vec![ToolCall {
                    id: "audio-1".into(),
                    name: "analyze_audio".into(),
                    arguments: if self.omit_metadata {
                        json!({"path": self.path.clone()})
                    } else {
                        json!({
                            "path": self.path.clone(),
                            "sample_rate": 16000,
                            "channels": 1,
                            "sample_format": "s16le"
                        })
                    },
                }],
                usage: Usage::default(),
                finish_reason: FinishReason::ToolCalls,
            });
        }
        let result = req
            .items
            .iter()
            .find_map(|item| match item {
                ConversationItem::Tools(round) => round.results.first(),
                _ => None,
            })
            .expect("audio tool result should be present");
        assert!(result.success, "{}", result.output);
        assert!(
            result.output.contains("\"status\": \"ok\""),
            "{}",
            result.output
        );
        Ok(LlmResponse {
            text: Some("audio-ok".into()),
            tool_calls: vec![],
            usage: Usage::default(),
            finish_reason: FinishReason::Stop,
        })
    }
}

#[tokio::test]
async fn analyze_audio_is_dispatched_as_a_builtin_non_shell_tool() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("raw.pcm");
    std::fs::write(&path, vec![0u8; 3200])?;
    let llm = AnalyzeAudioToolLlm {
        calls: AtomicUsize::new(0),
        path: path.display().to_string(),
        omit_metadata: false,
    };
    let exec_calls = Arc::new(AtomicUsize::new(0));
    let cfg = Config::default();
    let outcome = AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &Exec {
            calls: exec_calls.clone(),
        },
        confirmer: &Confirm(false),
    }
    .run("analyze audio")
    .await?;
    assert_eq!(outcome.final_text, "audio-ok");
    assert_eq!(exec_calls.load(Ordering::SeqCst), 0);
    Ok(())
}

struct AudioQuestionConfirmer {
    calls: AtomicUsize,
}

#[async_trait]
impl Confirmer for AudioQuestionConfirmer {
    async fn confirm(
        &self,
        _: &nl2sh::agent::ConfirmationRequest<'_>,
        _: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        Ok(ConfirmationDecision::Reject)
    }

    async fn ask_questions(&self, questions: &[UserQuestion]) -> Result<Option<QuestionAnswers>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            questions
                .iter()
                .map(|question| question.id.as_str())
                .collect::<Vec<_>>(),
            ["sample_rate", "channels", "sample_format"]
        );
        Ok(Some(BTreeMap::from([
            ("sample_rate".into(), "16000".into()),
            ("channels".into(), "1".into()),
            ("sample_format".into(), "s16le".into()),
        ])))
    }
}

#[tokio::test]
async fn raw_audio_missing_metadata_is_collected_and_retried_without_the_model() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("raw.pcm");
    std::fs::write(&path, vec![0u8; 3200])?;
    let llm = AnalyzeAudioToolLlm {
        calls: AtomicUsize::new(0),
        path: path.display().to_string(),
        omit_metadata: true,
    };
    let questions = AudioQuestionConfirmer {
        calls: AtomicUsize::new(0),
    };
    let cfg = Config::default();
    let outcome = AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &Exec {
            calls: Arc::new(AtomicUsize::new(0)),
        },
        confirmer: &questions,
    }
    .run("analyze raw audio")
    .await?;
    assert_eq!(outcome.final_text, "audio-ok");
    assert_eq!(questions.calls.load(Ordering::SeqCst), 1);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 2);
    Ok(())
}

struct CachedAudioJudgeLlm {
    calls: AtomicUsize,
    path: String,
}

#[async_trait]
impl LlmClient for CachedAudioJudgeLlm {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        match call {
            0 => Ok(LlmResponse {
                text: None,
                tool_calls: vec![ToolCall {
                    id: "analyze".into(),
                    name: "analyze_audio".into(),
                    arguments: json!({
                        "path": self.path,
                        "sample_rate": 16000,
                        "channels": 1,
                        "sample_format": "s16le"
                    }),
                }],
                usage: Usage::default(),
                finish_reason: FinishReason::ToolCalls,
            }),
            1 => Ok(LlmResponse {
                text: None,
                tool_calls: vec![ToolCall {
                    id: "judge".into(),
                    name: "judge_audio_quality".into(),
                    arguments: json!({
                        "analysis_path": self.path,
                        "features": {"missing":"status and all real metrics"}
                    }),
                }],
                usage: Usage::default(),
                finish_reason: FinishReason::ToolCalls,
            }),
            2 => {
                assert!(req.tools.is_empty());
                let user = req.items.iter().find_map(|item| match item {
                    ConversationItem::Message(message) if message.role == Role::User => {
                        Some(message.content.as_str())
                    }
                    _ => None,
                });
                assert!(user.is_some_and(|value| value.contains("\"status\":\"ok\"")));
                Ok(LlmResponse {
                    text: Some(r#"{"clarity":{"score":4,"confidence":1},"background_noise":{"score":4,"confidence":1},"distortion":{"score":4,"confidence":1},"loudness":{"score":4,"confidence":1},"continuity":{"score":4,"confidence":1},"usability":{"score":4,"confidence":1},"overall":{"score":4,"confidence":1}}"#.into()),
                    tool_calls: vec![],
                    usage: Usage::default(),
                    finish_reason: FinishReason::Stop,
                })
            }
            _ => Ok(LlmResponse {
                text: Some("judged-from-cache".into()),
                tool_calls: vec![],
                usage: Usage::default(),
                finish_reason: FinishReason::Stop,
            }),
        }
    }
}

#[tokio::test]
async fn audio_judgment_uses_cached_complete_analysis_instead_of_model_copy() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("raw.pcm");
    std::fs::write(&path, vec![0u8; 3200])?;
    let llm = CachedAudioJudgeLlm {
        calls: AtomicUsize::new(0),
        path: path.display().to_string(),
    };
    let cfg = Config::default();
    let outcome = AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &Exec {
            calls: Arc::new(AtomicUsize::new(0)),
        },
        confirmer: &Confirm(false),
    }
    .run("analyze and judge")
    .await?;
    assert_eq!(outcome.final_text, "judged-from-cache");
    assert_eq!(llm.calls.load(Ordering::SeqCst), 4);
    Ok(())
}

struct ReadMutationBarrierLlm {
    requests: AtomicUsize,
    path: String,
}

#[async_trait]
impl LlmClient for ReadMutationBarrierLlm {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
        if self.requests.fetch_add(1, Ordering::SeqCst) == 0 {
            let call = |id: &str, name: &str, arguments| ToolCall {
                id: id.into(),
                name: name.into(),
                arguments,
            };
            Ok(LlmResponse {
                text: None,
                tool_calls: vec![
                    call("before-1", "read_file", json!({"path":self.path})),
                    call("before-2", "read_file", json!({"path":self.path})),
                    call(
                        "write",
                        "apply_patch",
                        json!({"path":self.path,"old_text":"before","new_text":"after"}),
                    ),
                    call("after-1", "read_file", json!({"path":self.path})),
                    call("after-2", "read_file", json!({"path":self.path})),
                ],
                usage: Usage::default(),
                finish_reason: FinishReason::ToolCalls,
            })
        } else {
            let round = request
                .items
                .iter()
                .find_map(|item| match item {
                    ConversationItem::Tools(round) => Some(round),
                    _ => None,
                })
                .expect("completed tool round");
            assert_eq!(round.calls.len(), 5);
            assert_eq!(round.results.len(), 5);
            for (call, result) in round.calls.iter().zip(&round.results) {
                assert_eq!(call.id, result.call_id);
                assert!(result.success, "{}", result.output);
            }
            assert!(round.results[0].output.contains("before"));
            assert!(round.results[1].output.contains("before"));
            assert!(round.results[3].output.contains("after"));
            assert!(round.results[4].output.contains("after"));
            Ok(LlmResponse {
                text: Some("done".into()),
                tool_calls: vec![],
                usage: Usage::default(),
                finish_reason: FinishReason::Stop,
            })
        }
    }
}

#[tokio::test]
async fn parallel_reads_preserve_mutation_barriers_order_and_audit_correlation() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("data.txt");
    std::fs::write(&path, "before")?;
    let cfg = Config {
        source: Some(directory.path().join("config.toml")),
        ..Config::default()
    };
    let llm = ReadMutationBarrierLlm {
        requests: AtomicUsize::new(0),
        path: path.to_string_lossy().into(),
    };
    let executor = Exec {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let outcome = AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &executor,
        confirmer: &Confirm(true),
    }
    .run("read then change then read")
    .await?;
    assert_eq!(outcome.tool_calls, 5);
    let records = std::fs::read_to_string(directory.path().join("nl2sh.log"))?;
    let records: Vec<serde_json::Value> = records
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert_eq!(records.len(), 5);
    let task = &records[0]["task_id"];
    let mut requests = std::collections::HashSet::new();
    for record in &records {
        assert_eq!(&record["task_id"], task);
        assert_eq!(record["result"], "success");
        assert!(requests.insert(record["request_id"].as_str().expect("request identifier")));
    }
    assert_eq!(records[2]["tool"], "apply_patch");
    assert_eq!(records[2]["approval"], "approved_once");
    assert!(!records
        .iter()
        .any(|event| event.to_string().contains(&llm.path)));
    Ok(())
}

#[tokio::test]
async fn agent_prepared_grant_does_not_inherit_task_approval_after_exhaustion() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("target.txt");
    std::fs::write(&path, "before")?;
    let config_path = directory.path().join("config.toml");
    let config = Config {
        source: Some(config_path.clone()),
        approval_grants: vec![nl2sh::config::ApprovalGrantConfig {
            id: "patch-once".into(),
            tool: Some("apply_patch".into()),
            package: None,
            max_risk: "mutating".into(),
            expires_at: None,
            uses: Some(1),
        }],
        ..Config::default()
    };
    nl2sh::config::save_config(&config_path, &config)?;
    let executor = Exec {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let confirmer = Confirm(false);
    for expected_success in [true, false] {
        std::fs::write(&path, "before")?;
        let llm = StructuredPatchLlm {
            calls: AtomicUsize::new(0),
            path: path.to_string_lossy().into_owned(),
            expected_success,
        };
        let runner = AgentRunner {
            config: &config,
            llm: &llm,
            executor: &executor,
            confirmer: &confirmer,
        };
        runner.run("patch the file".into()).await?;
        assert_eq!(
            std::fs::read_to_string(&path)?,
            if expected_success { "after" } else { "before" }
        );
    }
    Ok(())
}

struct BackgroundLlm(AtomicUsize);
#[async_trait]
impl LlmClient for BackgroundLlm {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(LlmResponse {
                text: None,
                tool_calls: vec![ToolCall {
                    id: "background".into(),
                    name: "execute_shell_command".into(),
                    arguments: json!({"command":"sleep 30", "background":true, "background_timeout_secs":60}),
                }],
                usage: Usage::default(),
                finish_reason: FinishReason::ToolCalls,
            })
        } else {
            let result = request
                .items
                .iter()
                .find_map(|item| match item {
                    ConversationItem::Tools(round) => round.results.first(),
                    _ => None,
                })
                .ok_or_else(|| anyhow::anyhow!("background evidence missing"))?;
            assert!(result.success);
            let value: serde_json::Value = serde_json::from_str(&result.output)?;
            assert_eq!(value["status"], "started");
            assert_eq!(value["child_id"], "00000000-0000-4000-8000-000000000001");
            assert!(value.get("exit_code").is_none());
            Ok(LlmResponse {
                text: Some("capture started".into()),
                tool_calls: vec![],
                usage: Usage::default(),
                finish_reason: FinishReason::Stop,
            })
        }
    }
}
struct BackgroundExec;
#[async_trait]
impl CommandExecutor for BackgroundExec {
    async fn execute(&self, _: &str, _: bool, _: bool) -> Result<ExecutionResult> {
        anyhow::bail!("background must not call foreground execution")
    }
    async fn spawn_background(&self, command: &str, root: bool, seconds: u64) -> Result<String> {
        assert_eq!(command, "sleep 30");
        assert!(!root);
        assert_eq!(seconds, 60);
        Ok("00000000-0000-4000-8000-000000000001".into())
    }
}
#[tokio::test]
async fn agent_background_start_returns_real_handle_without_foreground_wait_or_exit_claim(
) -> Result<()> {
    let config = Config::default();
    let llm = BackgroundLlm(AtomicUsize::new(0));
    let result = AgentRunner {
        config: &config,
        llm: &llm,
        executor: &BackgroundExec,
        confirmer: &Confirm(false),
    }
    .run("start capture")
    .await?;
    assert_eq!(result.final_text, "capture started");
    assert_eq!(llm.0.load(Ordering::SeqCst), 2);
    Ok(())
}
