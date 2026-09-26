//! Narrow JSON interface used by a separately deployed A2A gateway.

use crate::{
    agent::{AgentRunner, ConfirmationDecision, Confirmer},
    config::{self, ConfirmPolicy, SecurityLevel},
    llm::{build_client, ConversationItem},
    security::SecurityAssessment,
    sessions::SessionStore,
    shell::ShellExecutor,
    tools::{android::environment::inspect_environment, builtin_tools},
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
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AskRequest {
    session: String,
    message: String,
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

#[async_trait]
impl Confirmer for BridgeConfirmer {
    async fn confirm(
        &self,
        _command: &str,
        _assessment: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        Ok(ConfirmationDecision::Reject)
    }
}

/// Runs one bridge operation without starting the Web or terminal interface.
pub async fn run(operation: BridgeOperation, path: &Path) -> Result<()> {
    match operation {
        BridgeOperation::Tools => {
            let cfg = config::load_or_default_unvalidated(path)?;
            println!(
                "{}",
                serde_json::to_string(&builtin_tools(cfg.ima_enabled))?
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
            if !cfg.provider_is_configured() {
                bail!("model provider is not configured")
            }
            // Remote callers have no local confirmation channel. Preserve a
            // mandatory confirmation requirement even for an unsafe local UI.
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
            let runner = AgentRunner {
                config: &cfg,
                llm: &llm,
                executor: &executor,
                confirmer: &BridgeConfirmer,
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
    }
    Ok(())
}
