//! Explicit, model-facing adapters for built-in tools.

macro_rules! define_tool {
    ($adapter:ident, $args:ty, $metadata:ident, $prepare:path) => {
        pub(super) struct $adapter;

        #[async_trait::async_trait]
        impl crate::tools::Tool for $adapter {
            fn metadata(&self) -> &'static crate::tools::ToolMetadata {
                &$metadata
            }

            fn definition(&self) -> crate::llm::ToolDefinition {
                crate::tools::definition::<$args>($metadata.name, $metadata.description)
            }

            async fn prepare(
                &self,
                ctx: &crate::tools::ToolContext<'_>,
                arguments: serde_json::Value,
            ) -> anyhow::Result<crate::tools::PreparedToolCall> {
                let args: $args = crate::tools::parse_args($metadata.name, arguments)?;
                $prepare(ctx, args).await
            }
        }
    };
}

pub mod android;
pub mod audio;
mod extended;
pub mod file;
mod ima;
pub mod memory;
pub mod network;
mod shell;
pub mod ui;

use self::{audio::domain::AudioToolExecutor, file::domain::FileToolExecutor};
use crate::{
    agent::{Confirmer, TaskRuntime},
    config::Config,
    ima::ImaClient,
    llm::{LlmClient, ToolAttachment, ToolDefinition},
    security::{MatchedRule, RiskLevel, SecurityAssessment},
    shell::CommandExecutor,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use file::{ApplyPatchTool, ListDirTool, ReadFileTool, SearchTextTool};
use ima::{ImaListTool, ImaReadTool, ImaSearchTool};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use shell::ShellTool;
use std::collections::HashMap;

/// Validated arguments accepted from the built-in shell function tool.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShellToolArgs {
    /// Shell source to assess locally.
    pub command: String,
    /// Model explanation, informational only.
    #[serde(default)]
    pub reason: String,
    /// Model interaction hint; local detection remains authoritative too.
    #[serde(default)]
    pub interactive: bool,
    /// Model privilege hint; never directly authorizes root elevation.
    #[serde(default)]
    pub requires_root: bool,
}

pub(super) fn definition<A: JsonSchema>(name: &str, description: &str) -> ToolDefinition {
    let mut parameters = schemars::schema_for!(A).to_value();
    if let Some(object) = parameters.as_object_mut() {
        object.remove("$schema");
        object.remove("title");
        object.remove("description");
    }
    ToolDefinition {
        name: name.into(),
        description: description.into(),
        parameters,
    }
}

pub(super) fn parse_args<A: DeserializeOwned>(name: &str, arguments: Value) -> Result<A> {
    serde_json::from_value(arguments).with_context(|| format!("invalid {name} arguments"))
}

/// Returns the JSON-schema definition for the shell tool.
pub fn command_tool() -> ToolDefinition {
    ShellTool.definition()
}

/// Returns all built-in tools exposed to the model.
pub fn builtin_tools(ima_enabled: bool) -> Vec<ToolDefinition> {
    let capabilities = if ima_enabled {
        &[Capability::Ima][..]
    } else {
        &[][..]
    };
    ToolRegistry::builtin(capabilities).definitions()
}

pub(crate) struct ToolContext<'a> {
    pub file_tools: &'a FileToolExecutor,
    pub ima: Option<&'a ImaClient>,
    pub config: Option<&'a Config>,
    pub executor: Option<&'a dyn CommandExecutor>,
    pub llm: Option<&'a dyn LlmClient>,
    pub confirmer: Option<&'a dyn Confirmer>,
    pub audio_tools: Option<&'a AudioToolExecutor>,
    pub runtime: Option<&'a mut TaskRuntime>,
    pub audio_cache: Option<&'a mut HashMap<String, Value>>,
}

pub(crate) struct ToolOutput {
    pub content: String,
    pub success: bool,
    pub attachments: Vec<ToolAttachment>,
}

impl ToolOutput {
    pub(super) fn success(content: String) -> Self {
        Self {
            content,
            success: true,
            attachments: Vec::new(),
        }
    }

    pub(crate) fn refused(content: &str) -> Self {
        Self {
            content: content.into(),
            success: false,
            attachments: Vec::new(),
        }
    }
}

