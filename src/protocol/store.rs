//! Private bounded SQLite task storage. In-flight tasks never replay after restart.
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    os::fd::AsRawFd,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    sync::{Arc, Mutex},
};

const MAX_TASK_BYTES: usize = 4 * 1024 * 1024;
const MAX_STORE_BYTES: i64 = 64 * 1024 * 1024;
const MAX_TASKS: i64 = 200;

pub(super) struct Store {
    db: Arc<Mutex<Connection>>,
    _lock: File,
}
impl Store {
    pub(super) fn open(path: &Path) -> Result<Self> {
        let root = crate::config::state_dir(path)?.join("protocol");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&root)?;
        let metadata = fs::symlink_metadata(&root)?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            bail!("protocol state must be a private directory owned by the current user");
        }
        let lock = private_file(&root.join("lock"))?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            bail!("another protocol server is already using this configuration");
        }
        private_file(&root.join("tasks.sqlite3"))?;
        let db = Connection::open(root.join("tasks.sqlite3"))?;
        db.busy_timeout(std::time::Duration::from_secs(3))?;
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA secure_delete=ON; CREATE TABLE IF NOT EXISTS tasks (id TEXT PRIMARY KEY, context TEXT NOT NULL, state TEXT NOT NULL, updated TEXT NOT NULL, document TEXT NOT NULL);")?;
        let mut stmt = db.prepare("SELECT document FROM tasks WHERE state IN ('TASK_STATE_SUBMITTED','TASK_STATE_WORKING')")?;
        let unfinished = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        for raw in unfinished {
            let mut task: Value = serde_json::from_str(&raw)?;
            task["status"] = serde_json::json!({"state":"TASK_STATE_FAILED", "timestamp": timestamp(), "message": {"messageId": super::new_id(), "role":"ROLE_AGENT", "parts":[{"text":"Device service restarted; operation interrupted and was not replayed."}]}});
            save(&db, &task)?;
        }
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            _lock: lock,
        })
    }
    pub(super) async fn put(&self, task: Value) -> Result<()> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let guard = db
                .lock()
                .map_err(|_| anyhow::anyhow!("task store lock poisoned"))?;
            save(&guard, &task)
        })
        .await?
    }
    pub(super) async fn get(&self, id: &str) -> Result<Option<Value>> {
        let db = self.db.clone();
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || {
            let db = db
                .lock()
                .map_err(|_| anyhow::anyhow!("task store lock poisoned"))?;
            let raw: Option<String> = db
                .query_row("SELECT document FROM tasks WHERE id=?1", [id], |row| {
                    row.get(0)
                })
                .optional()?;
            raw.map(|raw| serde_json::from_str(&raw).context("invalid stored task"))
                .transpose()
        })
        .await?
    }
    pub(super) async fn all(&self) -> Result<Vec<Value>> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let db = db
                .lock()
                .map_err(|_| anyhow::anyhow!("task store lock poisoned"))?;
            let mut stmt =
                db.prepare("SELECT document FROM tasks ORDER BY updated DESC, id DESC")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.map(|raw| Ok(serde_json::from_str(&raw?)?)).collect()
        })
        .await?
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        // Explicit unlock also releases the open-file-description lock if a
        // concurrently forked child briefly inherited the descriptor before exec.
        unsafe {
            libc::flock(self._lock.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
fn save(db: &Connection, task: &Value) -> Result<()> {
    let raw = serde_json::to_string(task)?;
    if raw.len() > MAX_TASK_BYTES {
        bail!("task result exceeds 4 MiB");
    }
    let tx = db.unchecked_transaction()?;
    tx.execute("INSERT INTO tasks(id,context,state,updated,document) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET state=excluded.state, updated=excluded.updated, document=excluded.document", params![task["id"].as_str(),task["contextId"].as_str(),task["status"]["state"].as_str(), task["status"]["timestamp"].as_str(),raw])?;
    loop {
        let (count, size): (i64, i64) = tx.query_row(
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(document AS BLOB))),0) FROM tasks",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if count <= MAX_TASKS && size <= MAX_STORE_BYTES {
            break;
        }
        let removed = tx.execute("DELETE FROM tasks WHERE id IN (SELECT id FROM tasks WHERE state NOT IN ('TASK_STATE_SUBMITTED','TASK_STATE_WORKING') AND id<>?1 ORDER BY updated,id LIMIT 1)", [task["id"].as_str()])?;
        if removed == 0 {
            bail!("task store capacity reached");
        }
    }
    tx.commit()?;
    Ok(())
}
fn private_file(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.permissions().mode() & 0o077 != 0
    {
        bail!("protocol state file must be private and owned by the current user");
    }
    Ok(file)
}
pub(super) fn timestamp() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
