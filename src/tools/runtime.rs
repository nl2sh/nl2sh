//! Direct tool dispatch shared with non-Agent callers.

use super::{
    audio::domain::AudioToolExecutor, file::domain::FileToolExecutor, PreparedAction,
    PreparedExecution, ToolContext, ToolMetadata, ToolOutput, ToolRegistry, ToolRisk,
};
use crate::{
    agent::{ConfirmationDecision, Confirmer, TaskRuntime},
    config::Config,
    ima::ImaClient,
    limits::truncate_text,
    security::{assess, PrivilegeBroker},
    shell::{CommandExecutor, ExecutionBroker},
};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::{collections::HashMap, time::Instant};

/// Bounded outcome of one direct tool call. A rejected confirmation is never an execution.
#[derive(Debug, Serialize)]
pub struct DirectToolResult {
    /// Registry name of the requested tool.
    pub tool: String,
    /// Whether the operation completed successfully.
    pub success: bool,
    /// Bounded result or refusal reason.
    pub output: String,
    /// Bounded non-text content produced by an existing tool.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::llm::ToolAttachment>,
}

/// Apply the same structured-tool assessment and approval boundary for Agent and direct calls.
pub(crate) async fn execute_prepared_operation(
    metadata: &ToolMetadata,
    prepared_risk: Option<ToolRisk>,
    preview: &str,
    operation: Box<dyn PreparedExecution>,
    ctx: &mut ToolContext<'_>,
    confirmer: &dyn Confirmer,
) -> Result<ToolOutput> {
    let assessment = metadata
        .assessment_for(prepared_risk.unwrap_or(metadata.risk))
        .context("prepared operation has no structured security assessment")?;
    if assessment.requires_confirmation {
        if preview.trim().is_empty() {
            bail!("mutating tool has no approval preview")
        }
        let started = Instant::now();
        let decision = confirmer.confirm(preview, &assessment).await;
        ctx.runtime
            .as_deref_mut()
            .context("tool runtime unavailable")?
            .add_confirmation_time(started.elapsed());
        match decision? {
            ConfirmationDecision::Approve | ConfirmationDecision::ApproveForTask
            | ConfirmationDecision::ApproveCaptured | ConfirmationDecision::ApproveInteractive => {}
            ConfirmationDecision::ApproveForRun if crate::agent::can_remember_approval(&assessment) => {}
            ConfirmationDecision::Edit(_) => return Ok(ToolOutput::refused(
                "Tool not executed: edit is unavailable for a prepared operation; request a new call.",
            )),
            ConfirmationDecision::Reject | ConfirmationDecision::ApproveForRun => return Ok(ToolOutput::refused(
                "Tool not executed: user rejected the prepared operation.",
            )),
        }
    }
    operation.execute(ctx).await
}

/// Prepare, assess, confirm, and execute one registered tool without consulting an Agent.
pub async fn invoke(
    config: &Config,
    executor: &dyn CommandExecutor,
    confirmer: &dyn Confirmer,
    name: &str,
    arguments: Value,
) -> Result<DirectToolResult> {
    let base = std::env::current_dir().context("cannot determine tool base directory")?;
    let file_tools = FileToolExecutor::new(&base)?;
    let audio_tools = AudioToolExecutor::new(&base)?;
    let ima = ImaClient::from_config(config)?;
    let capabilities = crate::runtime::RuntimeCapabilities::discover(config, executor).await;
    let registry = ToolRegistry::for_runtime(config, &capabilities);
    let tool = registry
        .get(name)
        .with_context(|| format!("unsupported tool {name}"))?;
    let mut runtime = TaskRuntime::new();
    let mut audio_cache = HashMap::new();
    let mut ctx = ToolContext {
        file_tools: &file_tools,
        ima: ima.as_ref(),
        config: Some(config),
        executor: Some(executor),
        llm: None,
        confirmer: Some(confirmer),
        audio_tools: Some(&audio_tools),
        runtime: Some(&mut runtime),
        audio_cache: Some(&mut audio_cache),
    };
    let prepared = tool.prepare(&ctx, arguments).await?;
    let result = match prepared.action {
        PreparedAction::Operation(operation) => {
            let output = execute_prepared_operation(
                tool.metadata(),
                prepared.risk,
                &prepared.preview,
                operation,
                &mut ctx,
                confirmer,
            )
            .await?;
            DirectToolResult {
                tool: name.into(),
                success: output.success,
                output: output.content,
                attachments: output.attachments,
            }
        }
        PreparedAction::Shell(args) => {
            if tool.metadata().risk != ToolRisk::DynamicShell {
                bail!("shell tool metadata is invalid")
            }
            let mut command = args.command;
            let (assessment, approved) = loop {
                let assessment = assess(&command, config);
                if !assessment.requires_confirmation {
                    break (assessment, None);
                }
                match confirmer.confirm(&command, &assessment).await? {
                    ConfirmationDecision::Approve
                    | ConfirmationDecision::ApproveForTask
                    | ConfirmationDecision::ApproveCaptured
                    | ConfirmationDecision::ApproveInteractive => {
                        break (assessment, Some(command.clone()))
                    }
                    ConfirmationDecision::Edit(edited) => command = edited,
                    _ => {
                        return Ok(refused(
                            name,
                            "Tool not executed: local confirmation was not granted.",
                        ))
                    }
                }
            };
            let capability =
                PrivilegeBroker::authorize(&command, &assessment, config, approved.as_deref())?;
            let result = ExecutionBroker::execute(executor, capability, false).await?;
            DirectToolResult {
                tool: name.into(),
                success: result.exit_code == Some(0) && !result.timed_out && !result.interrupted,
                output: format!("executed_command={command}\nrisk={:?} root={}\nexit={:?} timed_out={} interrupted={}\nstdout:\n{}\nstderr:\n{}", assessment.risk_level, assessment.requires_root, result.exit_code, result.timed_out, result.interrupted, result.stdout, result.stderr),
                attachments: Vec::new(),
            }
        }
    };
    Ok(DirectToolResult {
        output: truncate_text(&result.output, config.tool_output_max_bytes),
        ..result
    })
}

