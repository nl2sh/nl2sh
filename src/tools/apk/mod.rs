//! Bounded, read-only APK inspection and optional JADX class decompilation.

use super::{
    PreparedExecution, PreparedToolCall, ToolCategory, ToolContext, ToolMetadata, ToolOutput,
    ToolRisk,
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::{fs::File, io::Read};
use zip::ZipArchive;

const MAX_ENTRIES: usize = 50_000;
const MAX_APK_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_DEX_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CLASSES: usize = 2_000;

const INSPECT_META: ToolMetadata = ToolMetadata {
    name: "inspect_apk",
    description: "Inspect a local APK archive: size, file counts, DEX files, manifest presence, and native ABIs. Does not execute APK contents.",
    category: ToolCategory::File,
    risk: ToolRisk::ReadOnly,
    requires: &[],
    parallel_safe: true,
};
const ENTRIES_META: ToolMetadata = ToolMetadata {
    name: "list_apk_entries",
    description:
        "List bounded APK ZIP entries by optional literal path prefix, without extracting files.",
    category: ToolCategory::File,
    risk: ToolRisk::ReadOnly,
    requires: &[],
    parallel_safe: true,
};
const CLASSES_META: ToolMetadata = ToolMetadata {
    name: "list_dex_classes",
    description: "List class names from bounded DEX tables inside a local APK; optional literal class-name filter.",
    category: ToolCategory::File,
    risk: ToolRisk::ReadOnly,
    requires: &[],
    parallel_safe: true,
};
const DECOMPILE_META: ToolMetadata = ToolMetadata {
    name: "decompile_apk_class",
    description: "Decompile one exact APK class using a verified Android DEX helper through app_process. Strong confirmation required.",
    category: ToolCategory::File,
    risk: ToolRisk::Dangerous,
    requires: &[],
    parallel_safe: false,
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
    let file = File::open(path).with_context(|| format!("cannot open APK {path}"))?;
    let zip = ZipArchive::new(file).context("APK is not a valid ZIP archive")?;
    if zip.len() > MAX_ENTRIES {
        bail!("APK contains too many entries")
    }
    Ok(zip)
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
    let mut zip = archive(&args.path)?;
    let max = limit(args.limit)?;
    if args.query.len() > 256 {
        bail!("class query too long")
    }
    let mut classes = Vec::new();
    let mut matches = 0usize;
    let mut dex_files = 0usize;
    let mut total_dex_bytes = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).context("cannot inspect APK entry")?;
        if !is_dex(entry.name()) {
            continue;
        }
        dex_files += 1;
        if dex_files > 32 {
            bail!("APK contains too many DEX files")
        }
        if entry.size() > MAX_DEX_BYTES {
            bail!("DEX file exceeds 32 MiB inspection limit")
        }
        total_dex_bytes = total_dex_bytes
            .checked_add(entry.size())
            .context("DEX total size overflow")?;
        if total_dex_bytes > 64 * 1024 * 1024 {
            bail!("APK DEX data exceeds 64 MiB inspection limit")
        }
        let dex_name = entry.name().to_string();
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .by_ref()
            .take(MAX_DEX_BYTES + 1)
            .read_to_end(&mut bytes)
            .context("cannot read DEX entry")?;
        if bytes.len() as u64 > MAX_DEX_BYTES {
            bail!("DEX file exceeds 32 MiB inspection limit")
        }
        for name in parse_classes(&bytes)? {
            if name.contains(&args.query) {
                matches += 1;
                if classes.len() < max {
                    classes.push(json!({"class":name,"dex":dex_name}));
                }
            }
        }
    }
    Ok(json!({"status":"ok","path":args.path,"dex_file_count":dex_files,"matches":matches,"classes":classes,"truncated":matches>classes.len()}).to_string())
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

fn dex_string(bytes: &[u8], offset: usize) -> Result<String> {
    let mut pos = offset;
    for _ in 0..5 {
        let byte = *bytes.get(pos).context("truncated DEX string length")?;
        pos += 1;
        if byte & 0x80 == 0 {
            break;
        }
    }
    let rest = bytes.get(pos..).context("invalid DEX string offset")?;
    let end = rest
        .iter()
        .take(1024)
        .position(|b| *b == 0)
        .context("DEX string too long or unterminated")?;
    Ok(std::str::from_utf8(&rest[..end])
        .context("non UTF-8 DEX class name")?
        .to_string())
}

fn parse_classes(bytes: &[u8]) -> Result<Vec<String>> {
    if bytes.len() < 112 || !bytes.starts_with(b"dex\n") || bytes.get(7) != Some(&0) {
        bail!("invalid DEX header")
    }
    let (strings, string_off) = table(bytes, 56, 60, 4)?;
    let (types, type_off) = table(bytes, 64, 68, 4)?;
    let (count, class_off) = table(bytes, 96, 100, 32)?;
    if count > MAX_CLASSES * 20 {
        bail!("DEX class table exceeds inspection limit")
    }
    let mut result = Vec::with_capacity(count.min(MAX_CLASSES));
    for index in 0..count {
        let type_id = u32_at(bytes, class_off + index * 32)?;
        if type_id >= types {
            bail!("invalid DEX class type index")
        }
        let string_id = u32_at(bytes, type_off + type_id * 4)?;
        if string_id >= strings {
            bail!("invalid DEX class string index")
        }
        let offset = u32_at(bytes, string_off + string_id * 4)?;
        let descriptor = dex_string(bytes, offset)?;
        if let Some(name) = descriptor
            .strip_prefix('L')
            .and_then(|s| s.strip_suffix(';'))
        {
            if !name.is_empty() && name.len() <= 512 {
                result.push(name.replace('/', "."));
            }
        }
    }
    Ok(result)
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
    async fn execute(self: Box<Self>, ctx: &mut ToolContext<'_>) -> Result<ToolOutput> {
        let config = ctx.config.context("tool config unavailable")?;
        let content = crate::runtime_dependencies::jadx::decompile_class(
            config,
            &self.0.path,
            &self.0.class_name,
        )
        .await?;
        Ok(ToolOutput::success(content))
    }
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
    let preview = format!("Decompile {} from {} using an Android DEX helper. This may download and cache a SHA-256-verified helper and run it through app_process.", args.class_name, args.path);
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
}
