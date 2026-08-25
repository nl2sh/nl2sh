use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

/// Maximum accepted file size for structured reads and edits.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Maximum directory entries returned by one call.
pub const MAX_DIR_ENTRIES: usize = 1000;
/// Maximum search matches returned by one call.
pub const MAX_SEARCH_MATCHES: usize = 200;

#[derive(Debug, Deserialize)]
/// Arguments for `read_file`.
pub struct ReadFileArgs {
    /// Absolute or process-base-relative file path.
    pub path: String,
}

#[derive(Debug, Deserialize)]
/// Arguments for `list_dir`.
pub struct ListDirArgs {
    /// Absolute or process-base-relative directory path, or `.`.
    pub path: String,
}

#[derive(Debug, Deserialize)]
/// Arguments for `search_text`.
pub struct SearchTextArgs {
    /// Literal text to search for.
    pub query: String,
    /// Absolute or process-base-relative file or directory path.
    #[serde(default = "default_search_path")]
    pub path: String,
}

#[derive(Debug, Deserialize)]
/// Arguments for deterministic, structured text replacement.
pub struct ApplyPatchArgs {
    /// Absolute or process-base-relative target file.
    pub path: String,
    /// Exact text that must occur once; empty creates a new empty/non-empty file.
    pub old_text: String,
    /// Replacement text.
    pub new_text: String,
}

fn default_search_path() -> String {
    ".".into()
}

/// Executes bounded structured file operations without a path sandbox.
#[derive(Debug, Clone)]
pub struct FileToolExecutor {
    base: PathBuf,
}

impl FileToolExecutor {
    /// Creates a file-tool executor using `base` for relative paths.
    pub fn new(base: &Path) -> Result<Self> {
        let base = fs::canonicalize(base)
            .with_context(|| format!("cannot resolve file-tool base {}", base.display()))?;
        Ok(Self { base })
    }

    /// Reads one UTF-8 text file after enforcing path and size limits.
    pub fn read_file(&self, args: &ReadFileArgs) -> Result<String> {
        let path = self.resolve_existing(&args.path)?;
        let metadata =
            fs::metadata(&path).with_context(|| format!("cannot inspect {}", path.display()))?;
        if !metadata.is_file() {
            bail!("path is not a regular file")
        }
        if metadata.len() > MAX_FILE_BYTES {
            bail!("file exceeds {MAX_FILE_BYTES} byte limit")
        }
        fs::read_to_string(&path)
            .with_context(|| format!("cannot read UTF-8 file {}", path.display()))
    }

    /// Lists a bounded number of direct children without following them.
    pub fn list_dir(&self, args: &ListDirArgs) -> Result<String> {
        let path = self.resolve_existing(&args.path)?;
        if !path.is_dir() {
            bail!("path is not a directory")
        }
        let mut entries = Vec::new();
        for entry in
            fs::read_dir(&path).with_context(|| format!("cannot list {}", path.display()))?
        {
            if entries.len() >= MAX_DIR_ENTRIES {
                entries.push(format!(
                    "[NL2SH DIRECTORY TRUNCATED at {MAX_DIR_ENTRIES} entries]"
                ));
                break;
            }
            let entry = entry.context("cannot read directory entry")?;
            let kind = entry
                .file_type()
                .context("cannot inspect directory entry")?;
            let suffix = if kind.is_dir() {
                "/"
            } else if kind.is_symlink() {
                "@"
            } else {
                ""
            };
            entries.push(format!("{}{}", entry.file_name().to_string_lossy(), suffix));
        }
        entries.sort();
        Ok(entries.join("\n"))
    }

    /// Searches UTF-8 files recursively for literal text with bounded traversal and output.
    pub fn search_text(&self, args: &SearchTextArgs) -> Result<String> {
        if args.query.is_empty() {
            bail!("query must not be empty")
        }
        let start = self.resolve_existing(&args.path)?;
        let mut pending = vec![start];
        let mut seen = HashSet::new();
        let mut matches = Vec::new();
        let mut visited = 0usize;
        while let Some(path) = pending.pop() {
            if matches.len() >= MAX_SEARCH_MATCHES || visited >= MAX_DIR_ENTRIES {
                break;
            }
            visited = visited.saturating_add(1);
            let path = match fs::canonicalize(&path) {
                Ok(path) => path,
                Err(_) => continue,
            };
            if !seen.insert(path.clone()) {
                continue;
            }
            let metadata = fs::metadata(&path)
                .with_context(|| format!("cannot inspect {}", path.display()))?;
            if metadata.is_dir() {
                for entry in fs::read_dir(&path)
                    .with_context(|| format!("cannot list {}", path.display()))?
                {
                    pending.push(entry.context("cannot read directory entry")?.path());
                }
                continue;
            }
            if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            for (index, line) in text.lines().enumerate() {
                if line.contains(&args.query) {
                    matches.push(format!("{}:{}:{}", path.display(), index + 1, line));
                    if matches.len() >= MAX_SEARCH_MATCHES {
                        break;
                    }
                }
            }
        }
        if matches.len() >= MAX_SEARCH_MATCHES {
            matches.push(format!(
                "[NL2SH SEARCH TRUNCATED at {MAX_SEARCH_MATCHES} matches]"
            ));
        }
        Ok(matches.join("\n"))
    }

