# Tool capabilities

An explicit registry gives models tool names and JSON schemas. Arguments are validated, actions prepared, risk assessed, and required approval obtained before execution. Results distinguish complete/partial/failed/timed-out evidence; model wording alone cannot prove success.

| Task | Documentation |
| --- | --- |
| Device diagnosis and UI | [Android](android.md) |
| Read, search, patch | [Filesystem](filesystem.md) |
| WAV / PCM features and quality | [Audio](audio.md) |
| ZIP / DEX / single-class decompilation | [APK / JADX](apk-jadx.md) |
| File transfers and port service | [Tailcat](tailcat.md) |
| Public HTTP / TLS | [Networking](network.md) |
| ima, notes, charts | [Knowledge and presentation](knowledge.md) |
| Inspect, change, or reset nl2sh settings | [Self configuration](configuration.md) |

APK/JADX and Tailcat default off. Enable groups or individual tools in Web or the Tools category of TUI `/config`; `tool_overrides` takes precedence over `tool_groups`. Disabled tools are absent from model definitions and direct calls. Changing a group switch clears its per-tool overrides. New Web tasks load configuration; saving TUI settings reloads configuration without a restart. Enabling a tool does not approve actions. ima needs separate credentials.

The [complete argument catalog](../reference/tool-catalog.md) is exported from code and includes optional tools. Configuration, capabilities, and entry point determine actual availability. The device protocol service supports long-lived Tailcat listener operations.

## Background shell capture

With `background: true`, `execute_shell_command` starts a command after the same local classification, reassessment after edits, and confirmation chain, then immediately returns `status: "started"` and a random `child_id`. This proves child creation, not successful command completion. Use it for continuous, noninteractive capture such as `logcat`. Background mode rejects `interactive: true`; setting `interactive_execute_timeout_secs: 0` still waits in the foreground.

```json
{"command":"logcat -v threadtime", "background":true, "background_timeout_secs":300}
```

Call `read_output` for actual status and independent stdout/stderr pages. Continue with the returned `stdout.next_offset` and `stderr.next_offset`:

```json
{"child_id":"<UUID returned by start>", "offset":0, "stderr_offset":0, "max_bytes":1024}
```

`offset` counts original stdout bytes; `stderr_offset` counts original stderr bytes. Neither counts characters or decoded text length. Invalid UTF-8 uses replacement characters, pages may split multibyte characters, and terminal controls are filtered. No new output does not prove exit: inspect `finished`, `exit_code`, `signal`, `timed_out`, and `error`. A successful read does not mean the command succeeded. Finish capture with `kill`, which remains a mutating operation requiring confirmation and accepts only managed `child_id` values, never arbitrary PIDs. Stop sends TERM, sends KILL to the original process group after 500 ms, and waits for cleanup. Repeated stop is supported for retained completed handles.

- Each process retains at most 16 handles. At capacity only the oldest completed entry is evicted; if all are running, new starts fail. Each stream retains its newest 1 MiB while pipes continue draining. A read returns 1–16384 raw bytes per stream. Evicted offsets produce `truncated: true`; `offset` and `available_offset` identify the retained starting point.
- `background_timeout_secs` defaults to 3600 and accepts 1–86400. Expiry cleans up automatically, independently of foreground execution timeouts. Background stdin is `/dev/null`; stdout and stderr use separate pipes. There is no PTY, terminal suspension, or live screen output. Programs may buffer differently without a TTY.
- Handles belong to the current nl2sh process and configuration identity and can be queried by later tasks in that domain. Task completion, cancellation, and client disconnection do not stop an already started capture. Normal nl2sh exit cleans up commands; restart restores neither handles nor output. Other processes/configurations cannot take over. Sudden host death, Android OOM, and vendor process reclamation do not guarantee survival or cleanup; this is not a persistent daemon service.
- `execute_user_mode` still applies, but background elevation through `su` from non-root nl2sh is rejected because reliable Root termination cannot be guaranteed. An already-root nl2sh can supervise children without lowering command risk or confirmation requirements. Running `tcpdump` also depends on UID, SELinux, and device permissions.
- With Android shell/root identity, background commands that may mutate state retain the UI resource lease until cleanup, Later actions in the same task and other UI/Shell tasks must reacquire the lease and may report busy. Read-only capture does not retain that lease after task completion. `read_output` and `kill` do not acquire it and remain usable for inspection and stopping. Do not use background capture for continuous UI automation.
- Commands must stay in their original process group. Do not add `nohup`, `setsid`, double forks, or daemonization. Natural shell exit also cleans up original-group descendants. Escaped descendants are outside supervision; pipes remaining open produce an `error` and must not be treated as complete capture.
