//! Bounded, read-only APK inspection and optional JADX class decompilation.

mod analysis;
mod dex;
mod manifest;
pub(crate) use analysis::*;

use super::{
    PreparedExecution, PreparedToolCall, ToolCategory, ToolContext, ToolMetadata, ToolOutput,
    ToolRisk,
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};
use zip::ZipArchive;

const MAX_ENTRIES: usize = 50_000;
const MAX_APK_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_DEX_BYTES: u64 = 32 * 1024 * 1024;
/// File-count ceiling only. Decompression and parse work stay bounded by [`MAX_DEX_BYTES`] per
/// entry and by the aggregate budget in `analysis::scan_dex`, so a higher count does not raise
/// peak memory; shipping APKs routinely exceed a few dozen DEX files.
const MAX_DEX_FILES: usize = 256;

const INSPECT_META: ToolMetadata = ToolMetadata {
    name: "inspect_apk",
    description: "Inspect a local APK archive: size, file counts, DEX files, manifest presence, and native ABIs. Does not execute APK contents.",
    category: ToolCategory::File,
    risk: ToolRisk::ReadOnly,
    requires: &[],
    group: Some(crate::tools::ToolGroup::Jadx), default_enabled: false,
            platform: crate::tools::ToolPlatform::Any, runtime: crate::tools::RuntimeRequirement::None,
            concurrency: crate::tools::ToolConcurrency::Parallel, lifetime: crate::tools::ToolLifetime::Call,
            schema: crate::tools::descriptor_schema::<ApkPathArgs>,
};
const ENTRIES_META: ToolMetadata = ToolMetadata {
    name: "list_apk_entries",
    description:
        "List bounded APK ZIP entries by optional literal path prefix, without extracting files.",
    category: ToolCategory::File,
    risk: ToolRisk::ReadOnly,
    requires: &[],
    group: Some(crate::tools::ToolGroup::Jadx),
    default_enabled: false,
    platform: crate::tools::ToolPlatform::Any,
    runtime: crate::tools::RuntimeRequirement::None,
    concurrency: crate::tools::ToolConcurrency::Parallel,
    lifetime: crate::tools::ToolLifetime::Call,
    schema: crate::tools::descriptor_schema::<ApkEntriesArgs>,
};
const CLASSES_META: ToolMetadata = ToolMetadata {
    name: "list_dex_classes",
    description: "List class names from bounded DEX tables inside a local APK; optional literal class-name filter.",
    category: ToolCategory::File,
    risk: ToolRisk::ReadOnly,
    requires: &[],
    group: Some(crate::tools::ToolGroup::Jadx), default_enabled: false,
            platform: crate::tools::ToolPlatform::Any, runtime: crate::tools::RuntimeRequirement::None,
            concurrency: crate::tools::ToolConcurrency::Parallel, lifetime: crate::tools::ToolLifetime::Call,
            schema: crate::tools::descriptor_schema::<DexClassesArgs>,
};
const DECOMPILE_META: ToolMetadata = ToolMetadata {
    name: "decompile_apk_class",
    description: "Decompile one exact APK class using an already installed Android DEX helper through app_process. Strong confirmation required. Does not download; if no helper is installed, use jadx_install after approval, then retry.",
    category: ToolCategory::File,
    risk: ToolRisk::Dangerous,
    requires: &[],
    group: Some(crate::tools::ToolGroup::Jadx), default_enabled: false,
            platform: crate::tools::ToolPlatform::Android, runtime: crate::tools::RuntimeRequirement::Jadx,
            concurrency: crate::tools::ToolConcurrency::Sequential, lifetime: crate::tools::ToolLifetime::Call,
            schema: crate::tools::descriptor_schema::<DecompileClassArgs>,
};
const JADX_CHECK_META: ToolMetadata = ToolMetadata {
    name: "jadx_check",
    description: "Report the Android DEX helper source, its authentication, and whether a validated helper is installed. Read-only; downloads nothing and runs no helper.",
    category: ToolCategory::File,
    risk: ToolRisk::ReadOnly,
    requires: &[],
    group: Some(crate::tools::ToolGroup::Jadx), default_enabled: false,
            platform: crate::tools::ToolPlatform::Android, runtime: crate::tools::RuntimeRequirement::None,
            concurrency: crate::tools::ToolConcurrency::Parallel, lifetime: crate::tools::ToolLifetime::Call,
            schema: crate::tools::descriptor_schema::<JadxHelperArgs>,
};
const JADX_INSTALL_META: ToolMetadata = ToolMetadata {
    name: "jadx_install",
    description: "Download, verify, and cache the pinned Android DEX helper so decompile_apk_class can run. Mutating and Android-only. Use jadx_check first to see the exact source and digest.",
    category: ToolCategory::Network,
    risk: ToolRisk::Mutating,
    requires: &[],
    group: Some(crate::tools::ToolGroup::Jadx), default_enabled: false,
            platform: crate::tools::ToolPlatform::Android, runtime: crate::tools::RuntimeRequirement::None,
            concurrency: crate::tools::ToolConcurrency::Sequential, lifetime: crate::tools::ToolLifetime::Call,
            schema: crate::tools::descriptor_schema::<JadxHelperArgs>,
};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApkPathArgs {
    /// Path to an existing local APK file.
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApkEntriesArgs {
    /// Path to an existing local APK file.
    pub path: String,
    /// Optional literal ZIP entry prefix.
    #[serde(default)]
    pub prefix: String,
    /// Maximum returned entries, from 1 to 200.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DexClassesArgs {
    /// Path to an existing local APK file.
    pub path: String,
    /// Optional literal substring of the dotted class name.
    #[serde(default)]
    pub query: String,
    /// Maximum returned classes, from 1 to 200.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecompileClassArgs {
    /// Path to an existing local APK file.
    pub path: String,
    /// Exact dotted class name, such as com.example.MainActivity.
    pub class_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JadxHelperArgs {}

fn default_limit() -> usize {
    50
}

fn validate_path(path: &str) -> Result<()> {
    if path.is_empty() || path.len() > 4096 {
        bail!("APK path is empty or too long")
    }
    let meta = std::fs::metadata(path).with_context(|| format!("cannot access APK {path}"))?;
    if !meta.is_file() {
        bail!("APK path is not a regular file")
    }
    if meta.len() > MAX_APK_BYTES {
        bail!("APK exceeds 2 GiB inspection limit")
    }
    Ok(())
}

fn archive(path: &str) -> Result<ZipArchive<File>> {
    validate_path(path)?;
    let mut file = File::open(path).with_context(|| format!("cannot open APK {path}"))?;
    validate_zip_directory(&mut file)?;
    let zip = ZipArchive::new(file).context("APK is not a valid ZIP archive")?;
    if zip.len() > MAX_ENTRIES {
        bail!("APK contains too many entries")
    }
    Ok(zip)
}

// Check directory bounds before the ZIP crate allocates its entry inventory.
fn validate_zip_directory(file: &mut File) -> Result<()> {
    let metadata = file.metadata().context("cannot inspect opened APK")?;
    if !metadata.is_file() || metadata.len() > MAX_APK_BYTES {
        bail!("opened APK exceeds inspection limit")
    }
    let length = metadata.len();
    let tail_length = length.min(65_557) as usize;
    file.seek(SeekFrom::End(-(tail_length as i64)))?;
    let mut tail = vec![0; tail_length];
    file.read_exact(&mut tail)?;
    let position = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|position| {
            tail.get(*position..*position + 4) == Some(b"PK\x05\x06")
                && crate::tools::apk::dex::u16_at(&tail, *position + 20)
                    .is_ok_and(|comment| *position + 22 + comment == tail.len())
        })
        .context("APK ZIP end directory is missing or truncated")?;
    let eocd = &tail[position..];
    if dex::u16_at(eocd, 4)? != 0 || dex::u16_at(eocd, 6)? != 0 {
        bail!("multipart APK ZIP is unsupported")
    }
    let mut count = dex::u16_at(eocd, 10)? as u64;
    let mut directory_size = u32_at(eocd, 12)? as u64;
    let mut directory_offset = u32_at(eocd, 16)? as u64;
    if count == u16::MAX as u64
        || directory_size == u32::MAX as u64
        || directory_offset == u32::MAX as u64
    {
        let absolute = length - tail_length as u64 + position as u64;
        file.seek(SeekFrom::Start(
            absolute.checked_sub(20).context("missing ZIP64 locator")?,
        ))?;
        let mut locator = [0; 20];
        file.read_exact(&mut locator)?;
        if !locator.starts_with(b"PK\x06\x07")
            || u32_at(&locator, 4)? != 0
            || u32_at(&locator, 16)? != 1
        {
            bail!("invalid APK ZIP64 locator")
        }
        let word = |bytes: &[u8], offset: usize| -> Result<u64> {
            let data: [u8; 8] = bytes
                .get(offset..offset + 8)
                .context("truncated ZIP64 word")?
                .try_into()
                .context("invalid ZIP64 word")?;
            Ok(u64::from_le_bytes(data))
        };
        let offset = word(&locator, 8)?;
        if offset.checked_add(56).is_none_or(|end| end > absolute - 20) {
            bail!("invalid APK ZIP64 directory offset")
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut directory = [0; 56];
        file.read_exact(&mut directory)?;
        if !directory.starts_with(b"PK\x06\x06")
            || word(&directory, 4)? < 44
            || u32_at(&directory, 16)? != 0
            || u32_at(&directory, 20)? != 0
            || word(&directory, 24)? != word(&directory, 32)?
        {
            bail!("invalid APK ZIP64 end directory")
        }
        count = word(&directory, 32)?;
        directory_size = word(&directory, 40)?;
        directory_offset = word(&directory, 48)?;
    } else if dex::u16_at(eocd, 8)? as u64 != count {
        bail!("APK ZIP directory entry count mismatch")
    }
    if count > MAX_ENTRIES as u64
        || directory_size > 32 * 1024 * 1024
        || directory_offset
            .checked_add(directory_size)
            .is_none_or(|end| end > length)
    {
        bail!("APK ZIP directory exceeds inspection limit")
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(())
}

fn limit(value: usize) -> Result<usize> {
    if !(1..=200).contains(&value) {
        bail!("limit must be from 1 to 200")
    }
    Ok(value)
}

fn inspect(args: &ApkPathArgs) -> Result<String> {
    let mut zip = archive(&args.path)?;
    let mut dex = Vec::new();
    let mut dex_count = 0usize;
    let mut abis = std::collections::BTreeSet::new();
    let mut manifest = false;
    let mut resources = false;
    for index in 0..zip.len() {
        let entry = zip.by_index(index).context("cannot inspect APK entry")?;
        let name = entry.name();
        if name == "AndroidManifest.xml" {
            manifest = true;
        }
        if name == "resources.arsc" {
            resources = true;
        }
        if is_dex(name) {
            dex_count += 1;
            if dex.len() < 32 {
                dex.push(
                    json!({"name":name.chars().take(512).collect::<String>(),"size":entry.size()}),
                );
            }
        }
        if let Some(abi) = name.strip_prefix("lib/").and_then(|v| v.split('/').next()) {
            if !abi.is_empty() {
                abis.insert(abi.to_string());
            }
        }
    }
    let size = std::fs::metadata(&args.path)?.len();
    Ok(json!({"status":"ok","path":args.path,"size_bytes":size,"entry_count":zip.len(),"manifest_present":manifest,"resources_present":resources,"dex_count":dex_count,"dex_files":dex,"dex_files_truncated":dex_count>dex.len(),"native_abis":abis}).to_string())
}

fn is_dex(name: &str) -> bool {
    name.starts_with("classes")
        && name.ends_with(".dex")
        && !name.contains('/')
        && name[7..name.len() - 4].chars().all(|c| c.is_ascii_digit())
}

fn entries(args: &ApkEntriesArgs) -> Result<String> {
    let mut zip = archive(&args.path)?;
    let max = limit(args.limit)?;
    if args.prefix.len() > 256 {
        bail!("entry prefix too long")
    }
    let mut result = Vec::new();
    let mut matches = 0usize;
    for index in 0..zip.len() {
        let entry = zip.by_index(index).context("cannot inspect APK entry")?;
        if entry.name().starts_with(&args.prefix) {
            matches += 1;
            if result.len() < max {
                result.push(json!({"name":entry.name().chars().take(512).collect::<String>(),"size":entry.size(),"compressed_size":entry.compressed_size()}));
            }
        }
    }
    Ok(json!({"status":"ok","path":args.path,"prefix":args.prefix,"matches":matches,"entries":result,"truncated":matches>result.len()}).to_string())
}

fn dex_classes(args: &DexClassesArgs) -> Result<String> {
    let max = limit(args.limit)?;
    if args.query.len() > 256 {
        bail!("class query too long")
    }
    let mut classes = Vec::new();
    let mut matches = 0usize;
    let scan = analysis::scan_dex(&args.path, |name, dex| {
        for class in dex.class_names()? {
            if class.contains(&args.query) {
                matches += 1;
                if classes.len() < max {
                    classes.push(json!({"class":class,"dex":name}));
                }
            }
        }
        Ok(())
    })?;
    Ok(json!({"status":"ok","path":args.path,"dex_file_count":scan.indexed,"matches":matches,"classes":classes,"truncated":matches>classes.len(),"unindexed_dex":scan.json()}).to_string())
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<usize> {
    let end = offset.checked_add(4).context("DEX offset overflow")?;
    let data: [u8; 4] = bytes
        .get(offset..end)
        .context("truncated DEX table")?
        .try_into()
        .context("invalid DEX word")?;
    Ok(u32::from_le_bytes(data) as usize)
}

fn table(
    bytes: &[u8],
    size_offset: usize,
    offset_offset: usize,
    width: usize,
) -> Result<(usize, usize)> {
    let size = u32_at(bytes, size_offset)?;
    let offset = u32_at(bytes, offset_offset)?;
    let end = size
        .checked_mul(width)
        .and_then(|n| offset.checked_add(n))
        .context("DEX table overflow")?;
    if end > bytes.len() {
        bail!("DEX table exceeds file length")
    }
    Ok((size, offset))
}

#[cfg(test)]
fn parse_classes(bytes: &[u8]) -> Result<Vec<String>> {
    dex::Dex::parse(bytes)?.class_names()
}

fn validate_class_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 512
        || name.split('.').any(|part| {
            part.is_empty()
                || !part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        })
    {
        bail!("class_name must be a dotted Java class name")
    }
    Ok(())
}

struct ReadOperation<A> {
    args: A,
    run: fn(&A) -> Result<String>,
}

#[async_trait]
impl<A: Send + 'static> PreparedExecution for ReadOperation<A> {
    async fn execute(self: Box<Self>, _: &mut ToolContext<'_>) -> Result<ToolOutput> {
        let content = tokio::task::spawn_blocking(move || (self.run)(&self.args))
            .await
            .context("APK reader worker failed")??;
        Ok(ToolOutput::success(content))
    }
}

async fn prepare_inspect(_: &ToolContext<'_>, args: ApkPathArgs) -> Result<PreparedToolCall> {
    validate_path(&args.path)?;
    Ok(PreparedToolCall::operation(
        String::new(),
        Box::new(ReadOperation { args, run: inspect }),
    ))
}
async fn prepare_entries(_: &ToolContext<'_>, args: ApkEntriesArgs) -> Result<PreparedToolCall> {
    validate_path(&args.path)?;
    limit(args.limit)?;
    Ok(PreparedToolCall::operation(
        String::new(),
        Box::new(ReadOperation { args, run: entries }),
    ))
}
async fn prepare_classes(_: &ToolContext<'_>, args: DexClassesArgs) -> Result<PreparedToolCall> {
    validate_path(&args.path)?;
    limit(args.limit)?;
    Ok(PreparedToolCall::operation(
        String::new(),
        Box::new(ReadOperation {
            args,
            run: dex_classes,
        }),
    ))
}

struct DecompileOperation(DecompileClassArgs);

#[async_trait]
impl PreparedExecution for DecompileOperation {
    async fn execute(self: Box<Self>, _: &mut ToolContext<'_>) -> Result<ToolOutput> {
        let content =
            crate::runtime_dependencies::jadx::decompile_class(&self.0.path, &self.0.class_name)
                .await?;
        Ok(ToolOutput::success(content))
    }
}

struct JadxCheckOperation;

#[async_trait]
impl PreparedExecution for JadxCheckOperation {
    async fn execute(self: Box<Self>, ctx: &mut ToolContext<'_>) -> Result<ToolOutput> {
        let executor = ctx.executor.context("shell executor unavailable")?;
        let content = crate::runtime_dependencies::jadx::check(executor).await?;
        Ok(ToolOutput::success(content))
    }
}

struct JadxInstallOperation;

#[async_trait]
impl PreparedExecution for JadxInstallOperation {
    async fn execute(self: Box<Self>, ctx: &mut ToolContext<'_>) -> Result<ToolOutput> {
        let config = ctx.config.context("tool config unavailable")?;
        let content = crate::runtime_dependencies::jadx::install(config).await?;
        Ok(ToolOutput::success(content))
    }
}

async fn prepare_jadx_check(_: &ToolContext<'_>, _: JadxHelperArgs) -> Result<PreparedToolCall> {
    if !cfg!(target_os = "android") {
        bail!("jadx_check reports helper state for the Android app_process runtime only")
    }
    Ok(PreparedToolCall::operation(
        "Report the DEX helper source, authentication, and installed state without downloading it"
            .to_string(),
        Box::new(JadxCheckOperation),
    ))
}

async fn prepare_jadx_install(_: &ToolContext<'_>, _: JadxHelperArgs) -> Result<PreparedToolCall> {
    if !cfg!(target_os = "android") {
        bail!("jadx_install installs the DEX helper for Android app_process only")
    }
    let source = crate::runtime_dependencies::jadx::install_preview()?;
    Ok(PreparedToolCall::operation(
        source,
        Box::new(JadxInstallOperation),
    ))
}

async fn prepare_decompile(
    _: &ToolContext<'_>,
    args: DecompileClassArgs,
) -> Result<PreparedToolCall> {
    if !cfg!(target_os = "android") {
        bail!("decompile_apk_class requires Android app_process")
    }
    validate_path(&args.path)?;
    validate_class_name(&args.class_name)?;
    let preview = format!(
        "Decompile {} from {} with the installed Android DEX helper. This runs the helper through app_process and does not download anything; if no helper is installed, use jadx_install after approval, then retry.",
        args.class_name, args.path
    );
    Ok(PreparedToolCall::operation(
        preview,
        Box::new(DecompileOperation(args)),
    ))
}

define_tool!(InspectApkTool, ApkPathArgs, INSPECT_META, prepare_inspect);
define_tool!(
    ListApkEntriesTool,
    ApkEntriesArgs,
    ENTRIES_META,
    prepare_entries
);
define_tool!(
    ListDexClassesTool,
    DexClassesArgs,
    CLASSES_META,
    prepare_classes
);
define_tool!(
    DecompileApkClassTool,
    DecompileClassArgs,
    DECOMPILE_META,
    prepare_decompile
);
define_tool!(
    JadxCheckTool,
    JadxHelperArgs,
    JADX_CHECK_META,
    prepare_jadx_check
);
define_tool!(
    JadxInstallTool,
    JadxHelperArgs,
    JADX_INSTALL_META,
    prepare_jadx_install
);

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sample_dex() -> Vec<u8> {
        let descriptor = b"Lcom/example/Main;\0";
        let mut bytes = vec![0u8; 116 + descriptor.len()];
        bytes[..8].copy_from_slice(b"dex\n035\0");
        bytes[56..60].copy_from_slice(&1u32.to_le_bytes());
        bytes[60..64].copy_from_slice(&112u32.to_le_bytes());
        bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
        bytes[68..72].copy_from_slice(&116u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&1u32.to_le_bytes());
        bytes[100..104].copy_from_slice(&120u32.to_le_bytes());
        bytes.resize(153 + descriptor.len(), 0);
        bytes[112..116].copy_from_slice(&152u32.to_le_bytes());
        bytes[152] = 18;
        bytes[153..153 + descriptor.len()].copy_from_slice(descriptor);
        let length = bytes.len() as u32;
        bytes[32..36].copy_from_slice(&length.to_le_bytes());
        bytes[36..40].copy_from_slice(&112u32.to_le_bytes());
        bytes[40..44].copy_from_slice(&0x12345678u32.to_le_bytes());
        bytes
    }

    #[test]
    fn reads_apk_and_dex_class_without_extraction() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("sample.apk");
        let file = File::create(&path)?;
        let mut writer = zip::ZipWriter::new(file);
        let options =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        writer.start_file("AndroidManifest.xml", options)?;
        writer.write_all(b"manifest")?;
        writer.start_file("classes.dex", options)?;
        writer.write_all(&sample_dex())?;
        writer.finish()?;
        let path = path.to_string_lossy().to_string();
        let summary: serde_json::Value =
            serde_json::from_str(&inspect(&ApkPathArgs { path: path.clone() })?)?;
        assert_eq!(summary["dex_count"], 1);
        assert_eq!(summary["manifest_present"], true);
        let names: serde_json::Value = serde_json::from_str(&dex_classes(&DexClassesArgs {
            path: path.clone(),
            query: "example".into(),
            limit: 10,
        })?)?;
        assert_eq!(names["classes"][0]["class"], "com.example.Main");
        let listed: serde_json::Value = serde_json::from_str(&entries(&ApkEntriesArgs {
            path,
            prefix: "classes".into(),
            limit: 10,
        })?)?;
        assert_eq!(listed["matches"], 1);
        Ok(())
    }

    #[test]
    fn dex_parser_rejects_bad_offsets() {
        let mut bytes = vec![0; 112];
        bytes[..8].copy_from_slice(b"dex\n035\0");
        bytes[96..100].copy_from_slice(&1u32.to_le_bytes());
        bytes[100..104].copy_from_slice(&200u32.to_le_bytes());
        assert!(parse_classes(&bytes).is_err());
    }

    #[test]
    fn exact_class_name_rejects_paths_and_flags() {
        assert!(validate_class_name("com.example.Main$Inner").is_ok());
        for invalid in ["../Main", "-help", "com..Main", "a/b", "a;rm"] {
            assert!(validate_class_name(invalid).is_err());
        }
        let assessment = DECOMPILE_META.assessment();
        assert!(assessment.is_some_and(|value| value.requires_double_confirmation));
    }

    #[test]
    fn helper_acquisition_tools_are_always_available_and_decompile_stays_dangerous() {
        use crate::tools::{RuntimeRequirement, ToolPlatform, ToolRisk as Risk};

        // Recovery must stay registered even where the helper cannot run, so the model can
        // diagnose and install instead of repeating static-index guesses.
        for (metadata, risk) in [
            (&JADX_CHECK_META, Risk::ReadOnly),
            (&JADX_INSTALL_META, Risk::Mutating),
        ] {
            assert_eq!(metadata.risk, risk, "{}", metadata.name);
            assert_eq!(
                metadata.runtime,
                RuntimeRequirement::None,
                "{}",
                metadata.name
            );
            assert_eq!(
                metadata.platform,
                ToolPlatform::Android,
                "{}",
                metadata.name
            );
            assert_eq!(metadata.group.map(|group| group.id()), Some("jadx"));
        }

        // Only the decompile call may execute helper code, and it requires strong confirmation.
        assert_eq!(DECOMPILE_META.risk, Risk::Dangerous);
        assert_eq!(DECOMPILE_META.runtime, RuntimeRequirement::Jadx);
        assert!(!JADX_CHECK_META
            .description
            .contains("jadx_install after approval"));

        // The dangerous call must disclose that it does not fetch anything and name recovery.
        assert!(DECOMPILE_META.description.contains("Does not download"));
        assert!(DECOMPILE_META
            .description
            .contains("jadx_install after approval"));
    }
}