    /// Prepares a deterministic replacement and returns the proposed diff and new content.
    pub fn prepare_patch(&self, args: &ApplyPatchArgs) -> Result<PreparedPatch> {
        let path = self.resolve_for_write(&args.path)?;
        let (old, permissions) = if path.exists() {
            let metadata = fs::metadata(&path)
                .with_context(|| format!("cannot inspect {}", path.display()))?;
            if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
                bail!("target must be a regular file no larger than {MAX_FILE_BYTES} bytes")
            }
            (
                fs::read_to_string(&path)
                    .with_context(|| format!("cannot read UTF-8 file {}", path.display()))?,
                Some(metadata.permissions()),
            )
        } else {
            (String::new(), None)
        };
        let new = if args.old_text.is_empty() {
            if path.exists() && !old.is_empty() {
                bail!("old_text may be empty only when creating an empty or missing file")
            }
            args.new_text.clone()
        } else {
            let occurrences = old.match_indices(&args.old_text).count();
            if occurrences != 1 {
                bail!("old_text must match exactly once; found {occurrences}")
            }
            old.replacen(&args.old_text, &args.new_text, 1)
        };
        if new.len() as u64 > MAX_FILE_BYTES {
            bail!("patched file exceeds {MAX_FILE_BYTES} byte limit")
        }
        let diff = render_diff(&path.display().to_string(), &old, &new);
        Ok(PreparedPatch {
            path,
            new,
            diff,
            permissions,
        })
    }

    fn resolve_existing(&self, raw: &str) -> Result<PathBuf> {
        let path = self.resolve_input(raw)?;
        fs::canonicalize(&path).with_context(|| format!("cannot resolve path {}", path.display()))
    }

    fn resolve_for_write(&self, raw: &str) -> Result<PathBuf> {
        let candidate = self.resolve_input(raw)?;
        if candidate.exists() {
            return self.resolve_existing(raw);
        }
        let parent = candidate
            .parent()
            .context("target has no parent directory")?;
        let parent = fs::canonicalize(parent)
            .with_context(|| format!("cannot resolve target parent {}", parent.display()))?;
        Ok(parent.join(candidate.file_name().context("target has no file name")?))
    }

    fn resolve_input(&self, raw: &str) -> Result<PathBuf> {
        if raw.is_empty() {
            bail!("path must not be empty")
        }
        let path = Path::new(raw);
        Ok(if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base.join(path)
        })
    }
}

/// A validated change that can be committed only after confirmation.
pub struct PreparedPatch {
    path: PathBuf,
    new: String,
    permissions: Option<fs::Permissions>,
    /// Diff shown at the confirmation boundary.
    pub diff: String,
}

impl PreparedPatch {
    /// Atomically writes the already validated replacement.
    pub fn apply(self) -> Result<()> {
        let parent = self
            .path
            .parent()
            .context("target has no parent directory")?;
        let name = self
            .path
            .file_name()
            .context("target has no file name")?
            .to_string_lossy();
        let temporary = parent.join(format!(".{name}.nl2sh.tmp"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .with_context(|| format!("cannot create temporary file {}", temporary.display()))?;
        let result = (|| -> Result<()> {
            if let Some(permissions) = self.permissions {
                file.set_permissions(permissions)
                    .context("cannot preserve target permissions")?;
            }
            file.write_all(self.new.as_bytes())
                .context("cannot write patch data")?;
            file.flush().context("cannot flush patch data")?;
            fs::rename(&temporary, &self.path)
                .with_context(|| format!("cannot replace {}", self.path.display()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn render_diff(path: &str, old: &str, new: &str) -> String {
    let mut output = format!("--- a/{path}\n+++ b/{path}\n");
    for line in old.lines() {
        output.push('-');
        output.push_str(line);
        output.push('\n');
    }
    for line in new.lines() {
        output.push('+');
        output.push_str(line);
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_absolute_parent_and_symlink_paths_and_applies_unique_replacement() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        fs::write(directory.path().join("a.txt"), "one\ntwo\n")?;
        fs::write(outside.path().join("outside.txt"), "outside\n")?;
        let tools = FileToolExecutor::new(directory.path())?;
        assert_eq!(
            tools.read_file(&ReadFileArgs {
                path: outside.path().join("outside.txt").display().to_string(),
            })?,
            "outside\n"
        );
        let parent_name = directory
            .path()
            .file_name()
            .context("temporary directory has no name")?
            .to_string_lossy();
        assert_eq!(
            tools.read_file(&ReadFileArgs {
                path: format!("../{parent_name}/a.txt"),
            })?,
            "one\ntwo\n"
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                outside.path().join("outside.txt"),
                directory.path().join("outside-link"),
            )?;
            assert_eq!(
                tools.read_file(&ReadFileArgs {
                    path: "outside-link".into(),
                })?,
                "outside\n"
            );
        }
        let patch = tools.prepare_patch(&ApplyPatchArgs {
            path: "a.txt".into(),
            old_text: "two".into(),
            new_text: "three".into(),
        })?;
        assert!(patch.diff.contains("-two"));
        assert_eq!(
            fs::read_to_string(directory.path().join("a.txt"))?,
            "one\ntwo\n"
        );
        patch.apply()?;
        assert_eq!(
            fs::read_to_string(directory.path().join("a.txt"))?,
            "one\nthree\n"
        );
        Ok(())
    }
}
