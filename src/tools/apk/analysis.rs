//! Cheap static analysis adapters; no device subprocess, download, extraction or JADX is used.

use super::dex::{Dex, Query};
use super::manifest::Manifest;
use super::{
    archive, default_limit, is_dex, limit, validate_class_name, validate_path, ReadOperation,
    MAX_DEX_BYTES, MAX_DEX_FILES,
};
use crate::tools::{PreparedToolCall, ToolContext, ToolMetadata};
use anyhow::{bail, Context, Result};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::Read;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IndexArgs {
    /// Existing local APK path. Archive contents are never executed.
    pub path: String,
    /// Literal substring of a method signature, DEX string or native-library path.
    #[serde(default)]
    pub query: String,
    /// Maximum returned records, from 1 to 200.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassReferencesArgs {
    /// Existing local APK path.
    pub path: String,
    /// Exact dotted class name to find in declarations and direct bytecode references.
    pub class_name: String,
    /// Maximum returned references, from 1 to 200.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MethodReferencesArgs {
    /// Existing local APK path.
    pub path: String,
    /// Exact dotted owner class.
    pub class_name: String,
    /// Exact method name, including <init> or <clinit> when applicable.
    pub method_name: String,
    /// Optional exact DEX prototype, such as (Ljava/lang/String;)V; empty matches overloads.
    #[serde(default)]
    pub prototype: String,
    /// Maximum returned direct invoke references, from 1 to 200.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManifestArgs {
    /// Existing local APK path containing Android binary AndroidManifest.xml.
    pub path: String,
    /// Maximum returned records, from 1 to 200.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn query(value: &str) -> Result<()> {
    if value.len() > 256 {
        bail!("literal query exceeds 256 bytes")
    }
    Ok(())
}

/// Bounded outcome of indexing every DEX entry in one archive.
///
/// A single unparsable DEX file must not discard the evidence from every other file, so a
/// per-file parse failure is recorded in [`DexScan::skipped`] instead of aborting the scan.
/// Archive-level resource limits still fail the whole call.
pub(super) struct DexScan {
    pub indexed: usize,
    pub skipped: Vec<SkippedDex>,
}

/// One DEX entry that could not be indexed, with a bounded reason for the model.
pub(super) struct SkippedDex {
    pub dex: String,
    pub reason: String,
}

impl DexScan {
    pub fn json(&self) -> Value {
        json!({
            "count": self.skipped.len(),
            "files": self.skipped
                .iter()
                .map(|skip| json!({"dex": skip.dex, "reason": skip.reason}))
                .collect::<Vec<Value>>(),
        })
    }
}

pub(super) fn scan_dex(
    path: &str,
    mut visit: impl FnMut(&str, &Dex<'_>) -> Result<()>,
) -> Result<DexScan> {
    let mut zip = archive(path)?;
    let mut seen = 0usize;
    let mut indexed = 0usize;
    let mut skipped = Vec::new();
    let mut total = 0u64;
    let mut compressed = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).context("cannot inspect DEX entry")?;
        if !is_dex(entry.name()) {
            continue;
        }
        seen += 1;
        compressed = compressed
            .checked_add(entry.compressed_size())
            .context("DEX compressed size overflow")?;
        if compressed > 65 * 1024 * 1024 || entry.compressed_size() > MAX_DEX_BYTES + 1024 * 1024 {
            bail!("DEX compressed data exceeds inspection limit")
        }
        if seen > MAX_DEX_FILES || entry.size() > MAX_DEX_BYTES {
            bail!("DEX count or entry size exceeds inspection limit")
        }
        total = total
            .checked_add(entry.size())
            .context("DEX total size overflow")?;
        if total > 64 * 1024 * 1024 {
            bail!("APK DEX data exceeds 64 MiB")
        }
        let name = entry.name().to_owned();
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .by_ref()
            .take(MAX_DEX_BYTES + 1)
            .read_to_end(&mut bytes)
            .context("cannot read DEX")?;
        if bytes.len() as u64 > MAX_DEX_BYTES || bytes.len() as u64 != entry.size() {
            bail!("DEX size mismatch")
        }
        let dex = match Dex::parse(&bytes) {
            Ok(dex) => dex,
            Err(error) => {
                skipped.push(SkippedDex {
                    dex: bounded_entry_name(&name),
                    reason: bounded_reason(&error),
                });
                continue;
            }
        };
        visit(&name, &dex)?;
        indexed += 1;
    }
    Ok(DexScan { indexed, skipped })
}

