# Web UI

Interactive startup also listens on `0.0.0.0:9999`, choosing another available port if occupied. Use the actual URL in startup output. **There is no login: anyone reaching the page can view data, edit configuration, and submit tasks. Use a trusted network.** ADB forwarding does not disable LAN listening.

## Background startup without a terminal

```bash
adb shell '/data/local/tmp/nl2sh --config /data/local/tmp/config.toml service start --json'
adb shell '/data/local/tmp/nl2sh --config /data/local/tmp/config.toml service status --json'
```

`state=ready` means process identity and the PID, version, and actual port returned by `/api/info` have been verified. Use the returned `port` in `adb forward tcp:9999 tcp:<port>`, then open `http://127.0.0.1:9999/`. Repeated `start` returns a healthy existing service. Use `service restart --json` or `service stop --json` explicitly. `--port 9999` sets the preferred port; `--port-strict` refuses an occupied port instead of selecting another; `--port 0` requests an ephemeral port.

Each configuration has an adjacent private `config.service/` directory with runtime and operation locks, `state.json`, `service.log`. Startup verifies readiness before returning. State records process start identity, executable device/inode, UID, version, and actual port. Stop sends a private shutdown token only to the verified service; it never kills processes by name. Shutdown cancels tasks, rejects pending approvals, and exits. Public status JSON omits the token. After binary replacement, status still reports the running version; `restart` launches the new version. Configuration and session files remain in place.

Background mode initializes no TUI/PTY and survives ADB disconnection. Launcher `--web-only` (`-WebOnly` in the PowerShell installer) uses the native lifecycle interface. `nl2sh --web-only` remains a foreground option for an external process supervisor and does not register a managed service. `start` refuses to replace an owned but unhealthy process; inspect its log and explicitly restart. Legacy launcher or Helper `nohup` processes are unregistered and must first be stopped through their original manager. The new interface does not adopt them.

## Interface and sessions

The left icon bar opens sessions, files, apps, tools, safety terminal, memory, and configuration. Adjacent panels minimize, resize by dragging, and remember widths; narrow screens use overlays. The sidebar can create sessions; the logo shows the program version.

Multiple Agent sessions run concurrently; switching does not stop background work, and approvals are independent. Completed first turns generate short titles asynchronously; lists show creation time. Running, pending-approval, or terminal-connected sessions cannot be deleted. Web and TUI keep separate conversations.

Every new Web task loads saved configuration; restart the TUI to load browser changes. See [provider setup](../getting-started/configure-provider.md). The read-only device overview needs no model. Advanced controls show budgets, tokens, Root state, and stage timings. Provider-returned reasoning is displayed separately and excluded from model history.

The memory panel searches, creates, edits, and deletes persistent notes, and can clear all notes after a second confirmation. These are explicit local Web management actions; model-initiated `agent_memory` mutations still pass through security classification and approval. Native Android deployments keep the SQLite database at `memory/agent-memory.sqlite3` beside the nl2sh executable; Termux uses `memory/` under the nl2sh state directory. The old `.nl2sh-agent-memory.json` file is not migrated automatically.

## Files and output

The read-only file panel lists types, sizes, and timestamps. Bounded image/video/audio/text/code previews are available; arrows insert `@paths`. Text supports highlighting and wrapping, and video supports HTTP Range. WAV headers fill playback parameters; raw PCM requires actual sample rate, channels, and format. Parameter decoding is limited to 32 MiB. Other audio playback depends on browser codecs.

Fenced code blocks in model replies have an icon button in the upper-right corner that copies the complete code; the icon briefly changes to a check mark on success. Tool calls have separate collapsible cards; F2 expands/collapses all results in the current session. Charts derive from tool results; compare data and sources with original evidence. The safety terminal uses the same classification and approval chain and separates stdout/stderr/exit status.

## Stop, restore, and export

Stopping cancels model waits, terminates and reaps command process groups, and stops other tools after their current operation. Redacted in-progress checkpoints become interrupted diagnostics after restart; approvals and execution never resume automatically. Failed tasks with recoverable original input offer retry, resubmitting input with fresh approvals.

Export ZIPs contain `conversation.json`, shared `nl2sh.log`, and scope notes. Logs may contain other sessions and device information; inspect before sharing. Known credentials are redacted, and output/log sizes are bounded.

![Web storage analysis](../../assets/web.png)

More: [safety approvals](security-confirmation.md), [sessions](sessions.md), and [network troubleshooting](../troubleshooting/network.md).

## Health and runtime information

`GET /healthz` returns `{"status":"ok"}` and checks HTTP responsiveness without depending on
sessions, a model provider or companions. `GET /api/info` returns protocol `1`, native version,
PID, actual listening port, Web server uptime in seconds, process ABI and `capabilities`.
The snapshot distinguishes Android userspace, the current UID's shell/UI authority, existing root,
Bridge protocol and independent Accessibility/IME readiness, an installed supported JADX helper,
JADX acquisition possible after approval, and a reported Tailcat version. These read-only endpoints
contain no configuration credentials. Use healthz instead of a session listing for health checks;
a root flag never means an action has been approved. Discovery does not download assets, change
system settings or prompt for su. Internal probes use ordinary-user pipe capture and a five-second timeout.

In `/api/tools`, `enabled` is the configured switch and `available` means runtime prerequisites
are present; unavailable tools remain visible in settings. Agent requests, `bridge tools` and direct
invocation filter by current capability. Development hosts do not advertise Android control or ART
decompilation; ordinary Termux UIDs do not advertise UI tools requiring shell/root. Static APK
reads and Tailcat checks remain available. The next task or info request rediscovers state after
service/installation changes; a running task's registry does not change midway through execution.

`/api/info` also returns `update_ownership`: standalone installations can self-update, Termux APT uses `pkg upgrade nl2sh`, and Helper installations update through Helper. The native program reads its adjacent Helper ownership marker with a size limit and verifies the installed binary checksum. Corrupt or mismatched markers block self-update with recovery guidance, preventing competing update sources.

The tools panel derives optional groups from the runtime catalog and separates the saved enable switch from discovered availability. Unavailable tools remain visible for configuration, and their example-prompt button is disabled; enabling does not acquire a capability or approve execution.

## Tailcat shortcut

Click the Tailcat icon and label at the top right. The dialog selects the actual Web port and ADB connection port (automatically detected, falling back to editable 5555) by default. Confirming the selection starts installation and sharing without further safety prompts. It shows download and execution status, then stays open with copyable peer commands. No LLM is used; existing tools handle validation and execution. See [Tailcat](../tools/tailcat.md#tailcat-quick).

Confirming or retrying automatically stops the previous managed Tailcat listener and starts sharing again. The dialog stays open after success or failure until explicitly closed.
