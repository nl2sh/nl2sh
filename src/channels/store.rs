//! Private channel credentials and bounded delivery ledger.
use super::*;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
};

#[derive(Debug)]
pub(super) struct AlreadyManaged;
impl std::fmt::Display for AlreadyManaged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("channels are already managed by another process")
    }
}
impl std::error::Error for AlreadyManaged {}

pub(super) struct Store {
    root: PathBuf,
    _lock: File,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let root = crate::config::state_dir(path)?.join("channels");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&root)?;
        let meta = fs::symlink_metadata(&root)?;
        if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
            bail!("channel directory must be private and owned by the current user");
        }
        let lock = Self::file(&root.join("lock"))?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::WouldBlock {
                return Err(AlreadyManaged.into());
            }
            return Err(error).context("cannot lock channel state");
        }
        Ok(Self { root, _lock: lock })
    }
    fn file(path: &Path) -> Result<File> {
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
            || meta.mode() & 0o077 != 0
            || meta.nlink() != 1
        {
            bail!("channel state must be a private regular file");
        }
        Ok(file)
    }
    pub fn load(&self) -> Result<Saved> {
        let path = self.root.join("accounts.json");
        if !path.try_exists()? {
            return Ok(Saved::default());
        }
        let file = Self::file(&path)?;
        if file.metadata()?.len() > 1024 * 1024 {
            bail!("channel state exceeds its size limit");
        }
        let mut text = String::new();
        file.take(1024 * 1024).read_to_string(&mut text)?;
        Ok(serde_json::from_str(&text).context("invalid channel state")?)
    }
    pub fn save(&self, saved: &Saved) -> Result<()> {
        let tmp = self.root.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&tmp)?;
            file.write_all(&serde_json::to_vec(saved)?)?;
            file.sync_all()?;
            fs::rename(&tmp, self.root.join("accounts.json"))?;
            File::open(&self.root)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(tmp);
        }
        result
    }
}
