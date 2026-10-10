//! Shell tool declaration; command assessment and execution remain in the Agent boundary.

use super::{PreparedToolCall, ShellToolArgs, ToolCategory, ToolContext, ToolMetadata, ToolRisk};
use anyhow::Result;

const SHELL_META: ToolMetadata = ToolMetadata {
    name: "execute_shell_command",
    description: "Execute a shell command after local security evaluation and required confirmation. background=true returns a process-owned child_id immediately for bounded noninteractive capture; inspect read_output and stop with kill. Started does not mean succeeded. Background su elevation is unsupported.",
    category: ToolCategory::Shell,
    risk: ToolRisk::DynamicShell,
    requires: &[],
    group: None, default_enabled: true,
            platform: crate::tools::ToolPlatform::Any, runtime: crate::tools::RuntimeRequirement::None,
            concurrency: crate::tools::ToolConcurrency::Shell, lifetime: crate::tools::ToolLifetime::Process,
            schema: crate::tools::descriptor_schema::<ShellToolArgs>,
};

async fn prepare_shell(_: &ToolContext<'_>, args: ShellToolArgs) -> Result<PreparedToolCall> {
    if args.background && args.interactive {
        anyhow::bail!("background execution does not support interactive stdin or PTY")
    }
    if args.background && !(1..=86400).contains(&args.background_timeout_secs) {
        anyhow::bail!("background_timeout_secs must be 1–86400")
    }
    Ok(PreparedToolCall::shell(args))
}

define_tool!(ShellTool, ShellToolArgs, SHELL_META, prepare_shell);

use super::{PreparedExecution, ToolOutput};
use anyhow::Context;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadArgs {
    /// Opaque child_id returned by a successful background start, not a PID.
    child_id: String,
    /// Raw stdout byte offset; follow stdout.next_offset.
    #[serde(default)]
    offset: u64,
    /// Raw stderr byte offset; follow stderr.next_offset independently.
    #[serde(default)]
    stderr_offset: u64,
    /// Maximum raw bytes per stream, 1–16384 (default 1024).
    #[serde(default = "default_bytes")]
    max_bytes: usize,
}
fn default_bytes() -> usize {
    1024
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct KillArgs {
    /// Opaque managed child_id, never an arbitrary PID.
    child_id: String,
}
macro_rules! metadata {
    ($constant:ident, $name:literal, $description:literal, $args:ty, $risk:ident, $concurrency:ident) => {
        const $constant: ToolMetadata = ToolMetadata {
            name: $name,
            description: $description,
            category: ToolCategory::Shell,
            risk: ToolRisk::$risk,
            requires: &[],
            group: None,
            default_enabled: true,
            platform: super::ToolPlatform::Any,
            runtime: super::RuntimeRequirement::None,
            concurrency: super::ToolConcurrency::$concurrency,
            lifetime: super::ToolLifetime::Process,
            schema: super::descriptor_schema::<$args>,
        };
    };
}
metadata!(READ, "read_output", "Read bounded stdout/stderr pages and actual status of a managed background shell. Follow each stream's next_offset; truncated means older bytes were evicted. Handles belong to the current nl2sh process and configuration. Empty output is not proof of completion.", ReadArgs, ReadOnly, Parallel);
metadata!(KILL, "kill", "After confirmation, stop only the background shell identified by child_id using TERM then KILL and wait for cleanup. Never accepts arbitrary PIDs. Idempotent for retained completed handles; inspect error and finished.", KillArgs, Mutating, Sequential);

enum Operation {
    Read(ReadArgs),
    Kill(KillArgs),
}
#[async_trait]
impl PreparedExecution for Operation {
    async fn execute(self: Box<Self>, ctx: &mut ToolContext<'_>) -> Result<ToolOutput> {
        let executor = ctx.executor.context("shell executor unavailable")?;
        let result = match *self {
            Self::Read(args) => {
                executor
                    .read_output(
                        &args.child_id,
                        args.offset,
                        args.stderr_offset,
                        args.max_bytes,
                    )
                    .await?
            }
            Self::Kill(args) => executor.kill(&args.child_id).await?,
        };
        let success = result.error.is_none();
        Ok(ToolOutput {
            success,
            content: serde_json::to_string(&result)?,
            attachments: Vec::new(),
        })
    }
}
fn validate_id(id: &str) -> Result<()> {
    if uuid::Uuid::parse_str(id).is_err() {
        anyhow::bail!("child_id must be a managed UUID handle")
    }
    Ok(())
}
async fn prepare_read(_: &ToolContext<'_>, args: ReadArgs) -> Result<PreparedToolCall> {
    validate_id(&args.child_id)?;
    if !(1..=16384).contains(&args.max_bytes) {
        anyhow::bail!("max_bytes must be 1–16384")
    }
    Ok(PreparedToolCall::operation(
        String::new(),
        Box::new(Operation::Read(args)),
    ))
}
async fn prepare_kill(_: &ToolContext<'_>, args: KillArgs) -> Result<PreparedToolCall> {
    validate_id(&args.child_id)?;
    Ok(PreparedToolCall::operation(
        format!(
            "Stop managed background shell {} (TERM/KILL/wait)",
            args.child_id
        ),
        Box::new(Operation::Kill(args)),
    ))
}
define_tool!(ReadOutputTool, ReadArgs, READ, prepare_read);
define_tool!(KillTool, KillArgs, KILL, prepare_kill);
