use anyhow::Result;
use async_trait::async_trait;
use nl2sh::{
    agent::{AgentRunner, ConfirmationDecision, Confirmer, LimitType},
    config::Config,
    llm::{
        ConversationItem, ConversationMessage, FinishReason, LlmClient, LlmRequest, LlmResponse,
        Role, ToolCall, Usage,
    },
    security::SecurityAssessment,
    shell::{CommandExecutor, ExecutionResult},
};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
struct MockLlm {
    calls: AtomicUsize,
    command: &'static str,
}

struct AlwaysToolLlm {
    calls: AtomicUsize,
    command: &'static str,
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
    async fn confirm(&self, _: &str, _: &SecurityAssessment) -> Result<ConfirmationDecision> {
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
    async fn confirm(&self, _: &str, _: &SecurityAssessment) -> Result<ConfirmationDecision> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ConfirmationDecision::ApproveForTask)
    }
}
#[async_trait]
impl Confirmer for EditThenReject {
    async fn confirm(
        &self,
        _: &str,
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