fn refused(name: &str, reason: &str) -> DirectToolResult {
    DirectToolResult {
        tool: name.into(),
        success: false,
        output: reason.into(),
        attachments: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::invoke;
    use crate::{
        agent::{ConfirmationDecision, Confirmer},
        config::Config,
        security::SecurityAssessment,
        shell::ShellExecutor,
    };
    use anyhow::Result;
    use async_trait::async_trait;
    use serde_json::json;

    struct Reject;

    #[async_trait]
    impl Confirmer for Reject {
        async fn confirm(
            &self,
            _command: &str,
            _assessment: &SecurityAssessment,
        ) -> Result<ConfirmationDecision> {
            Ok(ConfirmationDecision::Reject)
        }
    }

    #[tokio::test]
    async fn disabled_optional_tool_cannot_be_invoked_directly() {
        let config = Config::default();
        let executor = ShellExecutor::new(config.clone());
        let result = invoke(&config, &executor, &Reject, "tailcat_check", json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejected_tailcat_listener_never_starts() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let binary = dir.path().join("tailcat");
        let shell = if cfg!(target_os = "android") {
            "/system/bin/sh"
        } else {
            "/bin/sh"
        };
        std::fs::write(&binary, format!("#!{shell}\necho 'tailcat v0.7.0'\n"))?;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))?;
        let mut config = Config {
            tailcat_binary_path: binary,
            ..Config::default()
        };
        config.tool_overrides.insert("tailcat_serve".into(), true);
        config.tool_overrides.insert("tailcat_status".into(), true);
        let executor = ShellExecutor::new(config.clone());
        let result = invoke(
            &config,
            &executor,
            &Reject,
            "tailcat_serve",
            json!({"port":8080}),
        )
        .await?;
        assert!(!result.success);
        let status = invoke(&config, &executor, &Reject, "tailcat_status", json!({})).await?;
        assert!(status.output.contains("no managed Tailcat listener"));
        Ok(())
    }

    #[tokio::test]
    async fn direct_runtime_rejects_write_before_execution() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("target.txt");
        let config = Config::default();
        let executor = ShellExecutor::new(config.clone());
        let result = invoke(
            &config,
            &executor,
            &Reject,
            "apply_patch",
            json!({
                "path": path.to_string_lossy(), "old_text": "", "new_text": "unapproved"
            }),
        )
        .await?;
        assert!(!result.success);
        assert!(!path.exists());
        Ok(())
    }

    #[tokio::test]
    async fn direct_runtime_runs_readonly_tool_without_model() -> Result<()> {
        let config = Config::default();
        let executor = ShellExecutor::new(config.clone());
        let result = invoke(
            &config,
            &executor,
            &Reject,
            "create_chart",
            json!({
                "chart_type": "bar", "title": "Test", "source": "provided values",
                "labels": ["A"], "values": [2]
            }),
        )
        .await?;
        assert!(result.success);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result.output)?["values"],
            json!([2.0])
        );
        Ok(())
    }

    #[tokio::test]
    async fn direct_runtime_preserves_image_attachment() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("screen.png");
        image::RgbImage::new(1, 1).save(&path)?;
        let config = Config::default();
        let executor = ShellExecutor::new(config.clone());
        let result = invoke(
            &config,
            &executor,
            &Reject,
            "view_screenshot",
            json!({
                "path": path.to_string_lossy()
            }),
        )
        .await?;
        assert!(result.success);
        assert_eq!(result.attachments.len(), 1);
        assert_eq!(result.attachments[0].media_type, "image/png");
        Ok(())
    }
}
