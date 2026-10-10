//! Process-shared ownership of Android input and focus resources.

use anyhow::{bail, Context, Result};
use std::{
    fs::{File, OpenOptions},
    future::Future,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

tokio::task_local! { static UI_LEASE_HELD: bool; }

/// An acquired kernel lock. Closing the descriptor releases ownership on every exit path.
#[derive(Debug)]
pub(crate) struct UiLease(File);

/// Whether shell commands in this process can modify the shared Android UI.
pub(crate) fn privileged_android_process() -> bool {
    cfg!(target_os = "android") && matches!(unsafe { libc::geteuid() }, 0 | 2000)
}

fn lock_path() -> PathBuf {
    if cfg!(target_os = "android") {
        PathBuf::from("/data/local/tmp/nl2sh-device-ui.lock")
    } else {
        std::env::temp_dir().join(format!("nl2sh-device-ui-{}.lock", unsafe {
            libc::geteuid()
        }))
    }
}

impl UiLease {
    pub(crate) async fn acquire() -> Result<Self> {
        Self::acquire_path(&lock_path(), Duration::from_secs(10)).await
    }

    async fn acquire_path(path: &Path, wait: Duration) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(path)
            .context("cannot open Android UI resource lock")?;
        let metadata = file
            .metadata()
            .context("cannot inspect Android UI resource lock")?;
        let uid = unsafe { libc::geteuid() };
        let permitted_owner = if cfg!(target_os = "android") {
            matches!(metadata.uid(), 0 | 2000)
        } else {
            metadata.uid() == uid
        };
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.len() != 0
            || metadata.mode() & 0o077 != 0
            || !permitted_owner
        {
            bail!("Android UI resource lock has unsafe ownership or permissions")
        }
        // A root-created inode must remain accessible to the adb shell process.
        #[cfg(target_os = "android")]
        if uid == 0
            && metadata.uid() == 0
            && unsafe { libc::fchown(file.as_raw_fd(), 2000, 2000) } != 0
        {
            return Err(std::io::Error::last_os_error())
                .context("cannot assign UI lock to Android shell");
        }
        let started = Instant::now();
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(Self(file));
            }
            let error = std::io::Error::last_os_error();
            if !matches!(error.raw_os_error(), Some(libc::EAGAIN) | Some(libc::EINTR)) {
                return Err(error).context("cannot acquire Android UI resource lock");
            }
            if started.elapsed() >= wait {
                bail!("Android UI is busy with another task; retry after it finishes")
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

impl Drop for UiLease {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
        // Do not unlink: waiters must continue to refer to the same inode.
    }
}

pub(crate) async fn with_ui_lease<F: Future>(held: bool, future: F) -> F::Output {
    UI_LEASE_HELD.scope(held, future).await
}

pub(crate) fn ui_lease_held() -> bool {
    UI_LEASE_HELD.try_with(|held| *held).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn child_process_holds_ui_lock() -> Result<()> {
        let Some(path) = std::env::var_os("NL2SH_TEST_UI_LOCK") else {
            return Ok(());
        };
        let path = PathBuf::from(path);
        let _lease = UiLease::acquire_path(&path, Duration::from_secs(1)).await?;
        std::fs::write(path.with_extension("ready"), b"ready")?;
        let mut release = String::new();
        std::io::stdin().read_line(&mut release)?;
        Ok(())
    }

    #[tokio::test]
    async fn coordinates_with_another_process_and_recovers_after_its_exit() -> Result<()> {
        use std::process::Stdio;
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("ui.lock");
        let mut child = tokio::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "runtime::resources::tests::child_process_holds_ui_lock",
            ])
            .env("NL2SH_TEST_UI_LOCK", &path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let started = Instant::now();
        while !path.with_extension("ready").exists() {
            anyhow::ensure!(
                started.elapsed() < Duration::from_secs(3),
                "child lock was not ready"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(UiLease::acquire_path(&path, Duration::from_millis(25))
            .await
            .is_err());
        child.kill().await?;
        child.wait().await?;
        let _lease = UiLease::acquire_path(&path, Duration::from_secs(1)).await?;
        Ok(())
    }

    #[tokio::test]
    async fn exclusive_ownership_survives_waiter_cancellation_and_releases_on_drop() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("ui.lock");
        let first = UiLease::acquire_path(&path, Duration::from_millis(100)).await?;
        assert!(UiLease::acquire_path(&path, Duration::from_millis(25))
            .await
            .is_err());
        let waiting = UiLease::acquire_path(&path, Duration::from_secs(1));
        assert!(tokio::time::timeout(Duration::from_millis(25), waiting)
            .await
            .is_err());
        drop(first);
        let second = UiLease::acquire_path(&path, Duration::from_millis(100)).await?;
        assert!(path.exists());
        drop(second);
        Ok(())
    }

    #[tokio::test]
    async fn refuses_symlinks_and_public_lock_files() -> Result<()> {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("target");
        std::fs::write(&target, [])?;
        let link = directory.path().join("link");
        symlink(&target, &link)?;
        assert!(UiLease::acquire_path(&link, Duration::ZERO).await.is_err());
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o666))?;
        assert!(UiLease::acquire_path(&target, Duration::ZERO)
            .await
            .is_err());
        Ok(())
    }
}

tokio::task_local! { static BACKGROUND_UI_LEASE: Option<std::sync::Arc<UiLease>>; }

pub(crate) async fn with_background_ui_lease<F: Future>(
    lease: Option<std::sync::Arc<UiLease>>,
    future: F,
) -> F::Output {
    BACKGROUND_UI_LEASE.scope(lease, future).await
}
pub(crate) fn background_ui_lease() -> Option<std::sync::Arc<UiLease>> {
    BACKGROUND_UI_LEASE.try_with(Clone::clone).ok().flatten()
}

pub(crate) fn background_ui_scope_present() -> bool {
    BACKGROUND_UI_LEASE.try_with(|_| ()).is_ok()
}
