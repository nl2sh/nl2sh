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
    #[serde(default)]
    title: String,
    #[serde(default)]
    created_unix_secs: u64,
    updated_unix_secs: u64,
    turns: Vec<Vec<ConversationItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    web_checkpoint: Option<WebCheckpoint>,
}

struct SaveMetadata<'a> {
    name: &'a str,
    title: &'a str,
    created_override: Option<u64>,
}

/// Diagnostic display state for one unfinished Web request; never used as model history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebCheckpoint {
    /// Running, interrupted, cancelled, or failed.
    pub status: String,
    /// Last known execution phase.
    pub activity: String,
    /// Bounded display lines for this request.
    pub history: Vec<String>,
    /// Bounded phase and tool completion events with Unix timestamps.
    pub events: Vec<WebCheckpointEvent>,
}

/// One low-sensitivity Web task progress event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebCheckpointEvent {
    /// Unix timestamp in seconds.
    pub at: u64,
    /// Fixed event name, without model text or tool arguments.
    pub kind: String,
    /// Optional registered tool name.
    pub tool: Option<String>,
}

/// Metadata safe to display in `/sessions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    /// User-visible session name.
    pub name: String,
    /// Short user-facing title; old snapshots fall back to the stable name.
    pub title: String,
    /// Creation time as Unix seconds, retained across later saves.
    pub created_unix_secs: u64,
    /// Number of complete conversation turns.
    pub turns: usize,
    /// Last save time as Unix seconds.
    pub updated_unix_secs: u64,
}

/// Private session directory in the active state directory.
#[derive(Debug, Clone)]
pub struct SessionStore {
    directory: PathBuf,
}

impl SessionStore {
    /// Opens or creates the private session directory.
    pub fn open(config_path: &Path) -> Result<Self> {
        let directory = crate::config::state_dir(config_path)?.join("sessions");
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
        self.save_redacted_with_title(name, name, turns, tool_limit, secrets)
    }

    /// Saves a session with an independent user-facing title.
    pub fn save_redacted_with_title(
        &self,
        name: &str,
        title: &str,
        turns: &[Vec<ConversationItem>],
        tool_limit: usize,
        secrets: &[String],
    ) -> Result<()> {
        self.save_web_state(name, title, turns, tool_limit, secrets, None)
    }

    /// Saves complete turns and an optional diagnostic Web checkpoint atomically.
    pub fn save_web_state(
        &self,
        name: &str,
        title: &str,
        turns: &[Vec<ConversationItem>],
        tool_limit: usize,
        secrets: &[String],
        checkpoint: Option<&WebCheckpoint>,
    ) -> Result<()> {
        self.save_web_state_with_metadata(
            SaveMetadata {
                name,
                title,
                created_override: None,
            },
            turns,
            tool_limit,
            secrets,
            checkpoint,
        )
    }

