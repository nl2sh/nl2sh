use super::{command_tool, ConfirmationDecision, Confirmer, ConversationContext, ShellToolArgs};
use crate::{
    config::Config,
    limits::truncate_text,
    llm::{
        ConversationItem, ConversationMessage, LlmClient, LlmRequest, Role, TextDeltaSink,
        ToolResult, ToolRound, Usage,
    },
    security::assess,
    shell::CommandExecutor,
};
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
/// Dependencies for one bounded Agent tool loop.
pub struct AgentRunner<'a> {
    /// Validated policy and provider configuration.
    pub config: &'a Config,
    /// Provider-neutral model client.
    pub llm: &'a dyn LlmClient,
    /// Command execution boundary.
    pub executor: &'a dyn CommandExecutor,
    /// User approval boundary.
    pub confirmer: &'a dyn Confirmer,
}
#[derive(Debug)]
/// Successful final response and the number of model steps used.
pub struct AgentOutcome {
    /// Model's evidence-based final response.
    pub final_text: String,
    /// Count of completed LLM requests.
    pub steps: usize,
    /// Complete current interaction, including inseparable tool rounds.
    pub transcript: Vec<ConversationItem>,
    /// Token usage accumulated across every model request in this task.
    pub usage: Usage,
    /// Input tokens reported for the final model request, used for context estimates.
    pub final_input_tokens: Option<u64>,
    /// Complete historical turns evicted using observed provider token usage.
    pub history_turns_evicted: usize,
}
impl AgentRunner<'_> {
    /// Runs one natural-language request until final text or the step limit.
    pub async fn run(&self, input: &str) -> Result<AgentOutcome> {
        self.run_with_history(input, &[]).await
    }

    /// Runs a request with prior complete interactions, truncating only whole turns.
    pub async fn run_with_history(
        &self,
        input: &str,
        history: &[Vec<ConversationItem>],
    ) -> Result<AgentOutcome> {
        self.run_inner(input, history, None).await
    }

    async fn run_inner(
        &self,
        input: &str,
        history: &[Vec<ConversationItem>],
        text_sink: Option<&dyn TextDeltaSink>,
    ) -> Result<AgentOutcome> {
        let system = system_prompt(
            self.executor
                .runtime_context()
                .await
                .ok()
                .flatten()
                .as_deref(),
        );
        let mut ctx =
            ConversationContext::new(system, self.config.max_context_turns.saturating_sub(1));
        for turn in history {
            ctx.push_turn(truncate_tool_results(
                turn,
                self.config.model_tool_output_max_bytes,
            ));
        }
        let user_item = ConversationItem::Message(ConversationMessage::new(Role::User, input));
        let mut current = vec![user_item.clone()];
        let mut transcript = vec![user_item];
        let mut task_approvals = HashSet::new();
        let mut usage = Usage::default();
        let mut final_input_tokens = None;
        let mut history_turns_evicted = 0;
        for step in 1..=self.config.max_agent_steps {
            let mut items = ctx.items();
            items.extend(current.clone());
            let request = LlmRequest {
                model: self.config.model.clone(),
                items,
                tools: vec![command_tool()],
            };
            let response = if let Some(sink) = text_sink {
                self.llm.complete_stream(request, sink).await?
            } else {
                self.llm.complete(request).await?
            };
            usage.accumulate(&response.usage);
            final_input_tokens = response.usage.input_tokens;
            if let (Some(observed), Some(budget)) = (
                response.usage.input_tokens,
                self.config.effective_input_token_budget(),
            ) {
                history_turns_evicted += ctx.trim_for_observed_usage(observed, budget);
            }
            if response.tool_calls.is_empty() {
                let final_text = response
                    .text
                    .unwrap_or_else(|| "Agent returned no text.".into());
                current.push(ConversationItem::Message(ConversationMessage::new(
                    Role::Assistant,
                    final_text.clone(),
                )));
                transcript.push(ConversationItem::Message(ConversationMessage::new(
                    Role::Assistant,
                    final_text.clone(),
                )));
                return Ok(AgentOutcome {
                    final_text,
                    steps: step,
                    transcript,
                    usage,
                    final_input_tokens,
                    history_turns_evicted,
                });
            }
            let calls = response.tool_calls;
            let mut results = Vec::new();
            'tool_calls: for call in calls.iter().cloned() {
                if call.name != "execute_shell_command" {
                    results.push(ToolResult {
                        call_id: call.id,
                        output: "unsupported tool".into(),
                        success: false,
                    });
                    continue;
                }
                let args: ShellToolArgs = serde_json::from_value(call.arguments)
                    .context("invalid shell tool arguments")?;
                let mut command = args.command;
                let mut interactive_override = None;
                let assessment = loop {
                    let assessment = assess(&command, self.config);
                    if !assessment.requires_confirmation {
                        break assessment;
                    }
                    if super::can_remember_approval(&assessment)
                        && task_approvals.contains(&command)
                    {
                        break assessment;
                    }
                    match self.confirmer.confirm(&command, &assessment).await? {
                        ConfirmationDecision::Approve => break assessment,
                        ConfirmationDecision::ApproveForTask => {
                            if super::can_remember_approval(&assessment) {
                                task_approvals.insert(command.clone());
                            }
                            break assessment;
                        }
                        ConfirmationDecision::ApproveInteractive => {
                            interactive_override = Some(true);
                            break assessment;
                        }
                        ConfirmationDecision::ApproveCaptured => {
                            interactive_override = Some(false);
                            break assessment;
                        }
                        ConfirmationDecision::Edit(edited) => {
                            command = edited;
                        }
                        ConfirmationDecision::Reject => {
                            results.push(ToolResult {
                                call_id: call.id,
                                output: format!(
                                    "risk={:?} root={}\nNot executed: user rejected command.",
                                    assessment.risk_level, assessment.requires_root
                                ),
                                success: false,
                            });
                            continue 'tool_calls;
                        }
                    }
                };
                let execution = self
                    .executor
                    .execute(
                        &command,
                        assessment.requires_root,
                        interactive_override.unwrap_or_else(|| {
                            crate::shell::is_interactive(&command, args.interactive)
                        }),
                    )
                    .await;
                let mut result = match execution {
                    Ok(x) if x.interrupted => {
                        bail!("agent interrupted during command execution")
                    }
                    Ok(x) => ToolResult {
                        call_id: call.id,
                        output: format!(
                            "executed_command={}\nrisk={:?} root={} matched_rules={}\nexit={:?} timed_out={} interrupted={}\nstdout:\n{}\nstderr:\n{}",
                            command,
                            assessment.risk_level,
                            assessment.requires_root,
                            assessment.matched_rules.iter().map(|rule| rule.id.as_str()).collect::<Vec<_>>().join(","),
                            x.exit_code, x.timed_out, x.interrupted, x.stdout, x.stderr
                        ),
                        success: x.exit_code == Some(0) && !x.timed_out && !x.interrupted,
                    },
                    Err(e) => ToolResult {
                        call_id: call.id,
                        output: format!(
                            "executed_command={command}\nrisk={:?} root={}\nExecution failed: {e:#}",
                            assessment.risk_level, assessment.requires_root
                        ),
                        success: false,
                    },
                };
                result.output = truncate_text(&result.output, self.config.tool_output_max_bytes);
                results.push(result)
            }
            transcript.push(ConversationItem::Tools(ToolRound {
                calls: calls.clone(),
                results: results.clone(),
            }));
            let model_results = results
                .into_iter()
                .map(|mut result| {
                    result.output =
                        truncate_text(&result.output, self.config.model_tool_output_max_bytes);
                    result
                })
                .collect();
            current.push(ConversationItem::Tools(ToolRound {
                calls,
                results: model_results,
            }));
        }
        let final_text = format!(
            "Agent stopped after reaching the maximum of {} steps; the last tool results were retained.",
            self.config.max_agent_steps
        );
        current.push(ConversationItem::Message(ConversationMessage::new(
            Role::Assistant,
            final_text.clone(),
        )));
        transcript.push(ConversationItem::Message(ConversationMessage::new(
            Role::Assistant,
            final_text.clone(),
        )));
        Ok(AgentOutcome {
            final_text,
            steps: self.config.max_agent_steps,
            transcript,
            usage,
            final_input_tokens,
            history_turns_evicted,
        })
    }

    /// Owned-history variant suitable for a UI-managed background future.
    pub async fn run_with_history_owned(
        &self,
        input: String,
        history: Vec<Vec<ConversationItem>>,
    ) -> Result<AgentOutcome> {
        self.run_with_history(&input, &history).await
    }

    /// Owned-history variant that forwards provider text deltas to a UI sink.
    pub async fn run_with_history_streaming_owned(
        &self,
        input: String,
        history: Vec<Vec<ConversationItem>>,
        text_sink: &dyn TextDeltaSink,
    ) -> Result<AgentOutcome> {
        self.run_inner(&input, &history, Some(text_sink)).await
    }
}

