//! Explicit, model-facing adapters for built-in tools.

macro_rules! define_tool {
    ($adapter:ident, $args:ty, $metadata:ident, $prepare:path) => {
        pub(super) struct $adapter;

        #[async_trait::async_trait]
        impl crate::tools::Tool for $adapter {
            fn metadata(&self) -> &'static crate::tools::ToolMetadata {
                &$metadata
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
pub mod apk;
pub mod audio;
pub mod chart;
mod configuration;
mod extended;
pub mod file;
mod ima;
pub mod memory;
pub mod network;
pub mod runtime;
mod shell;
pub mod tailcat;
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
use chart::ChartTool;
use file::{ApplyPatchTool, ListDirTool, ReadFileTool, SearchTextTool};
use ima::{ImaListTool, ImaReadTool, ImaSearchTool};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use shell::ShellTool;
use std::{collections::HashMap, sync::OnceLock};

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

/// Model-facing tools available under a loaded configuration.
pub fn configured_tools(config: &Config) -> Vec<ToolDefinition> {
    let capabilities = if config.ima_enabled {
        &[Capability::Ima][..]
    } else {
        &[][..]
    };
    ToolRegistry::for_config(config, capabilities).definitions()
}

/// Discover the actual environment before advertising configured tools to an external client.
pub async fn available_tools(
    config: &Config,
    executor: &dyn CommandExecutor,
) -> Vec<ToolDefinition> {
    let runtime = crate::runtime::RuntimeCapabilities::discover(config, executor).await;
    ToolRegistry::for_runtime(config, &runtime).definitions()
}

/// Remove managed listeners from one-shot bridge processes, which cannot keep them alive.
pub fn disable_managed_tailcat_for_bridge(config: &mut Config) {
    for descriptor in builtin_descriptors()
        .iter()
        .filter(|tool| tool.lifetime == ToolLifetime::Process)
    {
        config.tool_overrides.insert(descriptor.name.into(), false);
    }
}

/// Names whose availability can be configured independently of their group.
pub fn optional_tool_names() -> &'static [&'static str] {
    static NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| {
        builtin_descriptors()
            .iter()
            .filter(|tool| tool.group.is_some())
            .map(|tool| tool.name)
            .collect()
    })
}

/// Optional group that owns a registered tool, if any.
pub fn optional_group(name: &str) -> Option<&'static str> {
    builtin_descriptors()
        .iter()
        .find(|tool| tool.name == name)?
        .group
        .map(ToolGroup::id)
}

/// Check the assigned optional group of a tool.
pub fn tool_in_group(name: &str, group: &str) -> bool {
    optional_group(name) == Some(group)
}

/// Effective availability of one optional tool.
pub fn tool_enabled(config: &Config, name: &str) -> bool {
    builtin_descriptors()
        .iter()
        .find(|tool| tool.name == name)
        .is_none_or(|tool| {
            tool.group.is_none_or(|group| {
                config
                    .tool_overrides
                    .get(name)
                    .copied()
                    .or_else(|| config.tool_groups.get(group.id()).copied())
                    .unwrap_or(tool.default_enabled)
            })
        })
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
    fn definition(&self) -> ToolDefinition {
        self.metadata().definition()
    }
    async fn prepare(&self, ctx: &ToolContext<'_>, arguments: Value) -> Result<PreparedToolCall>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
/// Runtime capability required before exposing a tool to the model.
pub enum Capability {
    /// Configured Tencent ima read-only connector.
    Ima,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
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
    /// nl2sh configuration management.
    Configuration,
    /// Presentation-only chart data.
    Chart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
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

/// Optional tool group identity; labels and IDs live alongside its descriptors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolGroup {
    /// Bounded APK analysis and optional JADX.
    Jadx,
    /// Explicit network sharing and transfer.
    Tailcat,
}
impl ToolGroup {
    /// Configuration key.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Jadx => "jadx",
            Self::Tailcat => "tailcat",
        }
    }
    /// Default group switch derived from its declared tools.
    pub fn default_enabled(self) -> bool {
        builtin_descriptors()
            .iter()
            .find(|tool| tool.group == Some(self))
            .is_some_and(|tool| tool.default_enabled)
    }
    /// Human-readable settings label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Jadx => "APK/JADX",
            Self::Tailcat => "Tailcat",
        }
    }
}

