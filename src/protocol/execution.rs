//! Protocol-neutral execution; transport code cannot grant an approval.
use super::approval::LocalApprovalConfirmer;
use crate::{
    agent::{AgentRunner, ConfirmationDecision, Confirmer},
    config::{self, Config, ConfirmPolicy, SecurityLevel},
    llm::{build_client, ConversationItem, TextDeltaSink},
    security::SecurityAssessment,
    sessions::SessionStore,
    shell::{OutputSink, ShellExecutor},
    tools::{android::environment::inspect_environment, available_tools, runtime::invoke},
};
use anyhow::{bail, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use tokio::sync::watch;

pub(super) fn load(path: &Path) -> Result<Config> {
    let mut cfg = config::load_or_default_unvalidated(path)?;
    cfg.validate_runtime()?;
    if cfg.security_level == SecurityLevel::Unsafe {
        cfg.security_level = SecurityLevel::Balanced;
    }
    if cfg.execute_confirm_policy == ConfirmPolicy::Never {
        cfg.execute_confirm_policy = ConfirmPolicy::RiskOnly;
    }
    // Protocol stdout is reserved; external tools always use captured execution.
    cfg.enable_pty = false;
    Ok(cfg)
}

struct ProtocolConfirmer {
    local: LocalApprovalConfirmer,
    auto: bool,
    cancel: watch::Receiver<bool>,
}
#[async_trait]
impl Confirmer for ProtocolConfirmer {
    fn approval_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }
    fn audit_source(&self) -> &'static str {
        if self.auto {
            "protocol_explicit_auto_approve"
        } else {
            "protocol_terminal"
        }
    }
    async fn confirm(
        &self,
        request: &crate::agent::ConfirmationRequest<'_>,
        assessment: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        if *self.cancel.borrow() {
            return Ok(ConfirmationDecision::Reject);
        }
        if self.auto {
            return Ok(ConfirmationDecision::Approve);
        }
        let mut cancel = self.cancel.clone();
        tokio::select! {
            biased;
            _ = async { while !*cancel.borrow() { if cancel.changed().await.is_err() { break; } } } => Ok(ConfirmationDecision::Reject),
            decision = self.local.confirm(request, assessment) => decision,
        }
    }
}
struct Silent;
impl OutputSink for Silent {
    fn stdout(&self, _: &str) {}
    fn stderr(&self, _: &str) {}
}
impl TextDeltaSink for Silent {
    fn delta(&self, _: &str) {}
}