fn system_prompt(runtime: Option<&str>) -> String {
    let mut system = format!(
        "You are an Android shell agent. Use execute_shell_command for evidence. Never claim unexecuted results. {} Write the final answer in the user's language for a human reader. Summarize conclusions instead of dumping raw tool protocol output. Use a concise Markdown table when comparing multiple items or presenting repeated structured fields; otherwise use clear concise text.",
        android_shell_constraints()
    );
    if let Some(runtime) = runtime {
        system.push_str("\nRuntime environment (advisory only; never bypass security): ");
        system.push_str(runtime);
    }
    system
}

/// Baseline execution constraints shared by Agent and single-command prompts.
pub fn android_shell_constraints() -> &'static str {
    "The target is a stock Android API 26+ shell using /system/bin/sh and toybox, not a desktop Linux distribution or Termux. Unless runtime evidence proves otherwise, assume these are unavailable: python/python3, bash/zsh/fish, node/npm/npx, perl, ruby, PHP, Lua, Java, Go, git, jq, curl/wget, ssh/scp/rsync, gcc/clang, make/cmake, and package managers such as apt/apt-get, yum/dnf, apk, pacman, brew, pip, gem, or cargo. Do not use /bin/bash, /usr/bin/env, GNU-only flags, or scripts requiring those runtimes. Prefer Android commands such as cmd, am, pm, dumpsys, settings, getprop, logcat, and toybox utilities. Before using any non-baseline executable, verify it with command -v using a read-only tool call and provide a /system/bin/sh or toybox fallback; do not install missing tooling unless the user explicitly requests it."
}

