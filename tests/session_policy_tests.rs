//! Live approval changes must affect the pending call without changing its bound assessment.
use anyhow::Result;
use async_trait::async_trait;
use nl2sh::{
    agent::{
        confirm_assessed, confirmation_required, ConfirmationDecision, ConfirmationRequest,
        Confirmer, SessionConfirmationPolicy,
    },
    config::{Config, ConfirmPolicy, ExecuteUserMode, SecurityLevel},
    security::{assess, SecurityAssessment},
    shell::{CommandExecutor, ExecutionResult},
    tools::runtime::invoke,
};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct OwnerUi {
    policy: SessionConfirmationPolicy,
    select: Option<ConfirmPolicy>,
    calls: AtomicUsize,
    cancelled: AtomicBool,
    cancel_on_change: bool,
}
impl OwnerUi {
    fn new(select: Option<ConfirmPolicy>) -> Self {
        Self {
            policy: SessionConfirmationPolicy::default(),
            select,
            calls: AtomicUsize::new(0),
            cancelled: AtomicBool::new(false),
            cancel_on_change: false,
        }
    }
}
#[async_trait]
impl Confirmer for OwnerUi {
    fn confirmation_policy_override(&self) -> Option<ConfirmPolicy> {
        self.policy.get()
    }
    fn approval_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    async fn confirm(
        &self,
        _: &ConfirmationRequest<'_>,
        _: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            if let Some(policy) = self.select {
                self.policy.set(policy);
                if self.cancel_on_change {
                    self.cancelled.store(true, Ordering::Release);
                }
                return Ok(ConfirmationDecision::SetPolicy(policy));
            }
        }
        Ok(ConfirmationDecision::Reject)
    }
}
struct MockRoot(AtomicUsize);
#[async_trait]
impl CommandExecutor for MockRoot {
    async fn execute(&self, command: &str, root: bool, _: bool) -> Result<ExecutionResult> {
        assert_eq!(command, "rm -rf /system");
        assert!(root);
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ExecutionResult {
            stdout: "mock only".into(),
            stderr: String::new(),
            exit_code: Some(0),
            timed_out: false,
            interrupted: false,
        })
    }
}

#[tokio::test]
async fn selecting_full_access_executes_pending_and_later_critical_root_calls_in_only_this_session(
) -> Result<()> {
    let cfg = Config {
        execute_user_mode: ExecuteUserMode::Root,
        ..Config::default()
    };
    let ui = OwnerUi::new(Some(ConfirmPolicy::FullAccess));
    let executor = MockRoot(AtomicUsize::new(0));
    for _ in 0..2 {
        let result = invoke(
            &cfg,
            &executor,
            &ui,
            "execute_shell_command",
            json!({"command":"rm -rf /system"}),
        )
        .await?;
        assert!(result.success, "{}", result.output);
    }
    assert_eq!(ui.calls.load(Ordering::SeqCst), 1);
    assert_eq!(executor.0.load(Ordering::SeqCst), 2);
    let other = OwnerUi::new(None);
    let result = invoke(
        &cfg,
        &executor,
        &other,
        "execute_shell_command",
        json!({"command":"rm -rf /system"}),
    )
    .await?;
    assert!(!result.success);
    assert_eq!(executor.0.load(Ordering::SeqCst), 2);
    assert_eq!(cfg.execute_confirm_policy, ConfirmPolicy::RiskOnly);
    Ok(())
}

#[tokio::test]
async fn ordinary_choices_recheck_pending_reads_but_preserve_mutation_root_and_strict_approval(
) -> Result<()> {
    let cfg = Config {
        execute_confirm_policy: ConfirmPolicy::Always,
        ..Config::default()
    };
    let request = ConfirmationRequest::shell("printf hello");
    for policy in [ConfirmPolicy::RiskOnly, ConfirmPolicy::Never] {
        let ui = OwnerUi::new(Some(policy));
        assert_eq!(
            confirm_assessed(&cfg, &ui, &request, &assess(request.preview, &cfg)).await?,
            ConfirmationDecision::ApproveBySessionPolicy
        );
        for command in [
            "touch /tmp/session-policy-test",
            "su -c id",
            "rm -rf /system",
        ] {
            assert_eq!(
                confirm_assessed(
                    &cfg,
                    &ui,
                    &ConfirmationRequest::shell(command),
                    &assess(command, &cfg)
                )
                .await?,
                ConfirmationDecision::Reject
            );
        }
    }
    let strict = Config {
        security_level: SecurityLevel::Strict,
        ..cfg.clone()
    };
    let ui = OwnerUi::new(Some(ConfirmPolicy::RiskOnly));
    assert_eq!(
        confirm_assessed(&strict, &ui, &request, &assess(request.preview, &strict)).await?,
        ConfirmationDecision::Reject
    );
    let ordinary = Config::default();
    ui.policy.set(ConfirmPolicy::Always);
    assert!(confirmation_required(
        &ordinary,
        &ui,
        &assess(request.preview, &ordinary)
    ));
    let full = Config {
        execute_confirm_policy: ConfirmPolicy::FullAccess,
        ..ordinary
    };
    assert_eq!(
        confirm_assessed(&full, &ui, &request, &assess(request.preview, &full)).await?,
        ConfirmationDecision::Reject
    );
    Ok(())
}

#[tokio::test]
async fn cancellation_during_a_policy_change_never_authorizes_the_waiting_operation() -> Result<()>
{
    let cfg = Config::default();
    let mut ui = OwnerUi::new(Some(ConfirmPolicy::FullAccess));
    ui.cancel_on_change = true;
    let request = ConfirmationRequest::shell("rm -rf /system");
    assert_eq!(
        confirm_assessed(&cfg, &ui, &request, &assess(request.preview, &cfg)).await?,
        ConfirmationDecision::Reject
    );
    Ok(())
}
