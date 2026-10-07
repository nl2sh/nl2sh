//! Runtime-environment detection shared by configuration, prompts, and shells.

use std::{env, ffi::OsStr, path::PathBuf};

mod capabilities;
pub(crate) mod resources;
pub use capabilities::RuntimeCapabilities;

/// Android userspace hosting the current nl2sh process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidRuntime {
    /// Direct deployment from adb/root Android shell.
    AndroidShell,
    /// Compatibility deployment inside the Termux application userspace.
    Termux,
}

/// Detects direct Android shell versus Termux without executing a subprocess.
pub fn android_runtime() -> AndroidRuntime {
    classify_android_runtime(
        env::var_os("TERMUX_VERSION").as_deref(),
        env::var_os("PREFIX").as_deref(),
    )
}

/// Returns whether the process is running in a recognizable Termux userspace.
pub fn is_termux() -> bool {
    android_runtime() == AndroidRuntime::Termux
}

/// Returns the Termux prefix supplied by the runtime when it is recognizable.
pub fn termux_prefix() -> Option<PathBuf> {
    is_termux()
        .then(|| env::var_os("PREFIX"))
        .flatten()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn classify_android_runtime(
    termux_version: Option<&OsStr>,
    prefix: Option<&OsStr>,
) -> AndroidRuntime {
    let has_termux_version = termux_version.is_some_and(|value| !value.is_empty());
    let has_termux_prefix = prefix.is_some_and(|value| {
        !value.is_empty() && value.to_string_lossy().contains("com.termux/files/usr")
    });
    if has_termux_version || has_termux_prefix {
        AndroidRuntime::Termux
    } else {
        AndroidRuntime::AndroidShell
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_termux_markers_without_confusing_normal_prefixes() {
        assert_eq!(
            classify_android_runtime(Some(OsStr::new("0.118")), None),
            AndroidRuntime::Termux
        );
        assert_eq!(
            classify_android_runtime(None, Some(OsStr::new("/data/data/com.termux/files/usr"))),
            AndroidRuntime::Termux
        );
        assert_eq!(
            classify_android_runtime(None, Some(OsStr::new("/usr/local"))),
            AndroidRuntime::AndroidShell
        );
    }
}
