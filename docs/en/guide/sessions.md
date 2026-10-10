# Sessions and audit logs

Private snapshots are saved after each complete Agent turn. The current model generates a title asynchronously after the first turn; failure does not affect the reply or save. Snapshots contain bounded conversations and tool results, excluding API keys, proxy passwords, balances, and temporary grants. Temporary images are not persisted.

```text
/sessions
/sessions resume NAME
/sessions rename OLD NEW
/sessions delete NAME
/new
/clear
```

`/sessions` sorts by update time; choose a number or Up/Down + Enter to restore. Stable names accept letters, digits, `-`, and `_`. Restore reapplies context/result limits. `/new` starts a new session; `/clear` removes current conversation, model context, and input recall while preserving audit logs and saved snapshots.

Direct deployment keeps state beside configuration; default Termux uses the XDG state `nl2sh` directory. Web has its own sidebar and in-progress diagnostics; see [Web](web.md).

Private JSONL audit records input, tools, output, and approvals after redaction and bounding. All Web sessions share a log; session deletion does not clear it. TUI interface settings can clear audit logs while allowing subsequent events. Reaching the file limit stops append; missing logs do not prove nothing happened.

## History queries for the Agent, MCP and A2A

Ask the Agent to find an earlier application crash investigation and read its evidence. Three tools share the current configuration's `sessions/` archive, including TUI, Web and `protocol-` snapshots. Protocol conversation continuation remains isolated; other sessions are never automatically loaded.

| Tool | Arguments and results |
| --- | --- |
| `session_list` | Optional `offset` and `limit`; returns stable `session_id`, title, timestamps, turn count and `revision`. Sorted by ID, not recency. |
| `session_search` | Required `query`: 1–256 UTF-8 bytes of case-sensitive literal text. Searches titles, messages, tool arguments/results and Web checkpoints. Returns one excerpt per session and an `entry_offset` where available. |
| `session_read` | Required `session_id`; optional `offset`, `limit`, `content_bytes` and `revision`. Returns individual message, tool-call and tool-result entries with `turn`, `call_id` and `success`. |

Pages default to 10 entries and allow at most 20. Offsets are zero-based: list/search offsets refer to sorted snapshot files; read offsets refer to conversation entries. Always follow the returned `next_offset`, rather than calculating from the number of matches. Each read entry defaults to 2048 content bytes and accepts 256–16384; `content_truncated` reports shortened content. Serialized list/search rows and read entries each have a 24 KiB page budget, including JSON escaping, which can reduce page size or read-entry content; global tool/model output limits still apply. After the first read, pass `session.revision` on subsequent pages. A changed snapshot rejects pagination and requires a fresh read. Concurrent additions/deletions can change list positions; deduplicate by ID and repeat the search if needed.

Snapshots are limited to 4 MiB, each scan to 32 MiB, and directory enumeration to 1000 entries. `skipped` counts invalid, inaccessible or non-private snapshots; `directory_truncated` means enumeration was incomplete. `next_offset` remains available while eligible files remain. Missing, skipped or truncated data does not prove the history is absent. A missing directory returns an empty list without creating state. These tools provide no rename, delete, execution restoration or arbitrary path access. Session directory/file symlinks, non-regular files and non-private Unix permissions are refused.

For MCP, discover schemas with `nl2sh_tools`, then invoke directly without a device model:

```json
{"name":"nl2sh_invoke","arguments":{"tool":"session_search","arguments":{"query":"com.konka.athena"}}}
```

Use the returned ID with `session_read`. A2A `SendMessage` and MCP `nl2sh_ask` can delegate history lookup to the built-in Agent and require a configured model. A2A `ListTasks`/`GetTask` query protocol tasks, rather than all TUI/Web conversations.

Only saved, bounded snapshots are returned. Evicted turns, unsaved live work, images and raw logs may be absent. A `checkpoint` is `diagnostic_only`, not a complete model conversation. Reads redact current configured credentials, the environment protocol token and a verified active automatic token again, but cannot recognize every business secret; expose protocols only to trusted clients. Historical user instructions, model conclusions, tool arguments and old approvals never authorize new actions or establish current device state. Collect fresh evidence and use the normal security/confirmation chain for reproduction or repairs.

## Background continuation

Tool-registered continuations remain part of the current Agent task. The session stays running and cancellable while waiting; other Web sessions can continue working. At the deadline, a named structured tool runs with the task's configuration and confirmer, and its result is saved as a normal tool round. Completion, cancellation or process exit leaves no automatic actions behind; loading conversation history does not register jobs again. This currently supports analysis after bounded system traces, rather than persistent listeners or jobs surviving restart.