pub(super) async fn inspect(path: &Path) -> Result<Value> {
    let executor = ShellExecutor::new(load(path)?);
    let mut environment: Value = serde_json::from_str(&inspect_environment(&executor).await?)?;
    environment["build_identity"] = crate::build_identity::running_identity().await?;
    Ok(environment)
}
pub(super) async fn tools(path: &Path) -> Result<Value> {
    let cfg = load(path)?;
    let executor = ShellExecutor::new(cfg.clone());
    Ok(json!({"tools":available_tools(&cfg, &executor).await}))
}
pub(super) async fn invoke_tool(
    path: &Path,
    name: &str,
    arguments: Value,
    cancel: watch::Receiver<bool>,
) -> Result<Value> {
    if name.is_empty() || name.len() > 128 || !arguments.is_object() {
        bail!("invalid tool invocation");
    }
    if serde_json::to_vec(&arguments)?.len() > 16 * 1024 {
        bail!("tool arguments exceed 16 KiB");
    }
    if *cancel.borrow() {
        bail!("task cancelled before execution");
    }
    let cfg = load(path)?;
    let executor = ShellExecutor::new(cfg.clone())
        .with_output(std::sync::Arc::new(Silent))
        .with_cancel(cancel.clone());
    let confirmer = ProtocolConfirmer {
        local: LocalApprovalConfirmer::new(path)?,
        auto: cfg.protocol_auto_approve,
        cancel,
    };
    Ok(serde_json::to_value(
        invoke(&cfg, &executor, &confirmer, name, arguments).await?,
    )?)
}
pub(super) async fn ask(
    path: &Path,
    session: &str,
    message: &str,
    cancel: watch::Receiver<bool>,
) -> Result<Value> {
    if message.trim().is_empty() || message.len() > 8192 {
        bail!("message must contain 1–8192 UTF-8 bytes");
    }
    let cfg = load(path)?;
    if !cfg.provider_is_configured() {
        bail!("model provider is not configured");
    }
    let store = SessionStore::open(path)?;
    let history = match store.load(
        session,
        cfg.max_context_turns,
        cfg.model_tool_output_max_bytes,
    ) {
        Ok(history) => history,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|cause| cause.kind() == std::io::ErrorKind::NotFound) =>
        {
            Vec::new()
        }
        Err(error) => return Err(error),
    };
    let llm = build_client(&cfg)?;
    let executor = ShellExecutor::new(cfg.clone())
        .with_output(std::sync::Arc::new(Silent))
        .with_cancel(cancel.clone());
    let confirmer = ProtocolConfirmer {
        local: LocalApprovalConfirmer::new(path)?,
        auto: cfg.protocol_auto_approve,
        cancel: cancel.clone(),
    };
    let runner = AgentRunner {
        config: &cfg,
        llm: &llm,
        executor: &executor,
        confirmer: &confirmer,
    };
    let outcome = runner
        .run_with_history_streaming_cancellable(message.into(), history.clone(), &Silent, cancel)
        .await?;
    let evidence = tool_evidence(&outcome.transcript);
    let failed_tools: Vec<_> = outcome
        .transcript
        .iter()
        .filter_map(|item| match item {
            ConversationItem::Tools(round) => Some(&round.results),
            _ => None,
        })
        .flatten()
        .filter(|result| !result.success)
        .take(8)
        .map(|result| crate::limits::truncate_text(&result.output, 512))
        .collect();
    let mut turns = history;
    turns.push(outcome.transcript);
    store.save_redacted(
        session,
        &turns,
        cfg.model_tool_output_max_bytes,
        &[
            cfg.api_key,
            cfg.proxy_password,
            cfg.protocol_token,
            cfg.ima_api_key,
            cfg.jev_api_key,
        ],
    )?;
    Ok(
        json!({"session": session, "answer": outcome.final_text, "steps": outcome.steps, "tool_calls": outcome.tool_calls, "failed_tools": failed_tools, "evidence": evidence}),
    )
}

// Evidence is derived from actual ToolRounds, never parsed from model prose.
fn tool_evidence(transcript: &[ConversationItem]) -> Value {
    let mut observations = Vec::new();
    let mut total = 0usize;
    let mut missing = 0usize;
    for item in transcript {
        let ConversationItem::Tools(round) = item else {
            continue;
        };
        for call in &round.calls {
            total += 1;
            let result = round
                .results
                .iter()
                .find(|result| result.call_id == call.id);
            if result.is_none() {
                missing += 1;
            }
            if observations.len() == 64 {
                continue;
            }
            observations.push(json!({
                "id": observations.len(), "source": "device_tool",
                "call_id": call.id, "tool": call.name,
                "success": result.map(|result| result.success),
                "execution_status": if result.is_none() { "missing" } else if result.is_some_and(|result| result.success) { "succeeded" } else { "failed" },
                "output": result.map(|result| crate::limits::truncate_text(&result.output, 4096)),
                "output_status": result.and_then(|result| serde_json::from_str::<Value>(&result.output).ok())
                    .and_then(|output| output.get("status").and_then(Value::as_str).map(str::to_owned)),
                "output_truncated": result.is_some_and(|result| result.output.len() > 4096 || result.output.contains(crate::limits::TRUNCATION_LABEL)),
            }));
        }
    }
    json!({"schema_version": 1, "observations": observations,
        "total_calls": total, "missing_results": missing,
        "observations_truncated": total > 64,
        "limitations": ["Outputs are bounded model-visible tool results; success does not establish completeness or a root cause. Agent answer contains unverified analysis."]})
}

#[cfg(test)]
mod evidence_tests {
    use super::*;
    use crate::llm::{ToolCall, ToolResult, ToolRound};

