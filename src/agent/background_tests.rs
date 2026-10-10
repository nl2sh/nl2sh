use super::*;
use crate::llm::{FinishReason, LlmResponse, ToolCall};
use crate::tools::{PreparedToolCall, Tool, ToolOutput};
use async_trait::async_trait;
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

struct DeferredTool {
    start: bool,
    mutation: bool,
    fail: bool,
    delay: Duration,
    executed: Arc<AtomicUsize>,
}
impl DeferredTool {
    fn name(&self) -> &'static str {
        if self.start {
            "read_file"
        } else if self.mutation {
            "apply_patch"
        } else {
            "list_dir"
        }
    }
}
#[async_trait]
impl Tool for DeferredTool {
    fn metadata(&self) -> &'static ToolMetadata {
        crate::tools::builtin_descriptors()
            .iter()
            .find(|tool| tool.name == self.name())
            .expect("test descriptor")
    }
    async fn prepare(&self, _: &ToolContext<'_>, _: serde_json::Value) -> Result<PreparedToolCall> {
        if !self.start && self.fail {
            bail!("test preparation failure")
        }
        Ok(PreparedToolCall::operation(
            "test mutation preview".into(),
            Box::new(DeferredTool {
                start: self.start,
                mutation: self.mutation,
                fail: false,
                delay: self.delay,
                executed: self.executed.clone(),
            }),
        ))
    }
}
#[async_trait]
impl PreparedExecution for DeferredTool {
    async fn execute(self: Box<Self>, ctx: &mut ToolContext<'_>) -> Result<ToolOutput> {
        if self.start {
            let scheduled = ctx
                .runtime
                .as_deref_mut()
                .context("test runtime")?
                .background
                .schedule(
                    self.delay,
                    ToolCall {
                        id: String::new(),
                        name: if self.mutation {
                            "apply_patch"
                        } else {
                            "list_dir"
                        }
                        .into(),
                        arguments: json!({}),
                    },
                )?;
            Ok(ToolOutput::success(
                json!({"background_scheduled":scheduled}).to_string(),
            ))
        } else {
            self.executed.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutput::success("completed background evidence".into()))
        }
    }
}
struct Model {
    requests: AtomicUsize,
    expected_success: bool,
}
#[async_trait]
impl LlmClient for Model {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
        let first = self.requests.fetch_add(1, Ordering::SeqCst) == 0;
        if !first {
            let background: Vec<_> = request
                .items
                .iter()
                .filter_map(|item| match item {
                    ConversationItem::Tools(round)
                        if round.calls[0].id.starts_with("nl2sh-background-") =>
                    {
                        Some(round)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                background.len(),
                1,
                "model must receive the completed continuation exactly once"
            );
            assert_eq!(background[0].results[0].success, self.expected_success);
        }
        Ok(LlmResponse {
            text: (!first).then(|| "finished after actual background evidence".into()),
            tool_calls: if first {
                vec![ToolCall {
                    id: "start".into(),
                    name: "read_file".into(),
                    arguments: json!({}),
                }]
            } else {
                vec![]
            },
            usage: Usage::default(),
            finish_reason: FinishReason::Stop,
        })
    }
}
struct Boundary {
    approvals: AtomicUsize,
    approve: bool,
}
#[async_trait]
impl Confirmer for Boundary {
    async fn confirm(
        &self,
        preview: &crate::agent::ConfirmationRequest<'_>,
        assessment: &crate::security::SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        let preview = preview.preview;
        assert_eq!(preview, "test mutation preview");
        assert!(assessment.requires_confirmation);
        self.approvals.fetch_add(1, Ordering::SeqCst);
        Ok(if self.approve {
            ConfirmationDecision::Approve
        } else {
            ConfirmationDecision::Reject
        })
    }
}
#[async_trait]
impl CommandExecutor for Boundary {
    async fn execute(&self, _: &str, _: bool, _: bool) -> Result<crate::shell::ExecutionResult> {
        bail!("background must not execute raw shell")
    }
}
#[derive(Default)]
struct Sink {
    phases: Mutex<Vec<&'static str>>,
    cancel: Option<watch::Sender<bool>>,
}
impl TextDeltaSink for Sink {
    fn delta(&self, _: &str) {}
    fn agent_activity(&self, phase: &'static str, _: Option<&str>) {
        self.phases.lock().expect("phases").push(phase);
        if phase == "background" {
            if let Some(cancel) = &self.cancel {
                cancel.send_replace(true);
            }
        }
    }
}
fn registry(
    delay: Duration,
    mutation: bool,
    fail: bool,
    executed: &Arc<AtomicUsize>,
) -> ToolRegistry {
    ToolRegistry::from_test_adapters(vec![
        Box::new(DeferredTool {
            start: true,
            mutation,
            fail: false,
            delay,
            executed: executed.clone(),
        }),
        Box::new(DeferredTool {
            start: false,
            mutation,
            fail,
            delay,
            executed: executed.clone(),
        }),
    ])
}

#[tokio::test]
async fn background_resumes_once_with_real_results_and_lifecycle_audit() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let cfg = Config {
        source: Some(temp.path().join("config.toml")),
        ..Config::default()
    };
    let log = crate::history::HistoryLog::open(
        &cfg.source.clone().expect("source"),
        &cfg.history_log_file,
    )?;
    let model = Model {
        requests: AtomicUsize::new(0),
        expected_success: true,
    };
    let boundary = Boundary {
        approvals: AtomicUsize::new(0),
        approve: true,
    };
    let executed = Arc::new(AtomicUsize::new(0));
    let sink = Sink::default();
    let runner = AgentRunner {
        config: &cfg,
        llm: &model,
        executor: &boundary,
        confirmer: &boundary,
    };
    let result = crate::audit::AuditContext::new(&cfg, "test", None)
        .scope(runner.run_scoped_with_registry(
            "trace",
            &[],
            Some(&sink),
            None,
            Some(registry(Duration::from_millis(20), false, false, &executed)),
        ))
        .await?;
    assert_eq!(executed.load(Ordering::SeqCst), 1);
    assert_eq!(result.steps, 2);
    assert_eq!(result.tool_calls, 2);
    assert_eq!(result.stats.limit_reached, None);
    assert!(sink.phases.lock().expect("phases").contains(&"background"));
    let events = std::fs::read_to_string(log.path())?;
    for status in ["scheduled", "waiting", "resuming", "completed"] {
        assert!(
            events.contains(&format!("\\\"status\\\":\\\"{status}\\\"")),
            "missing {status}: {events}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn background_mutations_still_require_approval_and_rejection_prevents_execution() -> Result<()>
{
    for approve in [true, false] {
        let cfg = Config::default();
        let model = Model {
            requests: AtomicUsize::new(0),
            expected_success: approve,
        };
        let boundary = Boundary {
            approvals: AtomicUsize::new(0),
            approve,
        };
        let executed = Arc::new(AtomicUsize::new(0));
        AgentRunner {
            config: &cfg,
            llm: &model,
            executor: &boundary,
            confirmer: &boundary,
        }
        .run_scoped_with_registry(
            "trace",
            &[],
            None,
            None,
            Some(registry(Duration::ZERO, true, false, &executed)),
        )
        .await?;
        assert_eq!(boundary.approvals.load(Ordering::SeqCst), 1);
        assert_eq!(executed.load(Ordering::SeqCst), usize::from(approve));
    }
    Ok(())
}

#[tokio::test]
async fn background_preparation_failure_is_returned_to_model_without_execution() -> Result<()> {
    let cfg = Config::default();
    let model = Model {
        requests: AtomicUsize::new(0),
        expected_success: false,
    };
    let boundary = Boundary {
        approvals: AtomicUsize::new(0),
        approve: true,
    };
    let executed = Arc::new(AtomicUsize::new(0));
    AgentRunner {
        config: &cfg,
        llm: &model,
        executor: &boundary,
        confirmer: &boundary,
    }
    .run_scoped_with_registry(
        "trace",
        &[],
        None,
        None,
        Some(registry(Duration::ZERO, false, true, &executed)),
    )
    .await?;
    assert_eq!(executed.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn cancellation_during_background_wait_preserves_start_evidence_and_never_resumes(
) -> Result<()> {
    let cfg = Config::default();
    let model = Model {
        requests: AtomicUsize::new(0),
        expected_success: true,
    };
    let boundary = Boundary {
        approvals: AtomicUsize::new(0),
        approve: true,
    };
    let executed = Arc::new(AtomicUsize::new(0));
    let (cancel, signal) = watch::channel(false);
    let sink = Sink {
        cancel: Some(cancel),
        ..Sink::default()
    };
    let error = AgentRunner {
        config: &cfg,
        llm: &model,
        executor: &boundary,
        confirmer: &boundary,
    }
    .run_scoped_with_registry(
        "trace",
        &[],
        Some(&sink),
        Some(signal),
        Some(registry(Duration::from_secs(30), false, false, &executed)),
    )
    .await
    .expect_err("cancelled");
    let failure = error
        .downcast_ref::<AgentRunFailure>()
        .context("transcript failure")?;
    assert_eq!(failure.transcript().len(), 2);
    assert_eq!(executed.load(Ordering::SeqCst), 0);
    assert_eq!(model.requests.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn background_respects_tool_step_and_time_budgets() -> Result<()> {
    for (cfg, delay, limit) in [
        (
            Config {
                max_tool_calls: 1,
                ..Config::default()
            },
            Duration::ZERO,
            LimitType::ToolCalls,
        ),
        (
            Config {
                max_agent_steps: 1,
                ..Config::default()
            },
            Duration::ZERO,
            LimitType::Step,
        ),
        (
            Config {
                max_task_execution_time_secs: 1,
                ..Config::default()
            },
            Duration::from_secs(2),
            LimitType::ExecutionTime,
        ),
    ] {
        let model = Model {
            requests: AtomicUsize::new(0),
            expected_success: true,
        };
        let boundary = Boundary {
            approvals: AtomicUsize::new(0),
            approve: true,
        };
        let executed = Arc::new(AtomicUsize::new(0));
        let result = AgentRunner {
            config: &cfg,
            llm: &model,
            executor: &boundary,
            confirmer: &boundary,
        }
        .run_scoped_with_registry(
            "trace",
            &[],
            None,
            None,
            Some(registry(delay, false, false, &executed)),
        )
        .await?;
        assert_eq!(result.stats.limit_reached, Some(limit));
        assert_eq!(executed.load(Ordering::SeqCst), 0);
        assert_eq!(model.requests.load(Ordering::SeqCst), 1);
    }
    Ok(())
}
