//! Small private, persistent SQLite key/value notes for Agent continuity.

use crate::config;
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_ENTRIES: usize = 128;
const MAX_KEY_BYTES: usize = 128;
const MAX_VALUE_BYTES: usize = 16 * 1024;
const DATABASE_NAME: &str = "agent-memory.sqlite3";

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
/// Supported private Agent notebook operations exposed in the tool schema.
pub enum AgentMemoryAction {
    /// Read one value by key.
    Get,
    /// List all stored keys and values.
    List,
    /// Create or replace one value after confirmation.
    Set,
    /// Delete one key after confirmation.
    Delete,
    /// Delete all entries after confirmation.
    Clear,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentMemoryArgs {
    pub action: AgentMemoryAction,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
}

/// One persistent memory entry returned to explicit local interfaces.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MemoryEntry {
    /// Stable caller-provided key.
    pub key: String,
    /// UTF-8 value stored for the key.
    pub value: String,
    /// Last update time as Unix seconds.
    pub updated_unix_secs: u64,
}

#[derive(Debug, Clone)]
pub struct AgentMemory {
    directory: PathBuf,
    path: PathBuf,
}

impl AgentMemory {
    /// Opens memory at the platform-specific persistent location.
    pub fn open(config_path: &Path) -> Result<Self> {
        Self::in_directory(config::memory_dir(config_path)?)
    }

    /// Creates a memory handle in an explicit directory.
    pub fn in_directory(directory: PathBuf) -> Result<Self> {
        if let Ok(metadata) = fs::symlink_metadata(&directory) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                bail!("memory path is not a real directory")
            }
        }
        Ok(Self {
            path: directory.join(DATABASE_NAME),
            directory,
        })
    }

    pub fn read(&self, args: &AgentMemoryArgs) -> Result<String> {
        match args.action {
            AgentMemoryAction::Get => {
                let key = valid_key(args.key.as_deref())?;
                let connection = self.connection()?;
                let value = connection
                    .query_row(
                        "SELECT value FROM memory_entries WHERE key = ?1",
                        params![key],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .context("cannot read Agent memory")?;
                serde_json::to_string_pretty(&serde_json::json!({"key":key,"value":value}))
                    .context("cannot encode memory result")
            }
            AgentMemoryAction::List => {
                let values = self.values()?;
                serde_json::to_string_pretty(&values).context("cannot encode memory result")
            }
            _ => bail!("read action must be get or list"),
        }
    }

    /// Returns all entries in stable key order for explicit local UI access.
    pub fn entries(&self) -> Result<Vec<MemoryEntry>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT key, value, updated_unix_secs FROM memory_entries ORDER BY key COLLATE BINARY",
            )
            .context("cannot prepare Agent memory listing")?;
        let rows = statement
            .query_map([], |row| {
                Ok(MemoryEntry {
                    key: row.get(0)?,
                    value: row.get(1)?,
                    updated_unix_secs: row.get::<_, i64>(2)?.max(0) as u64,
                })
            })
            .context("cannot list Agent memory")?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("cannot decode Agent memory")
    }

    pub fn mutation_summary(args: &AgentMemoryArgs) -> Result<String> {
        match args.action {
            AgentMemoryAction::Set => {
                let key = valid_key(args.key.as_deref())?;
                let value = valid_value(args.value.as_deref())?;
                Ok(format!(
                    "Set persistent Agent memory key: {key}\nValue bytes: {}\nExact value:\n{value}",
                    value.len(),
                ))
            }
            AgentMemoryAction::Delete => Ok(format!(
                "Delete persistent Agent memory key: {}",
                valid_key(args.key.as_deref())?
            )),
            AgentMemoryAction::Clear => Ok("Clear all persistent Agent memory entries".into()),
            _ => bail!("mutation action must be set, delete, or clear"),
        }
    }

    pub fn apply(&self, args: &AgentMemoryArgs) -> Result<String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("cannot begin Agent memory transaction")?;
        match args.action {
            AgentMemoryAction::Set => {
                let key = valid_key(args.key.as_deref())?;
                let value = valid_value(args.value.as_deref())?;
                let exists = transaction
                    .query_row(
                        "SELECT 1 FROM memory_entries WHERE key = ?1",
                        params![key],
                        |_| Ok(()),
                    )
                    .optional()
                    .context("cannot inspect Agent memory key")?
                    .is_some();
                if !exists {
                    let count: i64 = transaction
                        .query_row("SELECT COUNT(*) FROM memory_entries", [], |row| row.get(0))
                        .context("cannot count Agent memory")?;
                    if count >= MAX_ENTRIES as i64 {
                        bail!("Agent memory entry limit reached")
                    }
                }
                let now = i64::try_from(unix_seconds()?).context("system time is too large")?;
                transaction
                    .execute(
                        "INSERT INTO memory_entries (key, value, created_unix_secs, updated_unix_secs) VALUES (?1, ?2, ?3, ?3) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_unix_secs = excluded.updated_unix_secs",
                        params![key, value, now],
                    )
                    .context("cannot set Agent memory")?;
            }
            AgentMemoryAction::Delete => {
                transaction
                    .execute(
                        "DELETE FROM memory_entries WHERE key = ?1",
                        params![valid_key(args.key.as_deref())?],
                    )
                    .context("cannot delete Agent memory")?;
            }
            AgentMemoryAction::Clear => {
                transaction
                    .execute("DELETE FROM memory_entries", [])
                    .context("cannot clear Agent memory")?;
            }
            _ => bail!("mutation action must be set, delete, or clear"),
        }
        let count: i64 = transaction
            .query_row("SELECT COUNT(*) FROM memory_entries", [], |row| row.get(0))
            .context("cannot count Agent memory")?;
        transaction
            .commit()
            .context("cannot commit Agent memory transaction")?;
        Ok(format!("Agent memory updated; {count} entries stored."))
    }

    fn values(&self) -> Result<BTreeMap<String, String>> {
        Ok(self
            .entries()?
            .into_iter()
            .map(|entry| (entry.key, entry.value))
            .collect())
    }

    fn connection(&self) -> Result<Connection> {
        self.ensure_private_storage()?;
        let connection = Connection::open(&self.path)
            .with_context(|| format!("cannot open {}", self.path.display()))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .context("cannot configure Agent memory timeout")?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = DELETE;\
                 PRAGMA synchronous = FULL;\
                 CREATE TABLE IF NOT EXISTS memory_entries (\
                   key TEXT PRIMARY KEY NOT NULL,\
                   value TEXT NOT NULL,\
                   created_unix_secs INTEGER NOT NULL,\
                   updated_unix_secs INTEGER NOT NULL\
                 );",
            )
            .context("cannot initialize Agent memory database")?;
        Ok(connection)
    }

    fn ensure_private_storage(&self) -> Result<()> {
        if !self.directory.exists() {
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder
                .create(&self.directory)
                .with_context(|| format!("cannot create {}", self.directory.display()))?;
        }
        let metadata = fs::symlink_metadata(&self.directory)
            .with_context(|| format!("cannot inspect {}", self.directory.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("memory path is not a real directory")
        }
        #[cfg(unix)]
        fs::set_permissions(
            &self.directory,
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .with_context(|| format!("cannot protect {}", self.directory.display()))?;

        match fs::symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                bail!("Agent memory database is not a regular file")
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                match options.open(&self.path) {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error).context("cannot create Agent memory database"),
                }
            }
            Err(error) => return Err(error).context("cannot inspect Agent memory database"),
        }
        #[cfg(unix)]
        fs::set_permissions(
            &self.path,
            std::os::unix::fs::PermissionsExt::from_mode(0o600),
        )
        .with_context(|| format!("cannot protect {}", self.path.display()))?;
        Ok(())
    }
}