    #[test]
    fn actual_results_and_missing_calls_are_distinct_from_agent_success() {
        let call = |id: &str| ToolCall {
            id: id.into(),
            name: "read_file".into(),
            arguments: json!({}),
        };
        let evidence = tool_evidence(&[ConversationItem::Tools(ToolRound {
            calls: vec![call("failed"), call("missing")],
            results: vec![ToolResult {
                call_id: "failed".into(),
                output: "denied".into(),
                success: false,
                attachments: vec![],
            }],
        })]);
        assert_eq!(evidence["observations"][0]["success"], false);
        assert_eq!(evidence["observations"][1]["execution_status"], "missing");
        assert_eq!(evidence["missing_results"], 1);
    }

    #[test]
    fn evidence_limits_preserve_truncation_and_total_call_count() {
        let calls = (0..65)
            .map(|index| ToolCall {
                id: index.to_string(),
                name: "read_file".into(),
                arguments: json!({}),
            })
            .collect::<Vec<_>>();
        let results = calls
            .iter()
            .map(|call| ToolResult {
                call_id: call.id.clone(),
                output: "界".repeat(5000),
                success: true,
                attachments: vec![],
            })
            .collect();
        let evidence = tool_evidence(&[ConversationItem::Tools(ToolRound { calls, results })]);
        assert_eq!(evidence["total_calls"], 65);
        assert_eq!(evidence["observations_truncated"], true);
        assert_eq!(evidence["observations"].as_array().map(Vec::len), Some(64));
        assert!(evidence["observations"]
            .as_array()
            .is_some_and(
                |items| items.iter().all(|item| item["output_truncated"] == true
                    && item["output"]
                        .as_str()
                        .is_some_and(|text| text.len() <= 4096))
            ));
    }

    #[test]
    fn successful_call_can_return_partial_evidence() {
        let evidence = tool_evidence(&[ConversationItem::Tools(ToolRound {
            calls: vec![ToolCall {
                id: "partial".into(),
                name: "inspect_android_environment".into(),
                arguments: json!({}),
            }],
            results: vec![ToolResult {
                call_id: "partial".into(),
                output: json!({"status":"partial"}).to_string(),
                success: true,
                attachments: vec![],
            }],
        })]);
        assert_eq!(evidence["observations"][0]["success"], true);
        assert_eq!(evidence["observations"][0]["output_status"], "partial");
    }
}

#[cfg(test)]
mod grant_execution_tests {
    use super::*;
    use crate::config::ApprovalGrantConfig;
    #[tokio::test]
    async fn granted_protocol_mutation_executes_without_pending_approval_and_audits_id(
    ) -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("config.toml");
        let config = Config {
            approval_grants: vec![ApprovalGrantConfig {
                id: "patch-once".into(),
                tool: Some("apply_patch".into()),
                package: None,
                max_risk: "mutating".into(),
                expires_at: None,
                uses: Some(1),
            }],
            ..Config::default()
        };
        config::save_config(&path, &config)?;
        let target = dir.path().join("approved.txt");
        let (_cancel_tx, cancel) = watch::channel(false);
        let result = invoke_tool(
            &path,
            "apply_patch",
            json!({"path":target,"old_text":"","new_text":"approved"}),
            cancel,
        )
        .await?;
        assert_eq!(result["success"], true);
        assert_eq!(std::fs::read_to_string(target)?, "approved");
        let logs = std::fs::read_to_string(dir.path().join("nl2sh.log"))?;
        assert!(logs.contains("approved_by_grant"));
        assert!(logs.contains("patch-once"));
        for entry in std::fs::read_dir(dir.path())? {
            let entry = entry?.path();
            if entry.is_dir() {
                for child in std::fs::read_dir(entry)? {
                    assert!(!child?.path().join("request.json").exists());
                }
            }
        }
        Ok(())
    }
    #[tokio::test]
    async fn protocol_no_grant_waits_for_local_approval_then_cancellation_refuses() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("config.toml");
        config::save_config(&path, &Config::default())?;
        let target = dir.path().join("denied.txt");
        let (tx, cancel) = watch::channel(false);
        let args = json!({"path":target,"old_text":"","new_text":"denied"});
        let task =
            tokio::spawn(async move { invoke_tool(&path, "apply_patch", args, cancel).await });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(!task.is_finished());
        tx.send_replace(true);
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), task).await???;
        assert_eq!(result["success"], false);
        assert!(!target.exists());
        Ok(())
    }
}
