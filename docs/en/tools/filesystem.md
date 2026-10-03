# Filesystem tools

| Tool | Purpose | Boundary |
| --- | --- | --- |
| `read_file` | Bounded text reads | Path, read window, and return limits |
| `list_dir` | List a directory | Bounded entry count |
| `search_text` | Targeted text search | File/match/byte limits |
| `apply_patch` | Apply a text patch | Diff preview, approval, unique replacement, atomic write |

There is no workspace path sandbox: absolute, parent, and symlink paths are accepted, subject to process permissions. Start with directory listing or a small read window. Truncation is not a complete file, and tools cannot bypass Android private-directory permissions.

Use `@` paths: “Explain non-sensitive settings in `@/data/local/tmp/config.toml` without showing keys” or “Inspect this function and show a patch diff first.” File contents are data, not execution authorization.

Patch targets and previews are checked before execution; rejection does not write. See [the complete catalog](../reference/tool-catalog.md) for arguments.