fn truncate_tool_results(items: &[ConversationItem], limit: usize) -> Vec<ConversationItem> {
    items
        .iter()
        .cloned()
        .map(|item| match item {
            ConversationItem::Tools(mut round) => {
                for result in &mut round.results {
                    result.output = truncate_text(&result.output, limit);
                }
                ConversationItem::Tools(round)
            }
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_tool_results_are_bounded_and_marked() {
        let items = vec![ConversationItem::Tools(ToolRound {
            calls: Vec::new(),
            results: vec![ToolResult {
                call_id: "call".into(),
                output: "x".repeat(1000),
                success: true,
            }],
        })];
        let bounded = truncate_tool_results(&items, 200);
        let ConversationItem::Tools(round) = &bounded[0] else {
            unreachable!("test constructs a tool round")
        };
        assert!(round.results[0].output.len() <= 200);
        assert!(round.results[0].output.contains("NL2SH OUTPUT TRUNCATED"));
    }

    #[test]
    fn runtime_summary_is_advisory_and_omitted_when_unavailable() {
        let base = system_prompt(None);
        assert!(!base.contains("Runtime environment"));

        let contextual = system_prompt(Some(
            "platform=Android api=34 abi=aarch64 shell=/system/bin/sh uid=2000 root=false su_available=true",
        ));
        assert!(contextual.contains("advisory only; never bypass security"));
        assert!(contextual.contains("api=34"));
        assert!(contextual.contains("uid=2000"));
    }

    #[test]
    fn system_prompt_forbids_assuming_desktop_script_runtimes() {
        let prompt = system_prompt(None);
        for required in [
            "/system/bin/sh",
            "python/python3",
            "node/npm/npx",
            "apt/apt-get",
            "command -v",
            "toybox fallback",
        ] {
            assert!(prompt.contains(required), "missing constraint: {required}");
        }
        assert!(prompt.contains("not a desktop Linux distribution or Termux"));
    }
}