/// Environment prerequisite, independent of authorization to execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPlatform {
    /// Platform-independent operation.
    Any,
    /// Android userspace.
    Android,
    /// Current process has Android shell/root authority.
    AndroidShell,
    /// The installer supports Android and Linux.
    AndroidOrLinux,
}

/// Optional runtime prerequisite discovered without installing or prompting for root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRequirement {
    /// No extra executable.
    None,
    /// Tailcat has reported a version.
    Tailcat,
    /// JADX has an explicit or authenticated acquisition source.
    Jadx,
}

/// Scheduling policy shared by every execution entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolConcurrency {
    /// Independent, bounded read-only operation.
    Parallel,
    /// A call must run sequentially within its task.
    Sequential,
    /// Android focus/display/clipboard operation requires the task's device UI lease.
    AndroidUi,
    /// Arbitrary shell execution uses platform-specific resource coordination.
    Shell,
}

/// Whether the operation needs its caller process to stay alive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolLifetime {
    /// One completed call is sufficient.
    Call,
    /// A managed listener lives in this process.
    Process,
}

/// One source for identity, schema, settings, risk, availability and scheduling policy.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ToolDescriptor {
    /// Name presented to the model.
    pub name: &'static str,
    /// Description presented to the model and confirmation interface.
    pub description: &'static str,
    /// Tool domain.
    pub category: ToolCategory,
    /// Minimum local risk policy.
    pub risk: ToolRisk,
    /// Configured connector capabilities required to expose this tool.
    pub requires: &'static [Capability],
    /// Optional configuration group.
    pub group: Option<ToolGroup>,
    /// Default when neither a group nor per-tool override is present.
    pub default_enabled: bool,
    /// Device/platform prerequisite.
    pub platform: ToolPlatform,
    /// Optional executable prerequisite.
    pub runtime: RuntimeRequirement,
    /// Resource and parallel execution policy.
    pub concurrency: ToolConcurrency,
    /// Managed process requirement.
    pub lifetime: ToolLifetime,
    /// Schema derived from typed arguments, with operation-specific narrowing where needed.
    #[serde(skip)]
    pub schema: fn(&ToolDescriptor) -> Value,
}

/// Source-compatible name for existing adapters; both names refer to the same descriptor.
pub type ToolMetadata = ToolDescriptor;

pub(super) fn descriptor_schema<A: JsonSchema>(_: &ToolDescriptor) -> Value {
    definition::<A>("", "").parameters
}

impl ToolCategory {
    const fn platform(self) -> ToolPlatform {
        match self {
            Self::Android => ToolPlatform::Android,
            _ => ToolPlatform::Any,
        }
    }
}

impl ToolMetadata {
    /// Generate the model definition from this descriptor's typed schema.
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.into(),
            description: self.description.into(),
            parameters: (self.schema)(self),
        }
    }

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
        Some(SecurityAssessment::from_policy(
            risk_level,
            vec![MatchedRule {
                id: format!("structured-{}", self.name),
                message: self.description.into(),
            }],
            false,
            self.description.into(),
            false,
        ))
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

fn all_adapters() -> Vec<Box<dyn Tool>> {
    let mut tools: Vec<Box<dyn Tool>> = vec![
        Box::new(ShellTool),
        Box::new(apk::InspectApkTool),
        Box::new(apk::ListApkEntriesTool),
        Box::new(apk::ListDexClassesTool),
        Box::new(apk::DecompileApkClassTool),
        Box::new(apk::ListDexMethodsTool),
        Box::new(apk::SearchDexStringsTool),
        Box::new(apk::FindClassReferencesTool),
        Box::new(apk::FindMethodReferencesTool),
        Box::new(apk::InspectManifestTool),
        Box::new(apk::ListPermissionsTool),
        Box::new(apk::ListExportedComponentsTool),
        Box::new(apk::FindNativeLibsTool),
        Box::new(ReadFileTool),
        Box::new(ListDirTool),
        Box::new(SearchTextTool),
        Box::new(ApplyPatchTool),
        Box::new(ChartTool),
        Box::new(configuration::ConfigTool),
        Box::new(ImaListTool),
        Box::new(ImaSearchTool),
        Box::new(ImaReadTool),
    ];
    tools.extend(tailcat::builtin_tools());
    tools.extend(extended::builtin_tools());
    tools.extend(android::tool::builtin_tools());
    tools
}

