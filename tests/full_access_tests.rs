//! Keep the process-global development policy isolated from all other test binaries.
use anyhow::Result;
use async_trait::async_trait;
use nl2sh::{
    agent::{confirm_assessed, ConfirmationDecision, ConfirmationRequest, Confirmer},
    config::{Config, ConfirmPolicy, ExecuteUserMode},
    security::{access, assess, PrivilegeBroker, RiskLevel, SecurityAssessment},
    shell::{CommandExecutor, ExecutionResult},
    tools::runtime::invoke,
};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};

struct AgentModel(AtomicBool);
#[async_trait]
impl nl2sh::llm::LlmClient for AgentModel {
    async fn complete(&self, request: nl2sh::llm::LlmRequest) -> Result<nl2sh::llm::LlmResponse> {
        use nl2sh::llm::{ConversationItem, FinishReason, LlmResponse, ToolCall, Usage};
        assert!(request.items.iter().any(|item| matches!(item,
            ConversationItem::Message(message) if message.content.contains("owner explicitly enabled Full access"))));
        let first = !self.0.swap(true, Ordering::Relaxed);
        Ok(LlmResponse {
            text: (!first).then(|| "done".into()),
            tool_calls: if first {
                vec![ToolCall {
                    id: "critical-root".into(),
                    name: "execute_shell_command".into(),
                    arguments: json!({"command":"rm -rf /system","reason":"mock only"}),
                }]
            } else {
                vec![]
            },
            usage: Usage::default(),
            finish_reason: if first {
                FinishReason::ToolCalls
            } else {
                FinishReason::Stop
            },
        })
    }
}

struct Refuse {
    cancelled: bool,
}
#[async_trait]
impl Confirmer for Refuse {
    fn approval_cancelled(&self) -> bool {
        self.cancelled
    }
    async fn confirm(
        &self,
        _: &ConfirmationRequest<'_>,
        _: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        Ok(ConfirmationDecision::Reject)
    }
}

struct RootRecorder;
#[async_trait]
impl CommandExecutor for RootRecorder {
    async fn execute(&self, command: &str, root: bool, _: bool) -> Result<ExecutionResult> {
        // No destructive command is ever passed to the operating system.
        assert_eq!(command, "rm -rf /system");
        assert!(root);
        Ok(ExecutionResult {
            stdout: "simulated root command".into(),
            stderr: String::new(),
            exit_code: Some(0),
            timed_out: false,
            interrupted: false,
        })
    }
}

#[tokio::test]
async fn full_access_approves_all_risks_retains_binding_cancellation_and_audit() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let config = Config {
        execute_user_mode: ExecuteUserMode::Root,
        source: Some(dir.path().join("config.toml")),
        ..Config::default()
    };
    let request = ConfirmationRequest::shell("rm -rf /system");
    let assessment = assess(request.preview, &config);
    assert_eq!(assessment.risk_level, RiskLevel::Critical);
    assert!(assessment.requires_root && assessment.requires_double_confirmation);
    assert!(!access::full_access_enabled());
    let fallback = Refuse { cancelled: false };
    assert_eq!(
        confirm_assessed(&config, &fallback, &request, &assessment).await?,
        ConfirmationDecision::Reject
    );
    assert!(PrivilegeBroker::authorize(request.preview, &assessment, &config, None).is_err());
    let mut saved_policy = config.clone();
    saved_policy.execute_confirm_policy = ConfirmPolicy::FullAccess;
    assert!(access::full_access_for(&saved_policy));
    assert!(!access::full_access_for(&config));
    assert_eq!(
        confirm_assessed(&saved_policy, &fallback, &request, &assessment).await?,
        ConfirmationDecision::ApproveByFullAccess
    );
    assert_eq!(
        confirm_assessed(
            &saved_policy,
            &Refuse { cancelled: true },
            &request,
            &assessment
        )
        .await?,
        ConfirmationDecision::Reject
    );
    let outcome = nl2sh::agent::AgentRunner {
        config: &saved_policy,
        executor: &RootRecorder,
        confirmer: &fallback,
        llm: &AgentModel(AtomicBool::new(false)),
    }
    .run("mock config-selected root operation")
    .await?;
    assert_eq!(outcome.tool_calls, 1);
    saved_policy.execute_confirm_policy = ConfirmPolicy::RiskOnly;
    assert_eq!(
        confirm_assessed(&saved_policy, &fallback, &request, &assessment).await?,
        ConfirmationDecision::Reject
    );
    access::enable_full_access();
    assert!(
        access::full_access_for(&saved_policy),
        "CLI override takes precedence over the saved ordinary policy"
    );
    for risk in [
        RiskLevel::ReadOnly,
        RiskLevel::Mutating,
        RiskLevel::Dangerous,
        RiskLevel::Critical,
    ] {
        let assessment = SecurityAssessment::from_policy(risk, vec![], true, "test".into(), true);
        assert_eq!(
            confirm_assessed(&config, &fallback, &request, &assessment).await?,
            ConfirmationDecision::ApproveByFullAccess
        );
        assert_eq!(
            confirm_assessed(&config, &Refuse { cancelled: true }, &request, &assessment).await?,
            ConfirmationDecision::Reject
        );
    }
    assert!(PrivilegeBroker::authorize(request.preview, &assessment, &config, Some("id")).is_err());
    let result = invoke(
        &config,
        &RootRecorder,
        &fallback,
        "execute_shell_command",
        json!({"command":request.preview,"reason":"test mocked critical root execution"}),
    )
    .await?;
    assert!(result.success, "{}", result.output);
    assert!(result.output.contains("risk=Critical root=true"));
    let target = dir.path().join("patched.txt");
    let result = invoke(
        &config,
        &RootRecorder,
        &fallback,
        "apply_patch",
        json!({"path":target,"old_text":"","new_text":"approved"}),
    )
    .await?;
    assert!(result.success, "{}", result.output);
    assert_eq!(std::fs::read_to_string(&target)?, "approved");
    let model = AgentModel(AtomicBool::new(false));
    let outcome = nl2sh::agent::AgentRunner {
        config: &config,
        executor: &RootRecorder,
        confirmer: &fallback,
        llm: &model,
    }
    .run("mock root operation")
    .await?;
    assert_eq!(outcome.tool_calls, 1);
    assert_eq!(outcome.final_text, "done");
    let logs = std::fs::read_to_string(dir.path().join("nl2sh.log"))?;
    assert!(logs.contains("approved_by_full_access"));
    assert!(logs.contains("Critical"));
    Ok(())
}