/// Bounds a reason reported to the model: single-line and length-capped, so hostile DEX metadata
/// cannot bloat the result or inject control characters.
fn bounded_reason(error: &anyhow::Error) -> String {
    let filtered = format!("{error:#}")
        .chars()
        .filter(|character| !character.is_control())
        .take(201)
        .collect::<String>();
    match filtered.chars().count() {
        201 => filtered[..].trim_end().to_owned() + "…",
        _ => filtered,
    }
}

/// Bounds a ZIP entry name reported for a skipped file. `is_dex` accepts arbitrarily long
/// all-digit names, so the length is capped before it reaches the result.
fn bounded_entry_name(name: &str) -> String {
    const MAX: usize = 128;
    if name.len() <= MAX {
        return name.to_owned();
    }
    let mut end = MAX;
    while end > 0 && !name.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &name[..end])
}

fn methods(args: &IndexArgs) -> Result<String> {
    let max = limit(args.limit)?;
    query(&args.query)?;
    let mut result = Vec::new();
    let mut matches = 0usize;
    let scan = scan_dex(&args.path, |name, dex| {
        for method in &dex.methods {
            if method.signature.contains(&args.query) || method.class.contains(&args.query) {
                matches += 1;
                if result.len() < max {
                    result.push(json!({"dex":name,"method":method}));
                }
            }
        }
        Ok(())
    })?;
    Ok(
        json!({"status":"ok","dex_file_count":scan.indexed,"matches":matches,"methods":result,
        "truncated":matches>result.len(),"scope":"method_ids, including external method references",
        "unindexed_dex":scan.json()})
        .to_string(),
    )
}

fn strings(args: &IndexArgs) -> Result<String> {
    let max = limit(args.limit)?;
    query(&args.query)?;
    if args.query.is_empty() {
        bail!("search_dex_strings requires a nonempty literal query")
    }
    let mut result = Vec::new();
    let mut matches = 0usize;
    let scan = scan_dex(&args.path, |name, dex| {
        for (index, value) in dex.strings.iter().enumerate() {
            if value.contains(&args.query) {
                matches += 1;
                if result.len() < max {
                    result.push(json!({"dex":name,"string_index":index,
                    "value":value.chars().take(1024).collect::<String>(),"value_truncated":value.chars().count()>1024}));
                }
            }
        }
        Ok(())
    })?;
    Ok(
        json!({"status":"ok","dex_file_count":scan.indexed,"matches":matches,"strings":result,
        "truncated":matches>result.len(),"unindexed_dex":scan.json()})
        .to_string(),
    )
}

fn references(path: &str, max: usize, query: Query<'_>) -> Result<String> {
    let mut result = Vec::new();
    let mut matches = 0usize;
    let scan = scan_dex(path, |name, dex| {
        let query = match &query {
            Query::Class(class) => Query::Class(class),
            Query::Method {
                class,
                name,
                prototype,
            } => Query::Method {
                class,
                name,
                prototype,
            },
        };
        dex.references(query, |reference| {
            matches += 1;
            if result.len() < max {
                result.push(json!({"dex":name,"reference":reference}));
            }
        })
    })?;
    Ok(json!({"status":"ok","dex_file_count":scan.indexed,"matches":matches,"references":result,
        "truncated":matches>result.len(),"scope":"declarations and direct instructions; reflection, dynamic call sites, method handles and native references are not resolved",
        "unindexed_dex":scan.json()}).to_string())
}
fn class_references(args: &ClassReferencesArgs) -> Result<String> {
    validate_class_name(&args.class_name)?;
    references(
        &args.path,
        limit(args.limit)?,
        Query::Class(&args.class_name),
    )
}
fn method_references(args: &MethodReferencesArgs) -> Result<String> {
    validate_class_name(&args.class_name)?;
    if args.method_name.is_empty()
        || args.method_name.len() > 256
        || args
            .method_name
            .chars()
            .any(|character| character.is_control())
        || args.prototype.len() > 1024
    {
        bail!("invalid method name or oversized prototype")
    }
    references(
        &args.path,
        limit(args.limit)?,
        Query::Method {
            class: &args.class_name,
            name: &args.method_name,
            prototype: &args.prototype,
        },
    )
}