/// Complete descriptor inventory, including disabled groups and unconfigured connectors.
pub fn builtin_descriptors() -> &'static [ToolDescriptor] {
    static DESCRIPTORS: OnceLock<Vec<ToolDescriptor>> = OnceLock::new();
    DESCRIPTORS.get_or_init(|| all_adapters().iter().map(|tool| *tool.metadata()).collect())
}

/// Optional groups derived from the descriptor inventory in catalog order.
pub fn optional_groups() -> &'static [ToolGroup] {
    static GROUPS: OnceLock<Vec<ToolGroup>> = OnceLock::new();
    GROUPS.get_or_init(|| {
        let mut groups = Vec::new();
        for group in builtin_descriptors().iter().filter_map(|tool| tool.group) {
            if !groups.contains(&group) {
                groups.push(group);
            }
        }
        groups
    })
}

/// An explicit, ordered list of adapters available to this task.
pub(crate) struct ToolRegistry {
    tools: Vec<(ToolDefinition, Box<dyn Tool>)>,
}

impl ToolRegistry {
    #[cfg(test)]
    pub(crate) fn from_test_adapters(adapters: Vec<Box<dyn Tool>>) -> Self {
        Self {
            tools: adapters
                .into_iter()
                .map(|tool| (tool.definition(), tool))
                .collect(),
        }
    }

    pub fn builtin(capabilities: &[Capability]) -> Self {
        Self::for_config(&Config::default(), capabilities)
    }

    /// Register only tools enabled by the current configuration.
    pub fn for_config(config: &Config, capabilities: &[Capability]) -> Self {
        Self::build(Some(config), capabilities)
    }

    /// Inventory all tools for settings, including disabled optional tools.
    pub fn catalog(capabilities: &[Capability]) -> Self {
        Self::build(None, capabilities)
    }

    fn build(config: Option<&Config>, capabilities: &[Capability]) -> Self {
        let tools = all_adapters();
        Self {
            tools: tools
                .into_iter()
                .filter(|tool| {
                    tool.metadata().available(capabilities)
                        && config.is_none_or(|config| tool_enabled(config, tool.metadata().name))
                })
                .map(|tool| (tool.definition(), tool))
                .collect(),
        }
    }

