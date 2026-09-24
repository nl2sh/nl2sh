//! Agent adapters and bounded structured file operations.

pub mod domain;

use self::domain::{
    ApplyPatchArgs, FileToolExecutor, ListDirArgs, PreparedPatch, ReadFileArgs, SearchTextArgs,
};
use super::{
    PreparedExecution, PreparedToolCall, ToolCategory, ToolContext, ToolMetadata, ToolOutput,
    ToolRisk,
};
use anyhow::{Context, Result};
use async_trait::async_trait;

const READ_META: ToolMetadata = ToolMetadata {
    name: "read_file",
    description: "Read a size-limited UTF-8 text file. Absolute paths, parent components, and symlinks are supported.",
    category: ToolCategory::File,
    risk: ToolRisk::ReadOnly,
    requires: &[],
    parallel_safe: true,
};

const LIST_META: ToolMetadata = ToolMetadata {
    name: "list_dir",
    description: "List a bounded number of direct children without using shell commands. Absolute paths are supported.",
    category: ToolCategory::File,
    risk: ToolRisk::ReadOnly,
    requires: &[],
    parallel_safe: true,
};

const SEARCH_META: ToolMetadata = ToolMetadata {
    name: "search_text",
    description: "Search recursively for literal text in bounded UTF-8 files. Paths are not confined to the current workspace and symlinks are followed with cycle detection.",
    category: ToolCategory::File,
    risk: ToolRisk::ReadOnly,
    requires: &[],
    parallel_safe: true,
};

const PATCH_META: ToolMetadata = ToolMetadata {
    name: "apply_patch",
    description: "Replace exactly one occurrence of old_text in any accessible file, or create a file when old_text is empty. A diff is always shown for local user confirmation before writing.",
    category: ToolCategory::File,
    risk: ToolRisk::Mutating,
    requires: &[],
    parallel_safe: false,
};

struct FileReadOperation<A> {
    args: A,
    run: fn(&FileToolExecutor, &A) -> Result<String>,
}

#[async_trait]
impl<A: Send + 'static> PreparedExecution for FileReadOperation<A> {
    async fn execute(self: Box<Self>, ctx: &mut ToolContext<'_>) -> Result<ToolOutput> {
        let tools = ctx.file_tools.clone();
        let content = tokio::task::spawn_blocking(move || (self.run)(&tools, &self.args))
            .await
            .context("file tool worker failed")??;
        Ok(ToolOutput::success(content))
    }
}

async fn prepare_read(_: &ToolContext<'_>, args: ReadFileArgs) -> Result<PreparedToolCall> {
    Ok(PreparedToolCall::operation(
        String::new(),
        Box::new(FileReadOperation {
            args,
            run: FileToolExecutor::read_file,
        }),
    ))
}

async fn prepare_list(_: &ToolContext<'_>, args: ListDirArgs) -> Result<PreparedToolCall> {
    Ok(PreparedToolCall::operation(
        String::new(),
        Box::new(FileReadOperation {
            args,
            run: FileToolExecutor::list_dir,
        }),
    ))
}

async fn prepare_search(_: &ToolContext<'_>, args: SearchTextArgs) -> Result<PreparedToolCall> {
    Ok(PreparedToolCall::operation(
        String::new(),
        Box::new(FileReadOperation {
            args,
            run: FileToolExecutor::search_text,
        }),
    ))
}

struct PatchOperation(PreparedPatch);

#[async_trait]
impl PreparedExecution for PatchOperation {
    async fn execute(self: Box<Self>, _: &mut ToolContext<'_>) -> Result<ToolOutput> {
        tokio::task::spawn_blocking(move || self.0.apply())
            .await
            .context("apply_patch worker failed")??;
        Ok(ToolOutput::success(
            "Patch applied after user confirmed the displayed diff.".into(),
        ))
    }
}

async fn prepare_patch(ctx: &ToolContext<'_>, args: ApplyPatchArgs) -> Result<PreparedToolCall> {
    let tools = ctx.file_tools.clone();
    let patch = tokio::task::spawn_blocking(move || tools.prepare_patch(&args))
        .await
        .context("apply_patch prepare worker failed")??;
    Ok(PreparedToolCall::operation(
        patch.diff.clone(),
        Box::new(PatchOperation(patch)),
    ))
}

define_tool!(ReadFileTool, ReadFileArgs, READ_META, prepare_read);
define_tool!(ListDirTool, ListDirArgs, LIST_META, prepare_list);
define_tool!(SearchTextTool, SearchTextArgs, SEARCH_META, prepare_search);
define_tool!(ApplyPatchTool, ApplyPatchArgs, PATCH_META, prepare_patch);
