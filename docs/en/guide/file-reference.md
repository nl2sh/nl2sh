# @ file and directory references

Typing `@` in TUI/Web opens path suggestions. Supported forms include `@file.txt`, `@dir/`, `@./relative`, `@../parent`, `@/absolute`, and `@~/home`. Select with Up/Down, complete with Enter/Tab, and drill into directories. Right remains cursor movement.

Submission resolves the longest existing path prefix; a question may immediately follow a path, such as `@test.txt写的是什么内容`. Absolute path hints go to the Agent; references neither execute files nor read all content automatically.

File tools have no workspace sandbox and accept absolute, parent, and symlink paths. Process permissions and size/count limits still apply. Patches show a diff and require approval before atomic writes. Arrows in the Web file panel also insert references.

Example: “Summarize recent errors in `@/data/local/tmp/logs/`, limit the query scope, and do not modify files.” See [file tools](../tools/filesystem.md).