fn manifest(path: &str) -> Result<Manifest> {
    let mut zip = archive(path)?;
    let mut entry = zip
        .by_name("AndroidManifest.xml")
        .context("APK has no AndroidManifest.xml")?;
    const MAX: u64 = 4 * 1024 * 1024;
    if entry.size() > MAX || entry.compressed_size() > MAX + 1024 * 1024 {
        bail!("manifest exceeds 4 MiB inspection limit")
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .by_ref()
        .take(MAX + 1)
        .read_to_end(&mut bytes)
        .context("cannot read manifest")?;
    if bytes.len() as u64 > MAX || bytes.len() as u64 != entry.size() {
        bail!("manifest size mismatch")
    }
    Manifest::parse(&bytes)
}
fn inspect_manifest(args: &ManifestArgs) -> Result<String> {
    let max = limit(args.limit)?;
    let manifest = manifest(&args.path)?;
    Ok(json!({"status":"ok","attributes":manifest.attributes,"sdk":manifest.sdk,"application":manifest.application,
        "permission_count":manifest.permissions.len(),"declared_permission_count":manifest.declared_permissions.len(),
        "component_count":manifest.components.len(),"features":manifest.features.iter().take(max).collect::<Vec<_>>(),
        "features_truncated":manifest.features.len()>max,"scope":"binary manifest only; referenced resources are represented by IDs"}).to_string())
}
fn permissions(args: &ManifestArgs) -> Result<String> {
    let max = limit(args.limit)?;
    let manifest = manifest(&args.path)?;
    Ok(json!({"status":"ok","requested_count":manifest.permissions.len(),"requested":manifest.permissions.iter().take(max).collect::<Vec<_>>(),
        "declared_count":manifest.declared_permissions.len(),"declared":manifest.declared_permissions.iter().take(max).collect::<Vec<_>>(),
        "truncated":manifest.permissions.len()>max || manifest.declared_permissions.len()>max,
        "scope":"manifest declarations; does not establish installed grants or AppOps"}).to_string())
}
fn exported(args: &ManifestArgs) -> Result<String> {
    let max = limit(args.limit)?;
    let manifest = manifest(&args.path)?;
    let candidates: Vec<_> = manifest
        .components
        .iter()
        .filter(|component| component.exported != Some(false))
        .collect();
    Ok(json!({"status":"ok","application":manifest.application,"matches":candidates.len(),
        "components":candidates.iter().take(max).collect::<Vec<_>>(),"truncated":candidates.len()>max,
        "scope":"declared exposure, including unknown values needing review; permissions, enabled state and runtime restrictions may prevent access"}).to_string())
}
fn native_libs(args: &IndexArgs) -> Result<String> {
    query(&args.query)?;
    let max = limit(args.limit)?;
    let mut zip = archive(&args.path)?;
    let mut result = Vec::new();
    let mut matches = 0usize;
    let mut compressed_budget = 32 * 1024 * 1024u64;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .context("cannot inspect native library entry")?;
        let Some(rest) = entry.name().strip_prefix("lib/") else {
            continue;
        };
        let Some((abi, basename)) = rest.split_once('/') else {
            continue;
        };
        if abi.is_empty()
            || basename.contains('/')
            || !basename.ends_with(".so")
            || !entry.name().contains(&args.query)
        {
            continue;
        }
        matches += 1;
        if result.len() < max {
            let name = entry.name().chars().take(512).collect::<String>();
            let abi = abi.to_owned();
            let size = entry.size();
            let scan = entry.compressed_size() <= compressed_budget;
            if scan {
                compressed_budget -= entry.compressed_size();
            }
            let mut header = [0u8; 20];
            let mut read = 0;
            while scan && read < header.len() {
                let count = entry
                    .read(&mut header[read..])
                    .context("cannot read ELF prefix")?;
                if count == 0 {
                    break;
                }
                read += count;
            }
            let elf = read == 20 && header.starts_with(b"\x7fELF");
            let machine = if elf && header[5] == 1 {
                Some(u16::from_le_bytes([header[18], header[19]]))
            } else if elf && header[5] == 2 {
                Some(u16::from_be_bytes([header[18], header[19]]))
            } else {
                None
            };
            result.push(json!({"path":name,"abi":abi,"size_bytes":size,"elf":if scan { Some(elf) } else { None },"prefix_status":if scan { "scanned" } else { "compressed_budget_exceeded" },
                "elf_class":if elf { Some(header[4]) } else { None },"elf_machine":machine}));
        }
    }
    Ok(json!({"status":"ok","matches":matches,"libraries":result,"truncated":matches>result.len(),
        "scope":"lib/<abi>/*.so metadata and ELF prefix; no extraction, symbol analysis or loading"}).to_string())
}

