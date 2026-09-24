//! Shell tool declaration; command assessment and execution remain in the Agent boundary.

use super::{PreparedToolCall, ShellToolArgs, ToolCategory, ToolContext, ToolMetadata, ToolRisk};
use anyhow::Result;

const SHELL_META: ToolMetadata = ToolMetadata {
    name: "execute_shell_command",
    description: "Execute a shell command in the Android shell environment after security evaluation and required user confirmation.",
    category: ToolCategory::Shell,
    risk: ToolRisk::DynamicShell,
    requires: &[],
    parallel_safe: false,
};

async fn prepare_shell(_: &ToolContext<'_>, args: ShellToolArgs) -> Result<PreparedToolCall> {
    Ok(PreparedToolCall::shell(args))
}

define_tool!(ShellTool, ShellToolArgs, SHELL_META, prepare_shell);
