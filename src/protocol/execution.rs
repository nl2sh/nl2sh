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
    fn audit_source(&self) -> &'static str {
        if self.auto {
            "protocol_explicit_auto_approve"
        } else {
            "protocol_terminal"
        }
    }
    async fn confirm(
        &self,
        preview: &str,
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
            decision = self.local.confirm(preview, assessment) => decision,
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
    Ok(serde_json::from_str(
        &inspect_environment(&executor).await?,
    )?)
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
            cfg.ima_api_key,
            cfg.jev_api_key,
        ],
    )?;
    Ok(
        json!({"session": session, "answer": outcome.final_text, "steps": outcome.steps, "tool_calls": outcome.tool_calls, "failed_tools": failed_tools}),
    )
}
