//! Narrow JSON interface used by a separately deployed A2A gateway.

mod approval;

use crate::{
    agent::{AgentRunner, ConfirmationDecision, Confirmer},
    config::{self, ConfirmPolicy, SecurityLevel},
    llm::{build_client, ConversationItem},
    security::SecurityAssessment,
    sessions::SessionStore,
    shell::ShellExecutor,
    tools::{
        android::environment::inspect_environment, available_tools,
        disable_managed_tailcat_for_bridge, runtime::invoke,
    },
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use std::path::Path;

const MAX_REQUEST_BYTES: u64 = 16 * 1024;

/// One machine-readable bridge operation.
pub enum BridgeOperation {
    /// Collect fixed, read-only probes.
    Inspect,
    /// Describe the tools currently available to the model.
    Tools,
    /// Run one Agent turn using a named private session.
    Ask {
        /// Base64url-encoded JSON request.
        payload_base64: String,
    },
    /// Invoke a registered tool without an LLM request.
    Invoke {
        /// Base64url-encoded JSON request.
        payload_base64: String,
    },
    /// List requests waiting for a local human decision.
    Approvals,
    /// Decide one request in an interactive local terminal.
    Approve {
        /// Opaque request identifier.
        id: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AskRequest {
    session: String,
    message: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InvokeRequest {
    tool: String,
    arguments: serde_json::Value,
}

#[derive(Serialize)]
struct AskResponse {
    session: String,
    answer: String,
    steps: usize,
    tool_calls: usize,
    failed_tools: Vec<String>,
}

struct BridgeConfirmer;

struct AutoApproveConfirmer;

#[async_trait]
impl Confirmer for BridgeConfirmer {
    fn audit_source(&self) -> &'static str {
        "bridge"
    }
    async fn confirm(
        &self,
        _command: &str,
        _assessment: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        Ok(ConfirmationDecision::Reject)
    }
}

#[async_trait]
impl Confirmer for AutoApproveConfirmer {
    fn audit_source(&self) -> &'static str {
        "bridge_explicit_auto_approve"
    }
    async fn confirm(
        &self,
        _command: &str,
        _assessment: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        Ok(ConfirmationDecision::Approve)
    }
}

/// Runs one bridge operation without starting the Web or terminal interface.
pub async fn run(operation: BridgeOperation, path: &Path) -> Result<()> {
    match operation {
        BridgeOperation::Tools => {
            let mut cfg = config::load_or_default_unvalidated(path)?;
            disable_managed_tailcat_for_bridge(&mut cfg);
            let executor = ShellExecutor::new(cfg.clone());
            println!(
                "{}",
                serde_json::to_string(&available_tools(&cfg, &executor).await)?
            );
        }
        BridgeOperation::Inspect => {
            let cfg = config::load_or_default_unvalidated(path)?;
            let executor = ShellExecutor::new(cfg);
            println!("{}", inspect_environment(&executor).await?);
        }
        BridgeOperation::Ask { payload_base64 } => {
            if payload_base64.len() as u64 > MAX_REQUEST_BYTES * 2 {
                bail!("bridge request exceeds size limit")
            }
            let bytes = URL_SAFE_NO_PAD
                .decode(payload_base64)
                .context("invalid bridge payload encoding")?;
            if bytes.len() as u64 > MAX_REQUEST_BYTES {
                bail!("bridge request exceeds size limit")
            }
            let request: AskRequest =
                serde_json::from_slice(&bytes).context("invalid bridge request")?;
            if request.message.trim().is_empty() || request.message.len() > 8192 {
                bail!("bridge message must contain 1–8192 bytes")
            }
            let mut cfg = config::load_or_default_unvalidated(path)?;
            cfg.validate_runtime()?;
            disable_managed_tailcat_for_bridge(&mut cfg);
            if !cfg.provider_is_configured() {
                bail!("model provider is not configured")
            }
            if cfg.security_level == SecurityLevel::Unsafe {
                cfg.security_level = SecurityLevel::Balanced;
            }
            if cfg.execute_confirm_policy == ConfirmPolicy::Never {
                cfg.execute_confirm_policy = ConfirmPolicy::RiskOnly;
            }
            let store = SessionStore::open(path)?;
            let history = match store.load(
                &request.session,
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
            let executor = ShellExecutor::new(cfg.clone());
            let auto_approve = AutoApproveConfirmer;
            let reject = BridgeConfirmer;
            let confirmer: &dyn Confirmer = if cfg.bridge_auto_approve {
                &auto_approve
            } else {
                &reject
            };
            let runner = AgentRunner {
                config: &cfg,
                llm: &llm,
                executor: &executor,
                confirmer,
            };
            let outcome = runner.run_with_history(&request.message, &history).await?;
            let failed_tools = outcome
                .transcript
                .iter()
                .filter_map(|item| match item {
                    ConversationItem::Tools(round) => Some(&round.results),
                    _ => None,
                })
                .flat_map(|results| results.iter())
                .filter(|result| !result.success)
                .take(8)
                .map(|result| crate::limits::truncate_text(&result.output, 512))
                .collect();
            let mut turns = history;
            turns.push(outcome.transcript);
            let secrets = vec![
                cfg.api_key,
                cfg.proxy_password,
                cfg.ima_api_key,
                cfg.jev_api_key,
            ];
            store.save_redacted(
                &request.session,
                &turns,
                cfg.model_tool_output_max_bytes,
                &secrets,
            )?;
            println!(
                "{}",
                serde_json::to_string(&AskResponse {
                    session: request.session,
                    answer: outcome.final_text,
                    steps: outcome.steps,
                    tool_calls: outcome.tool_calls,
                    failed_tools,
                })?
            );
        }
        BridgeOperation::Invoke { payload_base64 } => {
            if payload_base64.len() as u64 > MAX_REQUEST_BYTES * 2 {
                bail!("bridge request exceeds size limit")
            }
            let bytes = URL_SAFE_NO_PAD
                .decode(payload_base64)
                .context("invalid bridge payload encoding")?;
            if bytes.len() as u64 > MAX_REQUEST_BYTES {
                bail!("bridge request exceeds size limit")
            }
            let request: InvokeRequest =
                serde_json::from_slice(&bytes).context("invalid bridge tool request")?;
            if request.tool.is_empty() || request.tool.len() > 128 {
                bail!("invalid bridge tool name")
            }
            let mut cfg = config::load_or_default_unvalidated(path)?;
            disable_managed_tailcat_for_bridge(&mut cfg);
            if cfg.security_level == SecurityLevel::Unsafe {
                cfg.security_level = SecurityLevel::Balanced;
            }
            if cfg.execute_confirm_policy == ConfirmPolicy::Never {
                cfg.execute_confirm_policy = ConfirmPolicy::RiskOnly;
            }
            let executor = ShellExecutor::new(cfg.clone());
            let auto_approve = AutoApproveConfirmer;
            let local_approval = if cfg.bridge_auto_approve {
                None
            } else {
                Some(approval::BridgeApprovalConfirmer::new(path)?)
            };
            let confirmer: &dyn Confirmer = match &local_approval {
                Some(local) => local,
                None => &auto_approve,
            };
            let result =
                invoke(&cfg, &executor, confirmer, &request.tool, request.arguments).await?;
            println!("{}", serde_json::to_string(&result)?);
        }
        BridgeOperation::Approvals => approval::list_pending(path)?,
        BridgeOperation::Approve { id } => {
            let path = path.to_path_buf();
            tokio::task::spawn_blocking(move || approval::approve_interactive(&path, &id))
                .await
                .context("approval terminal worker failed")??;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AutoApproveConfirmer, BridgeConfirmer};
    use crate::{
        agent::{ConfirmationDecision, Confirmer},
        config::Config,
        security::{RiskLevel, SecurityAssessment},
        shell::ShellExecutor,
        tools::runtime::invoke,
    };
    use serde_json::json;

    #[tokio::test]
    async fn bridge_auto_approval_covers_critical_operations() -> anyhow::Result<()> {
        let assessment = SecurityAssessment::from_policy(
            RiskLevel::Critical,
            Vec::new(),
            true,
            "critical operation".into(),
            false,
        );
        assert!(assessment.requires_double_confirmation);
        assert!(matches!(
            BridgeConfirmer.confirm("command", &assessment).await?,
            ConfirmationDecision::Reject
        ));
        assert!(matches!(
            AutoApproveConfirmer.confirm("command", &assessment).await?,
            ConfirmationDecision::Approve
        ));
        Ok(())
    }

    #[tokio::test]
    async fn bridge_auto_approval_executes_prepared_write() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("target.txt");
        let config = Config::default();
        let executor = ShellExecutor::new(config.clone());
        let result = invoke(
            &config,
            &executor,
            &AutoApproveConfirmer,
            "apply_patch",
            json!({
                "path": path.to_string_lossy(),
                "old_text": "",
                "new_text": "approved"
            }),
        )
        .await?;
        assert!(result.success, "{}", result.output);
        assert_eq!(std::fs::read_to_string(path)?, "approved");
        Ok(())
    }
}
