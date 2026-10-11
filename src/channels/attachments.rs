//! Opaque references to immutable tool attachments, owned by one channel task.
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{collections::BTreeMap, sync::Mutex};

#[derive(Default)]
pub(crate) struct AttachmentStore {
    entries: Mutex<BTreeMap<String, (String, Vec<u8>)>>,
}
impl AttachmentStore {
    pub(crate) fn stage(&self, attachment: &crate::llm::ToolAttachment) -> Result<String> {
        if attachment.base64_data.len() > super::media::MAX_FILE_BYTES.div_ceil(3) * 4 {
            bail!("tool attachment exceeds delivery limit");
        }
        let bytes = STANDARD.decode(&attachment.base64_data)?;
        if bytes.is_empty() || bytes.len() > super::media::MAX_FILE_BYTES {
            bail!("tool attachment has invalid size");
        }
        let extension = match attachment.media_type.as_str() {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            "audio/wav" | "audio/x-wav" => "wav",
            "audio/mpeg" => "mp3",
            "audio/ogg" => "ogg",
            "video/mp4" => "mp4",
            "application/pdf" => "pdf",
            _ => "bin",
        };
        let id = uuid::Uuid::new_v4().to_string();
        let name = format!("attachment.{extension}");
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| anyhow!("attachment store unavailable"))?;
        let size: usize = entries.values().map(|(_, bytes)| bytes.len()).sum();
        if entries.len() >= 8 || size.saturating_add(bytes.len()) > 32 * 1024 * 1024 {
            bail!("task attachment capacity reached");
        }
        entries.insert(id.clone(), (name, bytes));
        Ok(id)
    }
    pub(crate) fn snapshot(&self, id: &str) -> Result<(String, Vec<u8>)> {
        self.entries.lock().map_err(|_| anyhow!("attachment store unavailable"))?
            .get(id).cloned().context("attachment ID unknown or expired; use a current-task attachment ID or an existing local path")
    }
}
