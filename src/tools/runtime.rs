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
    crate::audit::assessment(
        preview,
        &format!("{:?}", assessment.risk_level),
        assessment.requires_root,
        assessment.requires_confirmation,
    );
    if assessment.requires_confirmation {
        if preview.trim().is_empty() {
            bail!("mutating tool has no approval preview")
        }
        let started = Instant::now();
        let decision = crate::agent::confirm_assessed(
            ctx.config.context("tool configuration unavailable")?,
            confirmer,
            &crate::agent::ConfirmationRequest {
                preview,
                tool: Some(metadata.name),
                package: operation.approval_package(),
            },
            &assessment,
        )
        .await;
        ctx.runtime
            .as_deref_mut()
            .context("tool runtime unavailable")?
            .add_confirmation_time(started.elapsed());
        let decision = decision?;
        crate::audit::decision(&decision);
        match decision {
            ConfirmationDecision::Approve | ConfirmationDecision::ApproveByGrant(_) | ConfirmationDecision::ApproveForTask
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
    let held = ctx
        .runtime
        .as_deref()
        .is_some_and(TaskRuntime::holds_ui_lease);
    crate::runtime::resources::with_ui_lease(held, operation.execute(ctx)).await
}

/// Prepare, assess, confirm, and execute one registered tool without consulting an Agent.
pub async fn invoke(
    config: &Config,
    executor: &dyn CommandExecutor,
    confirmer: &dyn Confirmer,
    name: &str,
    arguments: Value,
) -> Result<DirectToolResult> {
    crate::audit::AuditContext::new(config, confirmer.audit_source(), confirmer.audit_session())
        .scope(async {
            let audit = crate::audit::AuditGuard::begin(name, "unassessed");
            let result = invoke_scoped(config, executor, confirmer, name, arguments).await;
            audit.finish(match &result {
                Ok(result) if result.success => "success",
                Ok(_) => "error",
                Err(_) => "error",
            });
            result
        })
        .await
}

async fn invoke_scoped(
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
    runtime.acquire_resources(tool.metadata()).await?;
    let held = runtime.holds_ui_lease();
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
    let prepared =
        crate::runtime::resources::with_ui_lease(held, tool.prepare(&ctx, arguments)).await?;
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
                crate::audit::assessment(
                    &command,
                    &format!("{:?}", assessment.risk_level),
                    assessment.requires_root,
                    assessment.requires_confirmation,
                );
                if !assessment.requires_confirmation {
                    break (assessment, None);
                }
                let preview = if args.background {
                    format!(
                        "Background capture (no stdin/PTY; limit {} seconds; process-owned):\n{}",
                        args.background_timeout_secs, command
                    )
                } else {
                    command.clone()
                };
                let decision = crate::agent::confirm_assessed(
                    config,
                    confirmer,
                    &crate::agent::ConfirmationRequest::shell(&preview),
                    &assessment,
                )
                .await?;
                crate::audit::decision(&decision);
                match decision {
                    ConfirmationDecision::Approve
                    | ConfirmationDecision::ApproveByGrant(_)
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
            if args.background {
                let lease = (assessment.risk_level != crate::security::RiskLevel::ReadOnly)
                    .then(|| runtime.background_ui_lease())
                    .flatten();
                let child_id = crate::runtime::resources::with_background_ui_lease(
                    lease,
                    ExecutionBroker::spawn_background(
                        executor,
                        capability,
                        args.background_timeout_secs,
                    ),
                )
                .await?;
                return Ok(DirectToolResult {
                    tool: name.into(), success: true,
                    output: serde_json::json!({"status":"started", "child_id":child_id, "finished":false, "background_timeout_secs":args.background_timeout_secs}).to_string(),
                    attachments: Vec::new(),
                });
            }
            let result = crate::runtime::resources::with_ui_lease(
                held,
                ExecutionBroker::execute(executor, capability, false),
            )
            .await?;
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
            _command: &crate::agent::ConfirmationRequest<'_>,
            _assessment: &SecurityAssessment,
        ) -> Result<ConfirmationDecision> {
            let _command = _command.preview;
            Ok(ConfirmationDecision::Reject)
        }
    }

    struct ScopeRecorder;
    #[async_trait]
    impl Confirmer for ScopeRecorder {
        async fn confirm(
            &self,
            request: &crate::agent::ConfirmationRequest<'_>,
            assessment: &SecurityAssessment,
        ) -> Result<ConfirmationDecision> {
            assert_eq!(request.tool, Some("apply_patch"));
            assert_eq!(request.package, None);
            assert!(request.preview.contains("+scope test"));
            assert_eq!(assessment.risk_level, crate::security::RiskLevel::Mutating);
            Ok(ConfirmationDecision::Reject)
        }
    }
    #[tokio::test]
    async fn prepared_confirmation_receives_registered_tool_scope() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("target");
        let config = Config::default();
        let executor = ShellExecutor::new(config.clone());
        let result = invoke(
            &config,
            &executor,
            &ScopeRecorder,
            "apply_patch",
            json!({"path":path,"old_text":"","new_text":"scope test"}),
        )
        .await?;
        assert!(!result.success);
        assert!(!path.exists());
        Ok(())
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

#[cfg(test)]
mod background_tests {
    use super::*;
    use crate::{
        agent::ConfirmationRequest,
        config::ExecuteUserMode,
        security::SecurityAssessment,
        shell::{RootProbe, ShellExecutor},
    };
    use async_trait::async_trait;
    use serde_json::json;

    struct Reject;
    #[async_trait]
    impl Confirmer for Reject {
        async fn confirm(
            &self,
            _: &ConfirmationRequest<'_>,
            _: &SecurityAssessment,
        ) -> Result<ConfirmationDecision> {
            Ok(ConfirmationDecision::Reject)
        }
    }
    struct EditThenReject(std::sync::atomic::AtomicUsize);
    #[async_trait]
    impl Confirmer for EditThenReject {
        async fn confirm(
            &self,
            request: &ConfirmationRequest<'_>,
            _: &SecurityAssessment,
        ) -> Result<ConfirmationDecision> {
            assert!(request.preview.contains("Background capture"));
            if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                Ok(ConfirmationDecision::Edit(
                    "rm -rf /not-a-real-target".into(),
                ))
            } else {
                Ok(ConfirmationDecision::Reject)
            }
        }
    }

    #[tokio::test]
    async fn background_mutation_rejection_and_edit_never_start() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("unapproved");
        let config = Config {
            source: Some(directory.path().join("config.toml")),
            execute_user_mode: ExecuteUserMode::Normal,
            ..Config::default()
        };
        let executor = ShellExecutor::new(config.clone());
        let result = invoke(
            &config,
            &executor,
            &Reject,
            "execute_shell_command",
            json!({"command":format!("touch {}", path.display()),"background":true}),
        )
        .await?;
        assert!(!result.success && !path.exists());
        let confirmer = EditThenReject(std::sync::atomic::AtomicUsize::new(0));
        let result = invoke(
            &config,
            &executor,
            &confirmer,
            "execute_shell_command",
            json!({"command":format!("touch {}", path.display()),"background":true}),
        )
        .await?;
        assert!(!result.success && !path.exists());
        assert_eq!(confirmer.0.load(std::sync::atomic::Ordering::SeqCst), 2);
        Ok(())
    }

    #[tokio::test]
    async fn handle_survives_call_executor_and_task_watch_but_kill_requires_confirmation(
    ) -> Result<()> {
        let dir = tempfile::tempdir()?;
        let config = Config {
            source: Some(dir.path().join("config.toml")),
            execute_user_mode: ExecuteUserMode::Normal,
            ..Config::default()
        };
        let (sender, receiver) = tokio::sync::watch::channel(false);
        let executor = ShellExecutor::new(config.clone()).with_cancel(receiver);
        let result = invoke(
            &config,
            &executor,
            &Reject,
            "execute_shell_command",
            json!({"command":"sleep 30","background":true,"background_timeout_secs":30}),
        )
        .await?;
        assert!(result.success);
        let start: Value = serde_json::from_str(&result.output)?;
        assert_eq!(start["status"], "started");
        let id = start["child_id"].as_str().context("child_id missing")?;
        sender.send_replace(true);
        drop(sender);
        drop(executor);
        let other = ShellExecutor::new(config.clone());
        let read = invoke(
            &config,
            &other,
            &Reject,
            "read_output",
            json!({"child_id":id}),
        )
        .await?;
        assert!(read.success);
        let output: Value = serde_json::from_str(&read.output)?;
        assert_eq!(output["finished"], false);
        let result = invoke(&config, &other, &Reject, "kill", json!({"child_id":id})).await?;
        assert!(!result.success);
        assert!(!other.read_output(id, 0, 0, 100).await?.finished);
        // Fixture cleanup uses the security-agnostic executor, without altering runtime policy.
        assert!(other.kill(id).await?.finished);
        assert!(
            invoke(&config, &other, &Reject, "kill", json!({"child_id":"1"}))
                .await
                .is_err()
        );
        assert!(invoke(
            &config,
            &other,
            &Reject,
            "execute_shell_command",
            json!({"command":"sleep 1","background":true,"interactive":true})
        )
        .await
        .is_err());
        assert!(invoke(
            &config,
            &other,
            &Reject,
            "execute_shell_command",
            json!({"command":"sleep 1","background":true,"background_timeout_secs":0})
        )
        .await
        .is_err());
        Ok(())
    }

    struct NonRoot;
    impl RootProbe for NonRoot {
        fn uid(&self) -> u32 {
            2000
        }
        fn su_available(&self) -> bool {
            true
        }
    }
    #[tokio::test]
    async fn background_su_plan_is_rejected_before_spawning() {
        let executor = ShellExecutor::with_probe(
            Config {
                execute_user_mode: ExecuteUserMode::Root,
                ..Config::default()
            },
            Box::new(NonRoot),
        );
        let result = executor.spawn_background("sleep 30", true, 30).await;
        assert!(result.is_err());
    }
}