macro_rules! analyzer {
    ($tool:ident, $args:ty, $name:literal, $description:literal, $run:ident) => {
        pub(crate) struct $tool;
        #[async_trait::async_trait]
        impl crate::tools::Tool for $tool {
            fn metadata(&self) -> &'static ToolMetadata {
                static META: ToolMetadata = ToolMetadata {
                    name: $name,
                    description: $description,
                    category: crate::tools::ToolCategory::File,
                    risk: crate::tools::ToolRisk::ReadOnly,
                    requires: &[],
                    group: Some(crate::tools::ToolGroup::Jadx),
                    default_enabled: false,
                    platform: crate::tools::ToolPlatform::Any,
                    runtime: crate::tools::RuntimeRequirement::None,
                    concurrency: crate::tools::ToolConcurrency::Parallel,
                    lifetime: crate::tools::ToolLifetime::Call,
                    schema: crate::tools::descriptor_schema::<$args>,
                };
                &META
            }
            async fn prepare(
                &self,
                _: &ToolContext<'_>,
                arguments: Value,
            ) -> Result<PreparedToolCall> {
                let args: $args = serde_json::from_value(arguments)
                    .context("invalid static APK analysis arguments")?;
                validate_path(&args.path)?;
                limit(args.limit)?;
                Ok(PreparedToolCall::operation(
                    String::new(),
                    Box::new(ReadOperation { args, run: $run }),
                ))
            }
        }
    };
}
analyzer!(ListDexMethodsTool, IndexArgs, "list_dex_methods", "Index bounded DEX method IDs and prototypes, including external references; filter by a literal signature substring. No JADX or execution.", methods);
analyzer!(SearchDexStringsTool, IndexArgs, "search_dex_strings", "Search bounded DEX strings by a nonempty literal substring, decoding modified UTF-8 without execution.", strings);
analyzer!(FindClassReferencesTool, ClassReferencesArgs, "find_class_references", "Find bounded class references in DEX declarations and direct instructions; does not resolve reflection or dynamic/native calls.", class_references);
analyzer!(FindMethodReferencesTool, MethodReferencesArgs, "find_method_references", "Find direct DEX invoke instructions for an exact owner/method, optionally narrowed by prototype; no decompilation.", method_references);
analyzer!(InspectManifestTool, ManifestArgs, "inspect_manifest", "Inspect bounded Android binary manifest package/version/SDK/application metadata; resource values remain explicit IDs.", inspect_manifest);
analyzer!(
    ListPermissionsTool,
    ManifestArgs,
    "list_permissions",
    "List requested and declared manifest permissions; does not infer installed grants or AppOps.",
    permissions
);
analyzer!(ListExportedComponentsTool, ManifestArgs, "list_exported_components", "List explicitly/default exported components and unresolved exposure needing review; include target-SDK defaults and intent filters.", exported);
analyzer!(FindNativeLibsTool, IndexArgs, "find_native_libs", "Find bounded lib/<abi>/*.so entries and inspect ELF prefixes without extracting or loading code.", native_libs);

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::File, io::Write};
    use zip::{write::FileOptions, ZipWriter};

    fn fixture(path: &std::path::Path) -> Result<()> {
        let mut archive = ZipWriter::new(File::create(path)?);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for name in ["classes.dex", "classes2.dex"] {
            archive.start_file(name, options)?;
            archive.write_all(include_bytes!("../../../tests/fixtures/apk/classes.dex"))?;
        }
        archive.start_file("AndroidManifest.xml", options)?;
        archive.write_all(include_bytes!(
            "../../../tests/fixtures/apk/AndroidManifest.axml"
        ))?;
        archive.start_file("lib/x86_64/libfixture.so", options)?;
        let mut elf = [0u8; 20];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2;
        elf[5] = 1;
        elf[18..20].copy_from_slice(&62u16.to_le_bytes());
        archive.write_all(&elf)?;
        archive.start_file("lib/arm64-v8a/not-elf.so", options)?;
        archive.write_all(b"text")?;
        archive.finish()?;
        Ok(())
    }

    #[test]
    fn all_eight_analyzers_read_multidex_manifest_and_native_metadata_without_execution(
    ) -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("fixture.apk");
        fixture(&path)?;
        let path = path.to_string_lossy().into_owned();
        let index = IndexArgs {
            path: path.clone(),
            query: "example.Target".into(),
            limit: 1,
        };
        let result: Value = serde_json::from_str(&methods(&index)?)?;
        assert_eq!(result["dex_file_count"], 2);
        assert_eq!(result["matches"], 6);
        assert_eq!(result["methods"].as_array().expect("methods").len(), 1);
        assert_eq!(result["truncated"], true);
        let index = IndexArgs {
            query: "needle".into(),
            ..index
        };
        let result: Value = serde_json::from_str(&strings(&index)?)?;
        assert_eq!(result["matches"], 2);
        assert_eq!(result["strings"][0]["value"], "needle\0🐈");
        let result: Value = serde_json::from_str(&class_references(&ClassReferencesArgs {
            path: path.clone(),
            class_name: "example.Target".into(),
            limit: 1,
        })?)?;
        assert!(result["matches"].as_u64().expect("matches") > 2);
        assert_eq!(result["truncated"], true);
        let result: Value = serde_json::from_str(&method_references(&MethodReferencesArgs {
            path: path.clone(),
            class_name: "example.Target".into(),
            method_name: "touch".into(),
            prototype: "(I)V".into(),
            limit: 50,
        })?)?;
        assert_eq!(result["matches"], 2);
        let manifest = ManifestArgs {
            path: path.clone(),
            limit: 50,
        };
        let result: Value = serde_json::from_str(&inspect_manifest(&manifest)?)?;
        assert_eq!(result["attributes"]["package"], "example.fixture");
        assert_eq!(result["component_count"], 4);
        let result: Value = serde_json::from_str(&permissions(&manifest)?)?;
        assert_eq!(result["requested_count"], 2);
        assert_eq!(result["declared_count"], 1);
        let result: Value = serde_json::from_str(&exported(&manifest)?)?;
        assert_eq!(result["matches"], 2);
        assert_eq!(result["components"][1]["exported"], Value::Null);
        let result: Value = serde_json::from_str(&native_libs(&IndexArgs {
            path,
            query: String::new(),
            limit: 50,
        })?)?;
        assert_eq!(result["matches"], 2);
        assert_eq!(result["libraries"][0]["elf_machine"], 62);
        assert_eq!(result["libraries"][1]["elf"], false);
        Ok(())
    }

    #[test]
    fn keeps_indexing_healthy_dex_files_when_one_file_is_unparsable() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("mixed.apk");
        let mut archive = ZipWriter::new(File::create(&path)?);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        archive.start_file("classes.dex", options)?;
        archive.write_all(include_bytes!("../../../tests/fixtures/apk/classes.dex"))?;
        // A single corrupt entry must not discard the evidence from every other file.
        archive.start_file("classes2.dex", options)?;
        archive.write_all(b"not a dex file")?;
        archive.start_file("classes3.dex", options)?;
        let mut oversized = include_bytes!("../../../tests/fixtures/apk/classes.dex").to_vec();
        oversized[56..60].copy_from_slice(&u32::MAX.to_le_bytes());
        archive.write_all(&oversized)?;
        archive.finish()?;
        let path = path.to_string_lossy().into_owned();

        let result: Value = serde_json::from_str(&strings(&IndexArgs {
            path: path.clone(),
            query: "needle".into(),
            limit: 50,
        })?)?;
        assert_eq!(result["status"], "ok");
        assert_eq!(result["dex_file_count"], 1);
        assert_eq!(result["matches"], 1);
        assert_eq!(result["unindexed_dex"]["count"], 2);
        let skipped = result["unindexed_dex"]["files"]
            .as_array()
            .expect("skipped files");
        let names: Vec<&str> = skipped
            .iter()
            .map(|file| file["dex"].as_str().expect("dex name"))
            .collect();
        assert_eq!(names, ["classes2.dex", "classes3.dex"]);
        for file in skipped {
            let reason = file["reason"].as_str().expect("reason");
            assert!(!reason.is_empty(), "skipped file needs a reason");
            assert!(!reason.contains('\n'), "reason must stay single-line");
        }

        let result: Value = serde_json::from_str(&methods(&IndexArgs {
            path,
            query: "example.Target".into(),
            limit: 50,
        })?)?;
        assert_eq!(result["dex_file_count"], 1);
        assert_eq!(result["matches"], 3);
        assert_eq!(result["unindexed_dex"]["count"], 2);
        Ok(())
    }

    #[test]
    fn indexes_apk_with_more_dex_files_than_the_old_thirty_two_file_ceiling() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("many.apk");
        let mut archive = ZipWriter::new(File::create(&path)?);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        // Matches a shipping APK observed in the field, well past the former 32-file ceiling.
        let count = 35;
        for index in 0..count {
            archive.start_file(format!("classes{index}.dex"), options)?;
            archive.write_all(include_bytes!("../../../tests/fixtures/apk/classes.dex"))?;
        }
        archive.start_file(format!("classes{count}.dex"), options)?;
        archive.write_all(b"not a dex file")?;
        archive.finish()?;
        let result: Value = serde_json::from_str(&strings(&IndexArgs {
            path: path.to_string_lossy().into_owned(),
            query: "needle".into(),
            limit: 1,
        })?)?;
        assert_eq!(result["dex_file_count"], count);
        assert_eq!(result["matches"], count);
        assert_eq!(result["unindexed_dex"]["count"], 1);

        // The ceiling itself must still reject an archive with too many DEX files.
        let path = directory.path().join("excess.apk");
        let mut archive = ZipWriter::new(File::create(&path)?);
        for index in 0..=MAX_DEX_FILES {
            archive.start_file(format!("classes{index}.dex"), options)?;
            archive.write_all(include_bytes!("../../../tests/fixtures/apk/classes.dex"))?;
        }
        archive.finish()?;
        assert!(strings(&IndexArgs {
            path: path.to_string_lossy().into_owned(),
            query: "needle".into(),
            limit: 1,
        })
        .is_err());
        Ok(())
    }

    #[test]
    fn bounds_skip_reasons_from_hostile_dex_metadata() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("hostile.apk");
        let mut archive = ZipWriter::new(File::create(&path)?);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        archive.start_file("classes.dex", options)?;
        let mut dex = include_bytes!("../../../tests/fixtures/apk/classes.dex").to_vec();
        // Point the string table at an offset whose declared length and payload are attacker text.
        dex[56..60].copy_from_slice(&1u32.to_le_bytes());
        let data_offset =
            u32::from_le_bytes(dex[60..64].try_into().expect("string table offset")) as usize;
        dex[data_offset..data_offset + 5].copy_from_slice(&[0x20, b'a', b'\n', b'b', 0]);
        archive.write_all(&dex)?;
        archive.finish()?;
        let result: Value = serde_json::from_str(&strings(&IndexArgs {
            path: path.to_string_lossy().into_owned(),
            query: "needle".into(),
            limit: 50,
        })?)?;
        assert_eq!(result["unindexed_dex"]["count"], 1);
        let reason = result["unindexed_dex"]["files"][0]["reason"]
            .as_str()
            .expect("reason");
        assert!(!reason.contains('\n'));
        assert!(
            reason.chars().count() <= 201,
            "reason {reason:?} is unbounded"
        );
        Ok(())
    }

    #[test]
    fn rejects_oversized_directory_before_loading_inventory_and_invalid_limits() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("fixture.apk");
        fixture(&path)?;
        let mut bytes = std::fs::read(&path)?;
        let end = bytes.len() - 22;
        bytes[end + 8..end + 10].copy_from_slice(&60_000u16.to_le_bytes());
        bytes[end + 10..end + 12].copy_from_slice(&60_000u16.to_le_bytes());
        std::fs::write(&path, &bytes)?;
        assert!(archive(&path.to_string_lossy()).is_err());
        assert!(methods(&IndexArgs {
            path: path.to_string_lossy().into(),
            query: String::new(),
            limit: 0
        })
        .is_err());
        assert!(strings(&IndexArgs {
            path: path.to_string_lossy().into(),
            query: String::new(),
            limit: 50
        })
        .is_err());
        Ok(())
    }
}
