//! Shell tool declaration; command assessment and execution remain in the Agent boundary.

use super::{PreparedToolCall, ShellToolArgs, ToolCategory, ToolContext, ToolMetadata, ToolRisk};
use anyhow::Result;

const SHELL_META: ToolMetadata = ToolMetadata {
    name: "execute_shell_command",
    description: "Execute a shell command in the Android shell environment after security evaluation and required user confirmation.",
    category: ToolCategory::Shell,
    risk: ToolRisk::DynamicShell,
    requires: &[],
    group: None, default_enabled: true,
            platform: crate::tools::ToolPlatform::Any, runtime: crate::tools::RuntimeRequirement::None,
            concurrency: crate::tools::ToolConcurrency::Shell, lifetime: crate::tools::ToolLifetime::Call,
            schema: crate::tools::descriptor_schema::<ShellToolArgs>,
};

async fn prepare_shell(_: &ToolContext<'_>, args: ShellToolArgs) -> Result<PreparedToolCall> {
    Ok(PreparedToolCall::shell(args))
}

define_tool!(ShellTool, ShellToolArgs, SHELL_META, prepare_shell);
