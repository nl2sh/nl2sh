use crate::limits::truncate_text;
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

#[derive(Clone)]
/// Append-only JSON Lines history used for diagnostics across process restarts.
pub struct HistoryLog {
    path: PathBuf,
    state: Arc<Mutex<HistoryState>>,
    event_max_bytes: usize,
    file_max_bytes: u64,
}

struct HistoryState {
    file: File,
    bytes: u64,
    full: bool,
}

#[derive(Serialize)]
struct HistoryRecord<'a> {
    timestamp_ms: u128,
    event: &'a str,
    message: &'a str,
}

impl HistoryLog {
    /// Opens the configured log path, resolving relative paths beside config.toml.
    pub fn open(config_path: &Path, configured_path: &Path) -> Result<Self> {
        Self::open_with_limits(config_path, configured_path, 256 * 1024, 10 * 1024 * 1024)
    }

    /// Opens a bounded history log. Once the file limit is reached, logging
    /// stops for the process instead of growing the file without bound.
    pub fn open_with_limits(
        config_path: &Path,
        configured_path: &Path,
        event_max_bytes: usize,
        file_max_bytes: u64,
    ) -> Result<Self> {
        let path = if configured_path.is_absolute() {
            configured_path.to_path_buf()
        } else {
            config_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(configured_path)
        };
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        options.mode(0o600);
        let file = options
            .open(&path)
            .with_context(|| format!("cannot open history log {}", path.display()))?;
        let bytes = file
            .metadata()
            .context("cannot inspect history log size")?
            .len();
        Ok(Self {
            path,
            state: Arc::new(Mutex::new(HistoryState {
                file,
                bytes,
                full: bytes >= file_max_bytes,
            })),
            event_max_bytes,
            file_max_bytes,
        })
    }

    /// Appends and flushes one structured event so crash diagnostics remain available.
    pub fn record(&self, event: &str, message: &str) -> Result<()> {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let message = truncate_text(message, self.event_max_bytes);
        let record = HistoryRecord {
            timestamp_ms,
            event,
            message: &message,
        };
        let encoded = serde_json::to_string(&record).context("cannot encode history record")?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("history log lock is poisoned"))?;
        if state.full {
            return Ok(());
        }
        let encoded_bytes = encoded.len() as u64 + 1;
        let marker = serde_json::to_string(&HistoryRecord {
            timestamp_ms,
            event: "log_limit",
            message: "[NL2SH LOG TRUNCATED: file size limit reached; later events omitted]",
        })?;
        let marker_bytes = marker.len() as u64 + 1;
        if state
            .bytes
            .saturating_add(encoded_bytes)
            .saturating_add(marker_bytes)
            > self.file_max_bytes
        {
            if state.bytes.saturating_add(marker.len() as u64 + 1) <= self.file_max_bytes {
                writeln!(state.file, "{marker}")?;
                state.bytes += marker_bytes;
                state.file.flush()?;
            }
            state.full = true;
            return Ok(());
        }
        writeln!(state.file, "{encoded}")
            .with_context(|| format!("cannot write history log {}", self.path.display()))?;
        state.bytes += encoded_bytes;
        state
            .file
            .flush()
            .with_context(|| format!("cannot flush history log {}", self.path.display()))
    }

    /// Truncates the active log and resumes bounded logging from an empty file.
    pub fn clear(&self) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("history log lock is poisoned"))?;
        state
            .file
            .flush()
            .with_context(|| format!("cannot flush history log {}", self.path.display()))?;
        state
            .file
            .set_len(0)
            .with_context(|| format!("cannot clear history log {}", self.path.display()))?;
        state.bytes = 0;
        state.full = false;
        Ok(())
    }

    /// Returns the resolved log path shown to users and diagnostics.
    pub fn path(&self) -> &Path {
        &self.path
    }
}
