//! Android binary XML inspection without loading resources or executing package code.
//! Layout: AOSP libs/androidfw/include/androidfw/ResourceTypes.h.

use super::dex::u16_at;
use super::u32_at;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const ANDROID: &str = "http://schemas.android.com/apk/res/android";
const NO_INDEX: usize = u32::MAX as usize;

#[derive(Debug, Serialize)]
pub(super) struct Component {
    pub kind: String,
    pub name: Option<String>,
    pub attributes: BTreeMap<String, Value>,
    pub exported: Option<bool>,
    pub exported_basis: &'static str,
    pub intent_filters: usize,
    pub actions: Vec<String>,
    pub categories: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct PermissionDeclaration {
    pub element: String,
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
pub(super) struct Manifest {
    pub attributes: BTreeMap<String, Value>,
    pub sdk: BTreeMap<String, Value>,
    pub application: BTreeMap<String, Value>,
    pub permissions: Vec<PermissionDeclaration>,
    pub declared_permissions: Vec<PermissionDeclaration>,
    pub features: Vec<BTreeMap<String, Value>>,
    pub components: Vec<Component>,
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 8 || u16_at(bytes, 0)? != 3 || u32_at(bytes, 4)? != bytes.len() {
            bail!("manifest must be complete Android binary XML")
        }
        let mut position = u16_at(bytes, 2)?;
        if position < 8 || position > bytes.len() {
            bail!("invalid binary XML header")
        }
        let mut pool = None;
        let mut stack: Vec<String> = Vec::new();
        let mut result = Self {
            attributes: BTreeMap::new(),
            sdk: BTreeMap::new(),
            application: BTreeMap::new(),
            permissions: Vec::new(),
            declared_permissions: Vec::new(),
            features: Vec::new(),
            components: Vec::new(),
        };
        let mut component_index = None;
        let mut nodes = 0usize;
        let mut attribute_bytes = 0usize;
        let mut attributes_total = 0usize;
        let mut applications = 0usize;
        let mut sdk_nodes = 0usize;
        let mut roots = 0;
        while position < bytes.len() {
            let kind = u16_at(bytes, position)?;
            let header = u16_at(bytes, position + 2)?;
            let size = u32_at(bytes, position + 4)?;
            if header < 8 || size < header {
                bail!("invalid binary XML chunk size")
            }
            let end = position
                .checked_add(size)
                .context("binary XML chunk overflow")?;
            let chunk = bytes
                .get(position..end)
                .context("truncated binary XML chunk")?;
            match kind {
                1 => {
                    if pool.is_some() || !stack.is_empty() {
                        bail!("duplicate or misplaced binary XML string pool")
                    }
                    pool = Some(string_pool(chunk, header)?);
                }
                0x102 => {
                    nodes += 1;
                    if nodes > 8192 || stack.len() >= 64 {
                        bail!("manifest tree exceeds inspection limit")
                    }
                    let strings = pool.as_ref().context("binary XML has no string pool")?;
                    if header < 16 {
                        bail!("invalid binary XML node header")
                    }
                    if u32_at(chunk, header)? != NO_INDEX {
                        bail!("namespaced manifest elements are unsupported")
                    }
                    let name = pool_string(strings, u32_at(chunk, header + 4)?)?.to_owned();
                    let attribute_start = u16_at(chunk, header + 8)?;
                    let attribute_size = u16_at(chunk, header + 10)?;
                    let attribute_count = u16_at(chunk, header + 12)?;
                    if attribute_start < 20
                        || !(20..=64).contains(&attribute_size)
                        || attribute_count > 512
                    {
                        bail!("invalid binary XML attribute layout")
                    }
                    let attributes_start = header
                        .checked_add(attribute_start)
                        .context("binary XML attribute offset overflow")?;
                    let attributes_end = attribute_count
                        .checked_mul(attribute_size)
                        .and_then(|size| attributes_start.checked_add(size))
                        .context("binary XML attribute size overflow")?;
                    if attributes_end > chunk.len() {
                        bail!("truncated binary XML attributes")
                    }
                    let mut attributes = BTreeMap::new();
                    for index in 0..attribute_count {
                        let base = attributes_start + index * attribute_size;
                        let namespace = u32_at(chunk, base)?;
                        let local = pool_string(strings, u32_at(chunk, base + 4)?)?;
                        let prefix = if namespace == NO_INDEX {
                            ""
                        } else if pool_string(strings, namespace)? == ANDROID {
                            "android:"
                        } else {
                            "other:"
                        };
                        let key = format!("{prefix}{local}");
                        let raw = u32_at(chunk, base + 8)?;
                        if u16_at(chunk, base + 12)? != 8 {
                            bail!("invalid binary XML typed value size")
                        }
                        let data_type =
                            *chunk.get(base + 15).context("truncated binary XML value")?;
                        let data = u32_at(chunk, base + 16)?;
                        let value = match data_type {
                            3 => Value::String(pool_string(strings, data)?.into()),
                            0x12 => Value::Bool(data != 0),
                            0x10 => Value::from(data as u32 as i32),
                            0x11 => json!({"hex":format!("0x{data:08x}")}),
                            1 => json!({"resource_id":format!("0x{data:08x}")}),
                            2 => json!({"attribute_id":format!("0x{data:08x}")}),
                            _ if raw != NO_INDEX => {
                                Value::String(pool_string(strings, raw)?.into())
                            }
                            _ => json!({"type":data_type,"data":format!("0x{data:08x}")}),
                        };
                        attributes_total += 1;
                        attribute_bytes += key.len() + serde_json::to_string(&value)?.len();
                        if attributes_total > 16384 || attribute_bytes > 8 * 1024 * 1024 {
                            bail!("manifest attributes exceed inspection limit")
                        }
                        if attributes.insert(key, value).is_some() {
                            bail!("duplicate binary XML attribute")
                        }
                    }
                    let parent = stack.last().map(String::as_str);
                    match (name.as_str(), parent) {
                        ("manifest", None) => {
                            roots += 1;
                            if roots != 1 {
                                bail!("multiple manifest roots")
                            }
                            result.attributes = attributes;
                        }
                        ("uses-sdk", Some("manifest")) => {
                            sdk_nodes += 1;
                            if sdk_nodes > 1 {
                                bail!("duplicate uses-sdk element")
                            }
                            result.sdk = attributes;
                        }
                        (
                            "uses-permission" | "uses-permission-sdk-23" | "uses-permission-sdk-m",
                            Some("manifest"),
                        ) => result.permissions.push(PermissionDeclaration {
                            element: name.clone(),
                            attributes,
                        }),
                        (
                            "permission" | "permission-group" | "permission-tree",
                            Some("manifest"),
                        ) => result.declared_permissions.push(PermissionDeclaration {
                            element: name.clone(),
                            attributes,
                        }),
                        ("uses-feature", Some("manifest")) => result.features.push(attributes),
                        ("application", Some("manifest")) => {
                            applications += 1;
                            if applications > 1 {
                                bail!("duplicate application element")
                            }
                            result.application = attributes;
                        }
                        (
                            "activity" | "activity-alias" | "service" | "receiver" | "provider",
                            Some("application"),
                        ) => {
                            component_index = Some(result.components.len());
                            let package = result.attributes.get("package").and_then(Value::as_str);
                            let class = attributes
                                .get("android:name")
                                .and_then(Value::as_str)
                                .map(|name| qualify(package, name));
                            result.components.push(Component {
                                kind: name.clone(),
                                name: class,
                                attributes,
                                exported: None,
                                exported_basis: "unknown",
                                intent_filters: 0,
                                actions: Vec::new(),
                                categories: Vec::new(),
                            });
                        }
                        ("intent-filter", _) => {
                            if let Some(index) = component_index {
                                let component = &mut result.components[index];
                                component.intent_filters += 1;
                            }
                        }
                        ("action" | "category", Some("intent-filter")) => {
                            if let Some(index) = component_index {
                                if let Some(value) =
                                    attributes.get("android:name").and_then(Value::as_str)
                                {
                                    let values = if name == "action" {
                                        &mut result.components[index].actions
                                    } else {
                                        &mut result.components[index].categories
                                    };
                                    if values.len() < 128 {
                                        values.push(value.to_owned());
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    stack.push(name);
                }
                0x103 => {
                    let strings = pool.as_ref().context("binary XML has no string pool")?;
                    if header < 16 {
                        bail!("invalid binary XML end header")
                    }
                    let name = pool_string(strings, u32_at(chunk, header + 4)?)?;
                    if stack.pop().as_deref() != Some(name) {
                        bail!("mismatched binary XML element")
                    }
                    if matches!(
                        name,
                        "activity" | "activity-alias" | "service" | "receiver" | "provider"
                    ) {
                        component_index = None;
                    }
                }
                0x100 | 0x101 | 0x104 | 0x180 => {}
                _ => bail!("unsupported binary XML chunk type"),
            }
            position = end;
        }
        if roots != 1 || !stack.is_empty() {
            bail!("incomplete manifest document")
        }
        let target = result
            .sdk
            .get("android:targetSdkVersion")
            .or_else(|| result.sdk.get("android:minSdkVersion"))
            .map(integer)
            .unwrap_or(Some(1));
        for component in &mut result.components {
            let (exported, basis) = match component.attributes.get("android:exported") {
                Some(Value::Bool(value)) => (Some(*value), "explicit"),
                Some(_) => (None, "unresolved_exported_value"),
                None if component.kind == "provider" => (
                    target.map(|target| target <= 16),
                    "provider_target_sdk_default",
                ),
                None if component.intent_filters == 0 => (Some(false), "no_intent_filter_default"),
                None => match target {
                    Some(target) if target >= 31 => (None, "missing_required_exported"),
                    Some(_) => (Some(true), "legacy_intent_filter_default"),
                    None => (None, "unresolved_target_sdk"),
                },
            };
            component.exported = exported;
            component.exported_basis = basis;
        }
        Ok(result)
    }
}

fn integer(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}
fn qualify(package: Option<&str>, name: &str) -> String {
    match package {
        Some(package) if name.starts_with('.') => format!("{package}{name}"),
        Some(package) if !name.contains('.') => format!("{package}.{name}"),
        _ => name.into(),
    }
}
fn pool_string(strings: &[String], index: usize) -> Result<&str> {
    strings
        .get(index)
        .map(String::as_str)
        .context("invalid binary XML string index")
}

fn length8(bytes: &[u8], position: &mut usize) -> Result<usize> {
    let a = *bytes
        .get(*position)
        .context("truncated binary XML string length")?;
    *position += 1;
    if a & 128 == 0 {
        return Ok(a as usize);
    }
    let b = *bytes
        .get(*position)
        .context("truncated binary XML string length")?;
    *position += 1;
    Ok(((a as usize & 127) << 8) | b as usize)
}
fn length16(bytes: &[u8], position: &mut usize) -> Result<usize> {
    let a = u16_at(bytes, *position)?;
    *position += 2;
    if a & 0x8000 == 0 {
        return Ok(a);
    }
    let b = u16_at(bytes, *position)?;
    *position += 2;
    Ok(((a & 0x7fff) << 16) | b)
}

fn string_pool(chunk: &[u8], header: usize) -> Result<Vec<String>> {
    if header < 28 {
        bail!("invalid binary XML string pool header")
    }
    let count = u32_at(chunk, 8)?;
    let styles = u32_at(chunk, 12)?;
    let flags = u32_at(chunk, 16)?;
    let start = u32_at(chunk, 20)?;
    let styles_start = u32_at(chunk, 24)?;
    let offset_end = count
        .checked_add(styles)
        .and_then(|count| count.checked_mul(4))
        .and_then(|size| header.checked_add(size))
        .context("binary XML string table overflow")?;
    let data_end = if styles_start == 0 {
        chunk.len()
    } else {
        styles_start
    };
    if count > 16384 || start < offset_end || start > data_end || data_end > chunk.len() {
        bail!("invalid binary XML string table")
    }
    let data = &chunk[start..data_end];
    let mut result = Vec::with_capacity(count);
    let mut total_text = 0usize;
    for index in 0..count {
        let mut position = u32_at(chunk, header + index * 4)?;
        let value = if flags & 0x100 != 0 {
            let units = length8(data, &mut position)?;
            let length = length8(data, &mut position)?;
            if units > 8192 || length > 32768 {
                bail!("manifest string exceeds inspection limit")
            }
            let end = position
                .checked_add(length)
                .context("binary XML string overflow")?;
            let text = std::str::from_utf8(
                data.get(position..end)
                    .context("truncated binary XML string")?,
            )
            .context("invalid binary XML UTF-8")?;
            if data.get(end) != Some(&0) || text.encode_utf16().count() != units {
                bail!("binary XML string length mismatch")
            }
            text.to_owned()
        } else {
            let length = length16(data, &mut position)?;
            if length > 8192 {
                bail!("manifest string exceeds inspection limit")
            }
            let mut units = Vec::with_capacity(length);
            for index in 0..length {
                units.push(u16_at(data, position + index * 2)? as u16);
            }
            if u16_at(data, position + length * 2)? != 0 {
                bail!("unterminated binary XML UTF-16")
            }
            String::from_utf16_lossy(&units)
        };
        total_text += value.len();
        if total_text > 4 * 1024 * 1024 {
            bail!("manifest decoded strings exceed 4 MiB")
        }
        result.push(value);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/apk/AndroidManifest.axml");

    #[test]
    fn reads_package_permissions_and_explicit_default_unknown_exposure() -> Result<()> {
        let manifest = Manifest::parse(FIXTURE)?;
        assert_eq!(manifest.attributes["package"], "example.fixture");
        assert_eq!(manifest.attributes["android:versionCode"], 7);
        assert_eq!(manifest.sdk["android:targetSdkVersion"], 35);
        assert_eq!(manifest.permissions.len(), 2);
        assert_eq!(
            manifest.permissions[1].attributes["android:maxSdkVersion"],
            28
        );
        assert_eq!(manifest.declared_permissions.len(), 1);
        assert_eq!(manifest.features[0]["android:required"], false);
        let find = |name: &str| {
            manifest
                .components
                .iter()
                .find(|component| component.name.as_deref() == Some(name))
                .expect("fixture component")
        };
        let main = find("example.fixture.Main");
        assert_eq!(main.exported, Some(true));
        assert_eq!(main.exported_basis, "explicit");
        assert_eq!(main.actions, ["android.intent.action.MAIN"]);
        assert_eq!(find("example.fixture.PrivateService").exported, Some(false));
        assert_eq!(find("example.fixture.Provider").exported, Some(false));
        assert_eq!(
            find("example.fixture.Provider").exported_basis,
            "provider_target_sdk_default"
        );
        assert_eq!(find("example.fixture.NeedsReview").exported, None);
        assert_eq!(
            find("example.fixture.NeedsReview").exported_basis,
            "missing_required_exported"
        );
        Ok(())
    }

    #[test]
    fn rejects_truncated_chunks_out_of_range_strings_and_mismatched_nodes() -> Result<()> {
        for length in 0..FIXTURE.len() {
            assert!(Manifest::parse(&FIXTURE[..length]).is_err());
        }
        let mut corrupted = FIXTURE.to_vec();
        corrupted[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Manifest::parse(&corrupted).is_err());
        let mut position = 8;
        while u16_at(FIXTURE, position)? != 0x103 {
            position += u32_at(FIXTURE, position + 4)?;
        }
        let header = u16_at(FIXTURE, position + 2)?;
        let mut corrupted = FIXTURE.to_vec();
        corrupted[position + header + 4..position + header + 8]
            .copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Manifest::parse(&corrupted).is_err());
        Ok(())
    }
}