    fn save_web_state_with_metadata(
        &self,
        metadata: SaveMetadata<'_>,
        turns: &[Vec<ConversationItem>],
        tool_limit: usize,
        secrets: &[String],
        checkpoint: Option<&WebCheckpoint>,
    ) -> Result<()> {
        let SaveMetadata {
            name,
            title,
            created_override,
        } = metadata;
        validate_name(name)?;
        let title = if title.trim().is_empty() {
            name
        } else {
            title.trim()
        };
        if title.len() > 160 {
            bail!("session title exceeds 160 bytes")
        }
        let mut bounded = bound_turns(turns, tool_limit);
        redact_turns(&mut bounded, secrets);
        let mut web_checkpoint = checkpoint.cloned();
        if let Some(current) = web_checkpoint.as_mut() {
            current.history = current
                .history
                .iter()
                .rev()
                .take(100)
                .rev()
                .map(|line| truncate_text(line, 16 * 1024))
                .collect();
            current.events = current
                .events
                .iter()
                .rev()
                .take(120)
                .rev()
                .cloned()
                .collect();
            let secrets = secrets
                .iter()
                .filter(|secret| !secret.is_empty())
                .collect::<Vec<_>>();
            for line in &mut current.history {
                redact_text(line, &secrets);
            }
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
        let target = self.path(name);
        let created_unix_secs = created_override.unwrap_or_else(|| {
            fs::read(&target)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<SessionDocument>(&bytes).ok())
                .filter(|previous| previous.name == name && previous.version == 1)
                .map_or_else(
                    || creation_time_from_name(name, now),
                    |previous| {
                        if previous.created_unix_secs == 0 {
                            creation_time_from_name(name, previous.updated_unix_secs)
                        } else {
                            previous.created_unix_secs
                        }
                    },
                )
        });
        let document = SessionDocument {
            version: 1,
            name: name.into(),
            title: title.into(),
            created_unix_secs,
            updated_unix_secs: now,
            turns: bounded,
            web_checkpoint,
        };
        let encoded = serde_json::to_vec(&document).context("cannot encode session")?;
        if encoded.len() as u64 > MAX_SESSION_BYTES {
            bail!("session exceeds {MAX_SESSION_BYTES} byte limit")
        }
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

    /// Loads an unfinished Web request without adding it to model history.
    pub fn load_web_checkpoint(&self, name: &str) -> Result<Option<WebCheckpoint>> {
        validate_name(name)?;
        let path = self.path(name);
        let bytes = fs::read(&path).with_context(|| format!("cannot read session {name}"))?;
        if bytes.len() as u64 > MAX_SESSION_BYTES {
            bail!("session exceeds its size limit")
        }
        let document: SessionDocument =
            serde_json::from_slice(&bytes).context("invalid session data")?;
        if document.version != 1 || document.name != name {
            bail!("session identity or version is invalid")
        }
        Ok(document.web_checkpoint)
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
            let title = if document.title.is_empty() {
                document.name.clone()
            } else {
                document.title
            };
            let created_unix_secs = if document.created_unix_secs == 0 {
                creation_time_from_name(&document.name, document.updated_unix_secs)
            } else {
                document.created_unix_secs
            };
            sessions.push(SessionInfo {
                name: document.name,
                title,
                created_unix_secs,
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

    /// Loads one session's display metadata without the list limit.
    pub fn info(&self, name: &str) -> Result<SessionInfo> {
        validate_name(name)?;
        let bytes =
            fs::read(self.path(name)).with_context(|| format!("cannot read session {name}"))?;
        if bytes.len() as u64 > MAX_SESSION_BYTES {
            bail!("session exceeds its size limit")
        }
        let document: SessionDocument =
            serde_json::from_slice(&bytes).context("invalid session data")?;
        if document.version != 1 || document.name != name {
            bail!("session identity or version is invalid")
        }
        Ok(SessionInfo {
            name: name.to_owned(),
            title: if document.title.is_empty() {
                name.to_owned()
            } else {
                document.title
            },
            created_unix_secs: if document.created_unix_secs == 0 {
                creation_time_from_name(name, document.updated_unix_secs)
            } else {
                document.created_unix_secs
            },
            turns: document.turns.len(),
            updated_unix_secs: document.updated_unix_secs,
        })
    }

    /// Renames an existing session without overwriting another.
    pub fn rename(&self, old: &str, new: &str) -> Result<()> {
        validate_name(old)?;
        validate_name(new)?;
        let target = self.path(new);
        if target.exists() {
            bail!("session {new} already exists")
        }
        let info = self.info(old)?;
        let turns = self.load(old, usize::MAX, MAX_SESSION_BYTES as usize)?;
        let checkpoint = self.load_web_checkpoint(old)?;
        self.save_web_state_with_metadata(
            SaveMetadata {
                name: new,
                title: &info.title,
                created_override: Some(info.created_unix_secs),
            },
            &turns,
            MAX_SESSION_BYTES as usize,
            &[],
            checkpoint.as_ref(),
        )?;
        fs::remove_file(self.path(old))
            .with_context(|| format!("renamed session but cannot remove old session {old}"))
    }

    /// Deletes exactly one named session.
    pub fn delete(&self, name: &str) -> Result<()> {
        validate_name(name)?;
        fs::remove_file(self.path(name)).with_context(|| format!("cannot delete session {name}"))
    }

    /// Deletes every saved snapshot with a valid session filename.
    pub fn delete_all(&self) -> Result<usize> {
        let mut deleted = 0;
        for entry in fs::read_dir(&self.directory).context("cannot list sessions")? {
            let entry = entry.context("cannot read session entry")?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            if validate_name(name).is_err() {
                continue;
            }
            if !entry
                .file_type()
                .context("cannot inspect session entry")?
                .is_file()
            {
                continue;
            }
            fs::remove_file(&path).with_context(|| format!("cannot delete session {name}"))?;
            deleted += 1;
        }
        Ok(deleted)
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

fn creation_time_from_name(name: &str, fallback: u64) -> u64 {
    name.strip_prefix("session-")
        .and_then(|suffix| suffix.split('-').next())
        .and_then(|timestamp| timestamp.parse::<u64>().ok())
        .map(|millis| millis / 1000)
        .filter(|seconds| *seconds > 0 && *seconds <= fallback)
        .unwrap_or(fallback)
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
        let created = store.list()?[0].created_unix_secs;
        store.save_redacted_with_title("first", "LLM generated title", &turns, 1024, &[])?;
        assert_eq!(store.list()?[0].title, "LLM generated title");
        assert_eq!(store.list()?[0].created_unix_secs, created);
        assert_eq!(store.load("first", 10, 1024)?, turns);
        store.rename("first", "second")?;
        assert_eq!(store.list()?[0].created_unix_secs, created);
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

    #[test]
    fn old_snapshot_recovers_creation_time_and_keeps_it_on_save() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = SessionStore::open(&directory.path().join("config.toml"))?;
        let name = "session-1600000000000-7";
        store.save(name, &[], 1024)?;
        let path = directory.path().join(format!("sessions/{name}.json"));
        let mut old: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
        old.as_object_mut()
            .context("session object missing")?
            .remove("created_unix_secs");
        fs::write(&path, serde_json::to_vec(&old)?)?;
        assert_eq!(store.info(name)?.created_unix_secs, 1_600_000_000);
        store.save_redacted_with_title(name, "标题", &[], 1024, &[])?;
        assert_eq!(store.info(name)?.created_unix_secs, 1_600_000_000);
        Ok(())
    }

    #[test]
    fn web_checkpoint_is_private_redacted_and_separate_from_model_turns() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = SessionStore::open(&directory.path().join("config.toml"))?;
        let checkpoint = WebCheckpoint {
            status: "running".into(),
            activity: "tool".into(),
            history: vec!["> token=secret-value".into(), "🔧 read_file".into()],
            events: vec![WebCheckpointEvent {
                at: 123,
                kind: "tool_started".into(),
                tool: Some("read_file".into()),
            }],
        };
        store.save_web_state(
            "web",
            "新会话",
            &[],
            1024,
            &["secret-value".into()],
            Some(&checkpoint),
        )?;
        assert!(store.load("web", 10, 1024)?.is_empty());
        let restored = store
            .load_web_checkpoint("web")?
            .context("missing checkpoint")?;
        assert_eq!(restored.status, "running");
        assert!(restored.history[0].contains("[NL2SH CREDENTIAL REDACTED]"));
        let raw = fs::read_to_string(directory.path().join("sessions/web.json"))?;
        assert!(!raw.contains("secret-value"));
        store.save_redacted_with_title("web", "完成", &[], 1024, &[])?;
        assert!(store.load_web_checkpoint("web")?.is_none());
        Ok(())
    }
}