fn unix_seconds() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before epoch")?
        .as_secs())
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

fn valid_value(value: Option<&str>) -> Result<&str> {
    let value = value.context("set requires value")?;
    if value.len() > MAX_VALUE_BYTES {
        bail!("memory value is too large")
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_updates_and_deletes_notes_in_sqlite() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let memory = AgentMemory::in_directory(dir.path().join("memory"))?;
        let mut set = AgentMemoryArgs {
            action: AgentMemoryAction::Set,
            key: Some("task.note".into()),
            value: Some("done".into()),
        };
        memory.apply(&set)?;
        set.value = Some("updated".into());
        memory.apply(&set)?;
        assert_eq!(memory.entries()?[0].value, "updated");
        assert!(memory
            .read(&AgentMemoryArgs {
                action: AgentMemoryAction::Get,
                key: set.key.clone(),
                value: None,
            })?
            .contains("updated"));
        memory.apply(&AgentMemoryArgs {
            action: AgentMemoryAction::Delete,
            key: set.key,
            value: None,
        })?;
        assert!(memory.entries()?.is_empty());
        assert!(dir.path().join("memory/agent-memory.sqlite3").is_file());
        assert!(!dir.path().join(".nl2sh-agent-memory.json").exists());
        Ok(())
    }

    #[test]
    fn clear_removes_every_entry() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let memory = AgentMemory::in_directory(dir.path().join("memory"))?;
        for key in ["one", "two"] {
            memory.apply(&AgentMemoryArgs {
                action: AgentMemoryAction::Set,
                key: Some(key.into()),
                value: Some(key.into()),
            })?;
        }
        memory.apply(&AgentMemoryArgs {
            action: AgentMemoryAction::Clear,
            key: None,
            value: None,
        })?;
        assert!(memory.entries()?.is_empty());
        Ok(())
    }

    #[test]
    fn rejects_unknown_actions_before_risk_routing() {
        let error = serde_json::from_value::<AgentMemoryArgs>(serde_json::json!({
            "action": "read"
        }))
        .expect_err("unknown memory actions must be rejected");
        let message = error.to_string();
        assert!(message.contains("unknown variant `read`"));
        assert!(message.contains("get"));
        assert!(message.contains("list"));
        assert!(message.contains("set"));
        assert!(message.contains("delete"));
        assert!(message.contains("clear"));
    }
}
