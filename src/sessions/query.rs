//! Read-only, bounded archive queries. Saved content never grants execution authority.
use super::{validate_name, SessionDocument, MAX_SESSION_BYTES};
use crate::{config::Config, limits::truncate_text, llm::ConversationItem};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::PathBuf,
};

const SCAN_BYTES: u64 = 32 * 1024 * 1024;
const DIRECTORY_ENTRIES: usize = 1000;
const PAGE_BYTES: usize = 24 * 1024;

pub(crate) struct Archive {
    path: PathBuf,
    directory: Option<File>,
    secrets: Vec<String>,
}
impl Archive {
    pub(crate) fn open(config: &Config, protocol_token: Option<String>) -> Result<Self> {
        let config_path = match &config.source {
            Some(path) => path.clone(),
            None => crate::config::default_config_path()?,
        };
        let path = crate::config::state_dir(&config_path)?.join("sessions");
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC);
        }
        let directory = match options.open(&path) {
            Ok(file) => {
                if !file.metadata()?.is_dir() {
                    bail!("session archive is not a directory")
                }
                private(&file)?;
                Some(file)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).context("cannot open private session archive"),
        };
        let mut secrets = vec![
            config.api_key.clone(),
            config.proxy_password.clone(),
            config.protocol_token.clone(),
            config.ima_client_id.clone(),
            config.ima_api_key.clone(),
            config.jev_api_key.clone(),
        ];
        if let Ok(token) = std::env::var("NL2SH_PROTOCOL_TOKEN") {
            secrets.push(token);
        }
        if let Some(token) = protocol_token {
            secrets.push(token);
        }
        secrets.retain(|value| !value.is_empty());
        secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
        secrets.dedup();
        Ok(Self {
            path,
            directory,
            secrets,
        })
    }
    fn redact(&self, text: &str) -> String {
        self.secrets.iter().fold(text.to_owned(), |text, secret| {
            text.replace(secret, "[REDACTED]")
        })
    }
    fn load(&self, name: &str) -> Result<(SessionDocument, String, u64)> {
        validate_name(name)?;
        let directory = self
            .directory
            .as_ref()
            .context("session archive does not exist")?;
        #[cfg(unix)]
        let file = {
            use std::os::fd::{AsRawFd, FromRawFd};
            let name = std::ffi::CString::new(format!("{name}.json"))?;
            // SAFETY: directory is held open; the validated single-component name is NUL terminated.
            let fd = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error())
                    .context("cannot open session snapshot");
            }
            // SAFETY: openat returned a new, owned descriptor.
            unsafe { File::from_raw_fd(fd) }
        };
        #[cfg(not(unix))]
        let file = {
            let path = self.path.join(format!("{name}.json"));
            if fs::symlink_metadata(&path)?.file_type().is_symlink() {
                bail!("session symlinks are refused")
            }
            File::open(path)?
        };
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_SESSION_BYTES {
            bail!("invalid or oversized session snapshot")
        }
        private(&file)?;
        let mut bytes = Vec::new();
        file.take(MAX_SESSION_BYTES + 1)
            .read_to_end(&mut bytes)
            .context("cannot read session snapshot")?;
        if bytes.len() as u64 > MAX_SESSION_BYTES {
            bail!("session exceeds its size limit")
        }
        let revision = format!("{:x}", Sha256::digest(&bytes));
        let document: SessionDocument =
            serde_json::from_slice(&bytes).context("invalid session snapshot")?;
        if document.version != 1 || document.name != name {
            bail!("invalid session identity or version")
        }
        Ok((document, revision, bytes.len() as u64))
    }
    fn metadata(&self, doc: &SessionDocument, revision: &str) -> Value {
        json!({"session_id": doc.name, "title": truncate_text(&self.redact(if doc.title.is_empty() { &doc.name } else { &doc.title }), 256),
            "created_unix_secs": if doc.created_unix_secs == 0 { super::creation_time_from_name(&doc.name, doc.updated_unix_secs) } else {doc.created_unix_secs},
            "updated_unix_secs": doc.updated_unix_secs, "turns": doc.turns.len(), "revision": revision,
            "has_checkpoint": doc.web_checkpoint.is_some()})
    }
    /// Offset refers to sorted file names, including invalid snapshots, not to matching rows.
    pub(crate) fn list(&self, query: Option<&str>, offset: usize, limit: usize) -> Result<Value> {
        let Some(_) = self.directory else {
            return Ok(
                json!({"sessions": [], "next_offset": null, "directory_truncated": false, "skipped": 0, "historical": true}),
            );
        };
        let mut names = Vec::new();
        let mut directory_truncated = false;
        for (index, entry) in fs::read_dir(&self.path)?.enumerate() {
            if index >= DIRECTORY_ENTRIES {
                directory_truncated = true;
                break;
            }
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            if let Some(name) = path
                .file_stem()
                .and_then(|x| x.to_str())
                .filter(|name| validate_name(name).is_ok())
            {
                names.push(name.to_owned());
            }
        }
        names.sort();
        if offset > names.len() {
            bail!("offset exceeds archive entries")
        }
        let mut sessions = Vec::new();
        let mut page_bytes = 0;
        let mut position = offset;
        let mut scanned_bytes = 0;
        let mut skipped = 0;
        while position < names.len()
            && sessions.len() < limit
            && scanned_bytes + MAX_SESSION_BYTES <= SCAN_BYTES
        {
            let name = &names[position];
            position += 1;
            let (doc, revision, bytes) = match self.load(name) {
                Ok(doc) => doc,
                Err(_) => {
                    skipped += 1;
                    scanned_bytes += MAX_SESSION_BYTES;
                    continue;
                }
            };
            scanned_bytes += bytes;
            let mut row = self.metadata(&doc, &revision);
            if let Some(query) = query {
                let title = self.redact(&doc.title);
                let entries = self.entries(&doc)?;
                let hit = if title.contains(query) {
                    Some(json!({"kind": "title", "excerpt": excerpt(&title, query)}))
                } else {
                    entries.iter().enumerate().find_map(|(index, entry)| {
                        let text = entry["content"].as_str()?;
                        text.contains(query).then(|| {
                            json!({"kind": entry["kind"], "entry_offset": index,
                            "turn": entry["turn"], "excerpt": excerpt(text, query)})
                        })
                    })
                };
                let Some(hit) = hit else {
                    continue;
                };
                row["match"] = hit;
            }
            let row_bytes = serde_json::to_vec(&row)?.len();
            if page_bytes + row_bytes > PAGE_BYTES && !sessions.is_empty() {
                // Revisit this matching file on the next page, including JSON escaping costs.
                position -= 1;
                break;
            }
            page_bytes += row_bytes;
            sessions.push(row);
        }
        Ok(
            json!({"sessions": sessions, "next_offset": (position < names.len()).then_some(position),
            "directory_truncated": directory_truncated, "skipped": skipped, "scanned_bytes": scanned_bytes,
            "historical": true, "scope": "saved snapshots in the current configuration; live unsaved state and audit logs excluded"}),
        )
    }
    fn entries(&self, doc: &SessionDocument) -> Result<Vec<Value>> {
        let mut entries = Vec::new();
        for (turn, items) in doc.turns.iter().enumerate() {
            for item in items {
                match item {
                    ConversationItem::Message(message) => entries
                        .push(json!({"kind": "message", "turn": turn,
                        "role": message.role, "content": self.redact(&message.content)})),
                    ConversationItem::Tools(round) => {
                        for call in &round.calls {
                            entries.push(json!({"kind": "tool_call", "turn": turn, "call_id": truncate_text(&self.redact(&call.id), 128),
                                "tool": truncate_text(&self.redact(&call.name), 128), "content": self.redact(&serde_json::to_string(&call.arguments)?)}));
                        }
                        for result in &round.results {
                            entries.push(json!({"kind": "tool_result", "turn": turn,
                                "call_id": truncate_text(&self.redact(&result.call_id), 128), "success": result.success,
                                "content": self.redact(&result.output), "attachments_omitted": result.attachments.len()}));
                        }
                    }
                }
            }
        }
        if let Some(checkpoint) = &doc.web_checkpoint {
            entries.push(json!({"kind": "checkpoint", "diagnostic_only": true,
                "status": truncate_text(&self.redact(&checkpoint.status), 64),
                "content": self.redact(&checkpoint.history.join("\n"))}));
        }
        Ok(entries)
    }
    pub(crate) fn read(
        &self,
        name: &str,
        offset: usize,
        limit: usize,
        content_bytes: usize,
        revision: Option<&str>,
    ) -> Result<Value> {
        let (doc, actual_revision, _) = self.load(name)?;
        if revision.is_some_and(|revision| revision != actual_revision) {
            bail!("session changed; restart pagination with its new revision")
        }
        let all = self.entries(&doc)?;
        if offset > all.len() {
            bail!("offset exceeds session entries")
        }
        let total = all.len();
        let mut entries = Vec::new();
        let mut bytes = 0;
        for mut entry in all.into_iter().skip(offset).take(limit) {
            let text = entry["content"].as_str().unwrap_or_default();
            let bounded = truncate_text(text, content_bytes);
            entry["content_truncated"] =
                json!(bounded != text || text.contains(crate::limits::TRUNCATION_LABEL));
            entry["content"] = json!(bounded);
            let mut size = serde_json::to_vec(&entry)?.len();
            if entries.is_empty() {
                let mut bound = content_bytes;
                while size > PAGE_BYTES {
                    bound /= 2;
                    let text = entry["content"].as_str().unwrap_or_default();
                    entry["content"] = json!(truncate_text(text, bound));
                    entry["content_truncated"] = json!(true);
                    size = serde_json::to_vec(&entry)?.len();
                }
            }
            if bytes + size > PAGE_BYTES && !entries.is_empty() {
                break;
            }
            bytes += size;
            entries.push(entry);
        }
        let next = offset + entries.len();
        Ok(
            json!({"session": self.metadata(&doc, &actual_revision), "entries": entries, "total_entries": total,
            "offset": offset, "next_offset": (next < total).then_some(next), "historical": true,
            "scope": "saved bounded history, not proof of current device state; checkpoint is diagnostic only"}),
        )
    }
}
fn excerpt(text: &str, query: &str) -> String {
    let position = text.find(query).unwrap_or(0);
    let mut start = position.saturating_sub(120);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    truncate_text(&text[start..], 512)
}
fn private(file: &File) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        // SAFETY: geteuid has no arguments or memory preconditions.
        let uid = unsafe { libc::geteuid() };
        if metadata.mode() & 0o077 != 0 || (uid != 0 && metadata.uid() != uid) {
            bail!("session archive requires private permissions and the owning UID")
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        llm::{ConversationMessage, Role, ToolCall, ToolResult, ToolRound},
        sessions::{SessionStore, WebCheckpoint},
    };
    fn setup() -> Result<(tempfile::TempDir, Config, SessionStore)> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("config.toml");
        let config = Config {
            source: Some(path.clone()),
            api_key: "current-secret".into(),
            ..Config::default()
        };
        let store = SessionStore::open(&path)?;
        Ok((dir, config, store))
    }
    #[test]
    fn archive_search_read_preserve_evidence_and_paginate() -> Result<()> {
        let (_dir, config, store) = setup()?;
        let turn = vec![
            ConversationItem::Message(ConversationMessage::new(
                Role::User,
                "分析 com.konka.athena",
            )),
            ConversationItem::Tools(ToolRound {
                calls: vec![ToolCall {
                    id: "call-1".into(),
                    name: "android_crash_report".into(),
                    arguments: json!({"package": "com.konka.athena", "token": "current-secret"}),
                }],
                results: vec![ToolResult {
                    call_id: "call-1".into(),
                    output: "stack trace current-secret".repeat(100),
                    success: false,
                    attachments: vec![],
                }],
            }),
            ConversationItem::Message(ConversationMessage::new(
                Role::Assistant,
                "historical hypothesis",
            )),
        ];
        store.save_redacted_with_title("web-1", "闪退分析", &[turn], 10000, &[])?;
        store.save("protocol-2", &[], 1000)?;
        let archive = Archive::open(&config, Some("generated-token".into()))?;
        let list = archive.list(None, 0, 1)?;
        assert_eq!(list["sessions"][0]["session_id"], "protocol-2");
        assert_eq!(list["next_offset"], 1);
        let hit = archive.list(Some("com.konka.athena"), 0, 10)?;
        assert_eq!(hit["sessions"][0]["session_id"], "web-1");
        assert_eq!(hit["sessions"][0]["match"]["entry_offset"], 0);
        assert!(archive.list(Some("current-secret"), 0, 10)?["sessions"]
            .as_array()
            .is_some_and(Vec::is_empty));
        let read = archive.read("web-1", 1, 1, 256, None)?;
        assert_eq!(read["entries"][0]["call_id"], "call-1");
        assert!(!serde_json::to_string(&read)?.contains("current-secret"));
        let revision = read["session"]["revision"]
            .as_str()
            .context("revision missing")?;
        let result = archive.read("web-1", 2, 1, 256, Some(revision))?;
        assert_eq!(result["entries"][0]["success"], false);
        assert_eq!(result["entries"][0]["content_truncated"], true);
        assert_eq!(result["next_offset"], 3);
        store.save("web-1", &[], 1000)?;
        assert!(archive.read("web-1", 0, 10, 256, Some(revision)).is_err());
        Ok(())
    }
    #[test]
    fn archive_handles_missing_invalid_checkpoints_and_budgets() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let config = Config {
            source: Some(dir.path().join("config.toml")),
            ..Config::default()
        };
        let empty = Archive::open(&config, None)?;
        assert_eq!(empty.list(None, 0, 10)?["sessions"], json!([]));
        assert!(!dir.path().join("sessions").exists());
        let store = SessionStore::open(config.source.as_deref().context("config source")?)?;
        let checkpoint = WebCheckpoint {
            status: "interrupted".into(),
            activity: "tool".into(),
            history: vec!["checkpoint needle".into()],
            events: vec![],
        };
        store.save_web_state(
            "checkpoint",
            "unfinished",
            &[],
            1000,
            &[],
            Some(&checkpoint),
        )?;
        let archive = Archive::open(&config, None)?;
        let hit = archive.list(Some("needle"), 0, 10)?;
        assert_eq!(hit["sessions"][0]["match"]["kind"], "checkpoint");
        assert_eq!(
            archive.read("checkpoint", 0, 10, 256, None)?["entries"][0]["diagnostic_only"],
            true
        );
        assert!(archive.read("../config", 0, 10, 256, None).is_err());
        assert!(archive.read("checkpoint", 5, 10, 256, None).is_err());
        let turn = vec![ConversationItem::Message(ConversationMessage::new(
            Role::User,
            "\u{0001}".repeat(30000),
        ))];
        store.save("escaped", &[turn], 1000)?;
        let read = archive.read("escaped", 0, 20, 16384, None)?;
        assert!(serde_json::to_vec(&read)?.len() < PAGE_BYTES + 1500);
        assert_eq!(read["entries"][0]["content_truncated"], true);
        Ok(())
    }
    #[test]
    fn escaped_search_pages_stay_valid_and_visit_every_match() -> Result<()> {
        let (_dir, config, store) = setup()?;
        let turn = vec![ConversationItem::Message(ConversationMessage::new(
            Role::User,
            format!("needle{}", "\u{0001}".repeat(1000)),
        ))];
        for index in 0..20 {
            store.save_redacted_with_title(
                &format!("session-{index:02}"),
                &"\u{0001}".repeat(160),
                std::slice::from_ref(&turn),
                10000,
                &[],
            )?;
        }
        let archive = Archive::open(&config, None)?;
        for query in [None, Some("needle")] {
            let mut offset = 0;
            let mut count = 0;
            loop {
                let page = archive.list(query, offset, 20)?;
                assert!(serde_json::to_vec(&page)?.len() < PAGE_BYTES + 1500);
                count += page["sessions"].as_array().context("missing rows")?.len();
                let Some(next) = page["next_offset"].as_u64() else {
                    break;
                };
                assert!(next > offset as u64);
                offset = next as usize;
            }
            assert_eq!(count, 20);
        }
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn archive_refuses_symlinks_special_files_and_nonprivate_snapshots() -> Result<()> {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let (dir, config, store) = setup()?;
        store.save("safe", &[], 1000)?;
        symlink(
            dir.path().join("sessions/safe.json"),
            dir.path().join("sessions/link.json"),
        )?;
        let archive = Archive::open(&config, None)?;
        assert!(archive.read("link", 0, 10, 256, None).is_err());
        fs::set_permissions(
            dir.path().join("sessions/safe.json"),
            fs::Permissions::from_mode(0o644),
        )?;
        assert!(archive.read("safe", 0, 10, 256, None).is_err());
        assert_eq!(archive.list(None, 0, 10)?["skipped"], 2);
        fs::remove_file(dir.path().join("sessions/link.json"))?;
        fs::remove_file(dir.path().join("sessions/safe.json"))?;
        fs::remove_dir(dir.path().join("sessions"))?;
        symlink(dir.path(), dir.path().join("sessions"))?;
        assert!(Archive::open(&config, None).is_err());
        Ok(())
    }
}