#[async_trait]
pub(crate) trait Tool: Send + Sync {
    fn metadata(&self) -> &'static ToolMetadata;
    fn definition(&self) -> ToolDefinition;
    async fn prepare(&self, ctx: &ToolContext<'_>, arguments: Value) -> Result<PreparedToolCall>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Runtime capability required before exposing a tool to the model.
pub enum Capability {
    /// Configured Tencent ima read-only connector.
    Ima,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Stable grouping for model-facing tools.
pub enum ToolCategory {
    /// Device shell command.
    Shell,
    /// Structured file operation.
    File,
    /// Read-only knowledge source.
    Knowledge,
    /// Deterministic audio analysis and quality judgment.
    Audio,
    /// Android diagnostics and interaction.
    Android,
    /// Network access with local target restrictions.
    Network,
    /// Private Agent state.
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
/// Local minimum security policy for a tool.
pub enum ToolRisk {
    /// Classify each shell command using the shell security engine.
    DynamicShell,
    /// No modification is expected.
    ReadOnly,
    /// Modification requiring confirmation.
    Mutating,
    /// Dangerous operation requiring strong confirmation.
    Dangerous,
    /// Critical operation requiring strong confirmation.
    Critical,
}

/// Local tool identity, availability, and minimum risk.
pub struct ToolMetadata {
    /// Name presented to the model.
    pub name: &'static str,
    /// Description presented to the model and confirmation interface.
    pub description: &'static str,
    /// Tool domain.
    pub category: ToolCategory,
    /// Minimum local risk policy.
    pub risk: ToolRisk,
    /// Capabilities required to expose this tool.
    pub requires: &'static [Capability],
    /// Whether independent calls may eventually run concurrently.
    pub parallel_safe: bool,
}

impl ToolMetadata {
    /// Returns whether every declared capability is currently available.
    pub fn available(&self, capabilities: &[Capability]) -> bool {
        self.requires.iter().all(|need| capabilities.contains(need))
    }

    /// Creates the local security assessment for a structured tool.
    pub fn assessment(&self) -> Option<SecurityAssessment> {
        self.assessment_for(self.risk)
    }

    /// Builds an assessment for a prepared call while enforcing this tool's risk floor.
    pub fn assessment_for(&self, prepared_risk: ToolRisk) -> Option<SecurityAssessment> {
        let risk = self.risk.max(prepared_risk);
        let risk_level = match risk {
            ToolRisk::DynamicShell => return None,
            ToolRisk::ReadOnly => RiskLevel::ReadOnly,
            ToolRisk::Mutating => RiskLevel::Mutating,
            ToolRisk::Dangerous => RiskLevel::Dangerous,
            ToolRisk::Critical => RiskLevel::Critical,
        };
        Some(SecurityAssessment {
            risk_level,
            matched_rules: vec![MatchedRule {
                id: format!("structured-{}", self.name),
                message: self.description.into(),
            }],
            requires_confirmation: risk_level >= RiskLevel::Mutating,
            requires_double_confirmation: risk_level >= RiskLevel::Dangerous,
            requires_root: false,
            explanation: self.description.into(),
        })
    }
}

#[async_trait]
pub(crate) trait PreparedExecution: Send {
    async fn execute(self: Box<Self>, ctx: &mut ToolContext<'_>) -> Result<ToolOutput>;
}

pub(crate) enum PreparedAction {
    Shell(ShellToolArgs),
    Operation(Box<dyn PreparedExecution>),
}

pub(crate) struct PreparedToolCall {
    pub preview: String,
    pub action: PreparedAction,
    pub risk: Option<ToolRisk>,
}

impl PreparedToolCall {
    pub fn operation(preview: String, action: Box<dyn PreparedExecution>) -> Self {
        Self {
            preview,
            action: PreparedAction::Operation(action),
            risk: None,
        }
    }

    pub fn operation_with_risk(
        preview: String,
        action: Box<dyn PreparedExecution>,
        risk: ToolRisk,
    ) -> Self {
        Self {
            preview,
            action: PreparedAction::Operation(action),
            risk: Some(risk),
        }
    }

    pub fn shell(args: ShellToolArgs) -> Self {
        Self {
            preview: args.command.clone(),
            action: PreparedAction::Shell(args),
            risk: None,
        }
    }
}

/// An explicit, ordered list of adapters available to this task.
pub(crate) struct ToolRegistry {
    tools: Vec<(ToolDefinition, Box<dyn Tool>)>,
}

impl ToolRegistry {
    pub fn builtin(capabilities: &[Capability]) -> Self {
        let mut tools: Vec<Box<dyn Tool>> = vec![
            Box::new(ShellTool),
            Box::new(ReadFileTool),
            Box::new(ListDirTool),
            Box::new(SearchTextTool),
            Box::new(ApplyPatchTool),
            Box::new(ImaListTool),
            Box::new(ImaSearchTool),
            Box::new(ImaReadTool),
        ];
        tools.extend(extended::builtin_tools());
        Self {
            tools: tools
                .into_iter()
                .filter(|tool| tool.metadata().available(capabilities))
                .map(|tool| (tool.definition(), tool))
                .collect(),
        }
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .map(|(definition, _)| definition.clone())
            .collect()
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|(definition, _)| definition.name == name)
            .map(|(_, tool)| tool.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        builtin_tools, Capability, ToolCategory, ToolContext, ToolMetadata, ToolRegistry, ToolRisk,
    };
    use crate::{security::RiskLevel, tools::file::domain::FileToolExecutor};
    use anyhow::{Context, Result};
    use serde_json::json;
    use std::fs;

    #[test]
    fn registry_definitions_match_available_dispatch() {
        let registry = ToolRegistry::builtin(&[]);
        let names = registry
            .definitions()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 36);
        assert_eq!(
            names.iter().collect::<std::collections::HashSet<_>>().len(),
            names.len()
        );
        for name in [
            "execute_shell_command",
            "apply_patch",
            "analyze_audio",
            "inspect_android_environment",
            "android_clipboard",
            "agent_memory",
            "inspect_tls",
        ] {
            assert!(names.contains(&name.to_string()), "missing {name}");
        }
        assert!(registry.get("read_file").is_some());
        assert!(registry.get("ima_search").is_none());
        assert!(ToolRegistry::builtin(&[Capability::Ima])
            .get("ima_search")
            .is_some());
        assert_eq!(builtin_tools(true).len(), names.len() + 3);
    }

    #[test]
    fn generated_schemas_match_deserialization_contract() -> Result<()> {
        let definitions = builtin_tools(true);
        for tool in &definitions {
            assert_eq!(tool.parameters["type"], "object", "{} schema", tool.name);
            assert_eq!(
                tool.parameters["additionalProperties"], false,
                "{} must reject unknown arguments",
                tool.name
            );
        }
        let read = definitions
            .iter()
            .find(|tool| tool.name == "read_file")
            .context("read_file definition missing")?;
        assert_eq!(read.parameters["required"], json!(["path"]));
        assert_eq!(read.parameters["additionalProperties"], false);
        assert_eq!(read.parameters["properties"]["path"]["type"], "string");

        let search = definitions
            .iter()
            .find(|tool| tool.name == "search_text")
            .context("search_text definition missing")?;
        assert_eq!(search.parameters["required"], json!(["query"]));
        assert_eq!(search.parameters["properties"]["path"]["default"], ".");

        let shell = definitions
            .iter()
            .find(|tool| tool.name == "execute_shell_command")
            .context("shell definition missing")?;
        assert_eq!(shell.parameters["required"], json!(["command"]));
        assert_eq!(shell.parameters["additionalProperties"], false);
        Ok(())
    }

    #[tokio::test]
    async fn patch_prepare_only_builds_a_preview() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("a.txt");
        fs::write(&path, "before")?;
        let file_tools = FileToolExecutor::new(directory.path())?;
        let registry = ToolRegistry::builtin(&[]);
        let patch = registry.get("apply_patch").context("patch tool missing")?;
        let args = json!({"path":"a.txt","old_text":"before","new_text":"after"});
        let ctx = ToolContext {
            file_tools: &file_tools,
            ima: None,
            config: None,
            executor: None,
            llm: None,
            confirmer: None,
            audio_tools: None,
            runtime: None,
            audio_cache: None,
        };
        let prepared = patch.prepare(&ctx, args).await?;
        assert!(prepared.preview.contains("-before"));
        assert!(prepared.preview.contains("+after"));
        assert_eq!(patch.metadata().risk, ToolRisk::Mutating);
        assert!(
            patch
                .metadata()
                .assessment()
                .context("assessment missing")?
                .requires_confirmation
        );
        assert_eq!(fs::read_to_string(&path)?, "before");
        Ok(())
    }

    #[test]
    fn dangerous_metadata_requires_strong_confirmation() -> Result<()> {
        let metadata = ToolMetadata {
            name: "test_danger",
            description: "dangerous test operation",
            category: ToolCategory::File,
            risk: ToolRisk::Dangerous,
            requires: &[],
            parallel_safe: false,
        };
        let assessment = metadata.assessment().context("assessment missing")?;
        assert_eq!(assessment.risk_level, RiskLevel::Dangerous);
        assert!(assessment.requires_confirmation);
        assert!(assessment.requires_double_confirmation);
        Ok(())
    }

    #[tokio::test]
    async fn mixed_read_write_tools_raise_risk_only_for_mutations() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let file_tools = FileToolExecutor::new(directory.path())?;
        let registry = ToolRegistry::builtin(&[]);
        let ctx = ToolContext {
            file_tools: &file_tools,
            ima: None,
            config: None,
            executor: None,
            llm: None,
            confirmer: None,
            audio_tools: None,
            runtime: None,
            audio_cache: None,
        };
        for (name, read_args, write_args) in [
            (
                "android_clipboard",
                json!({}),
                json!({"text":"prepared only"}),
            ),
            (
                "android_media_control",
                json!({"action":"status"}),
                json!({"action":"pause"}),
            ),
            (
                "agent_memory",
                json!({"action":"list"}),
                json!({"action":"set","key":"test","value":"prepared only"}),
            ),
        ] {
            let tool = registry
                .get(name)
                .with_context(|| format!("missing {name}"))?;
            let read = tool.prepare(&ctx, read_args).await?;
            assert_eq!(read.risk, None, "{name} read risk");
            let write = tool.prepare(&ctx, write_args).await?;
            assert_eq!(write.risk, Some(ToolRisk::Mutating), "{name} write risk");
            assert!(!write.preview.is_empty(), "{name} missing approval preview");
            assert!(
                tool.metadata()
                    .assessment_for(write.risk.unwrap_or(tool.metadata().risk))
                    .context("assessment missing")?
                    .requires_confirmation
            );
        }
        assert!(!directory.path().join(".nl2sh-agent-memory.json").exists());
        Ok(())
    }
}
