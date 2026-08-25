use crate::{limits::truncate_text, llm::ConversationItem};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_SESSION_BYTES: u64 = 4 * 1024 * 1024;
const MAX_SESSIONS: usize = 200;

#[derive(Debug, Serialize, Deserialize)]
struct SessionDocument {
    version: u32,
    name: String,
    updated_unix_secs: u64,
    turns: Vec<Vec<ConversationItem>>,
}

/// Metadata safe to display in `/sessions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    /// User-visible session name.
    pub name: String,
    /// Number of complete conversation turns.
    pub turns: usize,
    /// Last save time as Unix seconds.
    pub updated_unix_secs: u64,
}

/// Private session directory adjacent to the active configuration file.
#[derive(Debug, Clone)]
pub struct SessionStore {
    directory: PathBuf,
}

impl SessionStore {
    /// Opens or creates the private session directory.
    pub fn open(config_path: &Path) -> Result<Self> {
        let parent = config_path.parent().unwrap_or_else(|| Path::new("."));
        let directory = parent.join("sessions");
        create_private_dir(&directory)?;
        Ok(Self { directory })
    }

    /// Creates a collision-resistant default name without device or account data.
    pub fn default_name() -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis());
        format!("session-{timestamp}-{}", std::process::id())
    }

    /// Atomically saves bounded complete turns with private permissions.
    pub fn save(
        &self,
        name: &str,
        turns: &[Vec<ConversationItem>],
        tool_limit: usize,
    ) -> Result<()> {
        self.save_redacted(name, turns, tool_limit, &[])
    }

    /// Saves a session after removing configured credentials from text and arguments.
    pub fn save_redacted(
        &self,
        name: &str,
        turns: &[Vec<ConversationItem>],
        tool_limit: usize,
        secrets: &[String],
    ) -> Result<()> {
        validate_name(name)?;
        let mut bounded = bound_turns(turns, tool_limit);
        redact_turns(&mut bounded, secrets);
        let document = SessionDocument {
            version: 1,
            name: name.into(),
            updated_unix_secs: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            turns: bounded,
        };
        let encoded = serde_json::to_vec(&document).context("cannot encode session")?;
        if encoded.len() as u64 > MAX_SESSION_BYTES {
            bail!("session exceeds {MAX_SESSION_BYTES} byte limit")
        }
        let target = self.path(name);
        let temporary = self.directory.join(format!(".{name}.tmp"));
        let mut file = private_new_file(&temporary)?;
        let result = (|| -> Result<()> {
            file.write_all(&encoded).context("cannot write session")?;
            file.flush().context("cannot flush session")?;
            fs::rename(&temporary, &target)
                .with_context(|| format!("cannot replace session {name}"))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    /// Loads a session and reapplies turn and tool-result bounds.
    pub fn load(
        &self,
        name: &str,
        max_turns: usize,
        tool_limit: usize,
    ) -> Result<Vec<Vec<ConversationItem>>> {
        validate_name(name)?;
        let path = self.path(name);
        let metadata =
            fs::metadata(&path).with_context(|| format!("cannot inspect session {name}"))?;
        if !metadata.is_file() || metadata.len() > MAX_SESSION_BYTES {
            bail!("session is invalid or exceeds its size limit")
        }
        let bytes = fs::read(&path).with_context(|| format!("cannot read session {name}"))?;
        let document: SessionDocument =
            serde_json::from_slice(&bytes).context("invalid session data")?;
        if document.version != 1 || document.name != name {
            bail!("session identity or version is invalid")
        }
        let start = document.turns.len().saturating_sub(max_turns);
        Ok(bound_turns(&document.turns[start..], tool_limit))
    }

    /// Lists saved sessions without reading unrelated files.
    pub fn list(&self) -> Result<Vec<SessionInfo>> {
        let mut sessions = Vec::new();
        for entry in fs::read_dir(&self.directory).context("cannot list sessions")? {
            if sessions.len() >= MAX_SESSIONS {
                break;
            }
            let entry = entry.context("cannot read session entry")?;
            if entry.path().extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let metadata = entry.metadata().context("cannot inspect session entry")?;
            if !metadata.is_file() || metadata.len() > MAX_SESSION_BYTES {
                continue;
            }
            let Ok(bytes) = fs::read(entry.path()) else {
                continue;
            };
            let Ok(document) = serde_json::from_slice::<SessionDocument>(&bytes) else {
                continue;
            };
            sessions.push(SessionInfo {
                name: document.name,
                turns: document.turns.len(),
                updated_unix_secs: document.updated_unix_secs,
            });
        }
        sessions.sort_by(|a, b| {
            b.updated_unix_secs
                .cmp(&a.updated_unix_secs)
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(sessions)
    }

    /// Renames an existing session without overwriting another.
    pub fn rename(&self, old: &str, new: &str) -> Result<()> {
        validate_name(old)?;
        validate_name(new)?;
        let target = self.path(new);
        if target.exists() {
            bail!("session {new} already exists")
        }
        let turns = self.load(old, usize::MAX, MAX_SESSION_BYTES as usize)?;
        self.save(new, &turns, MAX_SESSION_BYTES as usize)?;
        fs::remove_file(self.path(old))
            .with_context(|| format!("renamed session but cannot remove old session {old}"))
    }

    /// Deletes exactly one named session.
    pub fn delete(&self, name: &str) -> Result<()> {
        validate_name(name)?;
        fs::remove_file(self.path(name)).with_context(|| format!("cannot delete session {name}"))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(format!("{name}.json"))
    }
}

fn bound_turns(turns: &[Vec<ConversationItem>], tool_limit: usize) -> Vec<Vec<ConversationItem>> {
    turns
        .iter()
        .cloned()
        .map(|turn| {
            turn.into_iter()
                .map(|item| match item {
                    ConversationItem::Tools(mut round) => {
                        for result in &mut round.results {
                            result.output = truncate_text(&result.output, tool_limit);
                        }
                        ConversationItem::Tools(round)
                    }
                    other => other,
                })
                .collect()
        })
        .collect()
}

fn redact_turns(turns: &mut [Vec<ConversationItem>], secrets: &[String]) {
    let secrets = secrets
        .iter()
        .filter(|secret| !secret.is_empty())
        .collect::<Vec<_>>();
    for turn in turns {
        for item in turn {
            match item {
                ConversationItem::Message(message) => redact_text(&mut message.content, &secrets),
                ConversationItem::Tools(round) => {
                    for call in &mut round.calls {
                        redact_json(&mut call.arguments, &secrets);
                    }
                    for result in &mut round.results {
                        redact_text(&mut result.output, &secrets);
                    }
                }
            }
        }
    }
}

fn redact_json(value: &mut serde_json::Value, secrets: &[&String]) {
    match value {
        serde_json::Value::String(text) => redact_text(text, secrets),
        serde_json::Value::Array(values) => {
            for value in values {
                redact_json(value, secrets);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                redact_json(value, secrets);
            }
        }
        _ => {}
    }
}

fn redact_text(text: &mut String, secrets: &[&String]) {
    for secret in secrets {
        if text.contains(secret.as_str()) {
            *text = text.replace(secret.as_str(), "[NL2SH CREDENTIAL REDACTED]");
        }
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        bail!("session name must be 1-64 ASCII letters, digits, '-' or '_'")
    }
    Ok(())
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder
        .create(path)
        .with_context(|| format!("cannot create sessions directory {}", path.display()))
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("cannot create sessions directory {}", path.display()))
}

#[cfg(unix)]
fn private_new_file(path: &Path) -> Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("cannot create private session file {}", path.display()))
}

#[cfg(not(unix))]
fn private_new_file(path: &Path) -> Result<fs::File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("cannot create session file {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{ConversationMessage, Role};

    #[test]
    fn saves_lists_loads_renames_and_deletes() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = SessionStore::open(&directory.path().join("config.toml"))?;
        let turns = vec![vec![ConversationItem::Message(ConversationMessage::new(
            Role::User,
            "hello",
        ))]];
        store.save("first", &turns, 1024)?;
        assert_eq!(store.list()?[0].name, "first");
        assert_eq!(store.load("first", 10, 1024)?, turns);
        store.rename("first", "second")?;
        store.delete("second")?;
        assert!(store.list()?.is_empty());
        assert!(store.save("../escape", &turns, 1024).is_err());
        let secret_turns = vec![vec![ConversationItem::Message(ConversationMessage::new(
            Role::User,
            "token=secret-value",
        ))]];
        store.save_redacted("redacted", &secret_turns, 1024, &["secret-value".into()])?;
        let stored = fs::read_to_string(directory.path().join("sessions/redacted.json"))?;
        assert!(!stored.contains("secret-value"));
        assert!(stored.contains("NL2SH CREDENTIAL REDACTED"));
        Ok(())
    }
}
