//! Small private, persistent key/value notes for Agent continuity.

use anyhow::{bail, Context, Result};
use schemars::JsonSchema;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_ENTRIES: usize = 128;
const MAX_KEY_BYTES: usize = 128;
const MAX_VALUE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentMemoryArgs {
    pub action: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentMemory {
    path: PathBuf,
}

impl AgentMemory {
    pub fn new(base: &Path) -> Self {
        Self {
            path: base.join(".nl2sh-agent-memory.json"),
        }
    }

    pub fn read(&self, args: &AgentMemoryArgs) -> Result<String> {
        let values = self.load()?;
        match args.action.as_str() {
            "get" => {
                let key = valid_key(args.key.as_deref())?;
                serde_json::to_string_pretty(
                    &serde_json::json!({"key":key,"value":values.get(key)}),
                )
                .context("cannot encode memory result")
            }
            "list" => serde_json::to_string_pretty(&values).context("cannot encode memory result"),
            _ => bail!("read action must be get or list"),
        }
    }

    pub fn mutation_summary(&self, args: &AgentMemoryArgs) -> Result<String> {
        match args.action.as_str() {
            "set" => {
                let key = valid_key(args.key.as_deref())?;
                let value = args.value.as_deref().context("set requires value")?;
                if value.len() > MAX_VALUE_BYTES {
                    bail!("memory value is too large")
                }
                Ok(format!(
                    "Set persistent Agent memory key: {key}\nValue bytes: {}\nExact value:\n{value}",
                    value.len(),
                ))
            }
            "delete" => Ok(format!(
                "Delete persistent Agent memory key: {}",
                valid_key(args.key.as_deref())?
            )),
            "clear" => Ok("Clear all persistent Agent memory entries".into()),
            _ => bail!("mutation action must be set, delete, or clear"),
        }
    }

    pub fn apply(&self, args: &AgentMemoryArgs) -> Result<String> {
        let mut values = self.load()?;
        match args.action.as_str() {
            "set" => {
                let key = valid_key(args.key.as_deref())?.to_owned();
                let value = args.value.as_deref().context("set requires value")?;
                if value.len() > MAX_VALUE_BYTES {
                    bail!("memory value is too large")
                }
                if !values.contains_key(&key) && values.len() >= MAX_ENTRIES {
                    bail!("Agent memory entry limit reached")
                }
                values.insert(key, value.to_owned());
            }
            "delete" => {
                values.remove(valid_key(args.key.as_deref())?);
            }
            "clear" => values.clear(),
            _ => bail!("mutation action must be set, delete, or clear"),
        }
        self.store(&values)?;
        Ok(format!(
            "Agent memory updated; {} entries stored.",
            values.len()
        ))
    }

    fn load(&self) -> Result<BTreeMap<String, String>> {
        match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("cannot parse {}", self.path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(error) => {
                Err(error).with_context(|| format!("cannot read {}", self.path.display()))
            }
        }
    }

    fn store(&self, values: &BTreeMap<String, String>) -> Result<()> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock before epoch")?
            .as_nanos();
        let temporary = parent.join(format!(".nl2sh-memory-{nonce}.tmp"));
        let result = (|| -> Result<()> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&temporary)
                .with_context(|| format!("cannot create {}", temporary.display()))?;
            file.write_all(&serde_json::to_vec_pretty(values)?)
                .context("cannot write Agent memory")?;
            file.sync_all().context("cannot sync Agent memory")?;
            fs::rename(&temporary, &self.path)
                .with_context(|| format!("cannot replace {}", self.path.display()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn valid_key(value: Option<&str>) -> Result<&str> {
    let value = value.context("memory action requires key")?;
    if value.is_empty()
        || value.len() > MAX_KEY_BYTES
        || !value
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '/'))
    {
        bail!("invalid Agent memory key")
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persists_and_deletes_notes() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let memory = AgentMemory::new(dir.path());
        let set = AgentMemoryArgs {
            action: "set".into(),
            key: Some("task.note".into()),
            value: Some("done".into()),
        };
        memory.apply(&set)?;
        assert!(memory
            .read(&AgentMemoryArgs {
                action: "get".into(),
                key: set.key.clone(),
                value: None
            })?
            .contains("done"));
        memory.apply(&AgentMemoryArgs {
            action: "delete".into(),
            key: set.key,
            value: None,
        })?;
        assert!(!memory
            .read(&AgentMemoryArgs {
                action: "list".into(),
                key: None,
                value: None
            })?
            .contains("done"));
        Ok(())
    }
}