    /// Register only configured tools whose runtime prerequisites were discovered.
    pub fn for_runtime(config: &Config, runtime: &crate::runtime::RuntimeCapabilities) -> Self {
        let mut registry = Self::for_config(config, &runtime.configured());
        registry
            .tools
            .retain(|(_, tool)| runtime.supports(tool.metadata()));
        registry
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
    fn descriptors_drive_catalog_settings_and_runtime_policy() -> Result<()> {
        let catalog = ToolRegistry::catalog(&[Capability::Ima]);
        let descriptors = super::builtin_descriptors();
        let mut names = std::collections::HashSet::new();
        for descriptor in descriptors {
            assert!(names.insert(descriptor.name), "duplicate descriptor");
            assert_eq!(
                descriptor.definition(),
                catalog
                    .get(descriptor.name)
                    .context("missing descriptor dispatch")?
                    .definition()
            );
            assert_eq!(
                super::optional_group(descriptor.name),
                descriptor.group.map(super::ToolGroup::id)
            );
            assert_eq!(
                super::tool_enabled(&crate::config::Config::default(), descriptor.name),
                descriptor.default_enabled
            );
            if descriptor.concurrency == super::ToolConcurrency::Parallel {
                assert_eq!(descriptor.risk, ToolRisk::ReadOnly);
            }
        }
        assert_eq!(catalog.definitions().len(), descriptors.len());
        assert!(descriptors
            .iter()
            .filter(|tool| tool.name.starts_with("android."))
            .all(|tool| tool.platform == super::ToolPlatform::AndroidShell
                && tool.concurrency == super::ToolConcurrency::AndroidUi));
        Ok(())
    }

    #[test]
    fn registry_definitions_match_available_dispatch() {
        let registry = ToolRegistry::builtin(&[]);
        let names = registry
            .definitions()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 54);
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
            "nl2sh_config",
            "inspect_tls",
        ] {
            assert!(names.contains(&name.to_string()), "missing {name}");
        }
        assert!(registry.get("read_file").is_some());
        assert!(registry.get("ima_search").is_none());
        assert!(registry.get("inspect_apk").is_none());
        assert!(registry.get("tailcat_check").is_none());
        assert!(registry.get("tailcat_install").is_none());
        assert!(ToolRegistry::builtin(&[Capability::Ima])
            .get("ima_search")
            .is_some());
        assert_eq!(builtin_tools(true).len(), names.len() + 3);
    }

    #[test]
    fn optional_groups_and_individual_overrides_gate_definitions_and_dispatch() {
        let mut config = crate::config::Config::default();
        let registry = ToolRegistry::for_config(&config, &[]);
        for name in super::optional_tool_names() {
            assert!(
                registry.get(name).is_none(),
                "{name} was exposed by default"
            );
        }
        config.tool_groups.insert("jadx".into(), true);
        config.tool_groups.insert("tailcat".into(), true);
        config
            .tool_overrides
            .insert("decompile_apk_class".into(), false);
        config.tool_overrides.insert("tailcat_serve".into(), false);
        let registry = ToolRegistry::for_config(&config, &[]);
        assert!(registry.get("inspect_apk").is_some());
        assert!(registry.get("tailcat_check").is_some());
        assert!(registry.get("tailcat_install").is_some());
        assert!(registry.get("tailcat_adb_pair").is_some());
        assert!(registry.get("decompile_apk_class").is_none());
        assert!(registry.get("tailcat_serve").is_none());
        config.tool_groups.insert("tailcat".into(), false);
        config.tool_overrides.insert("tailcat_check".into(), true);
        let registry = ToolRegistry::for_config(&config, &[]);
        assert!(registry.get("tailcat_check").is_some());
        assert!(registry.get("tailcat_receive").is_none());
        assert!(registry
            .definitions()
            .iter()
            .all(|definition| definition.name != "tailcat_receive"));
        super::disable_managed_tailcat_for_bridge(&mut config);
        let bridge = ToolRegistry::for_config(&config, &[]);
        assert!(bridge.get("tailcat_check").is_some());
        assert!(bridge.get("tailcat_receive").is_none());
        assert!(bridge.get("tailcat_serve").is_none());
        assert!(bridge.get("tailcat_adb_pair").is_none());
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

    #[test]
    fn agent_memory_schema_lists_every_supported_action() {
        let definition = builtin_tools(false)
            .into_iter()
            .find(|tool| tool.name == "agent_memory");
        let actions = definition.as_ref().and_then(|tool| {
            let reference = tool.parameters["properties"]["action"]["$ref"].as_str()?;
            let name = reference.strip_prefix("#/$defs/")?;
            let choices = tool.parameters["$defs"][name]["oneOf"].as_array()?;
            Some(
                choices
                    .iter()
                    .filter_map(|choice| choice["const"].as_str())
                    .collect::<Vec<_>>(),
            )
        });
        assert_eq!(actions, Some(vec!["get", "list", "set", "delete", "clear"]));
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
            group: None,
            default_enabled: true,
            platform: crate::tools::ToolPlatform::Any,
            runtime: crate::tools::RuntimeRequirement::None,
            concurrency: crate::tools::ToolConcurrency::Sequential,
            lifetime: crate::tools::ToolLifetime::Call,
            schema: crate::tools::descriptor_schema::<crate::tools::ShellToolArgs>,
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
        assert!(!directory
            .path()
            .join("memory/agent-memory.sqlite3")
            .exists());
        Ok(())
    }
}
