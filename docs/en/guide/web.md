# Web UI

Interactive startup also listens on `0.0.0.0:9999`, choosing another available port if occupied. Use the actual URL in startup output. **There is no login: anyone reaching the page can view data, edit configuration, and submit tasks. Use a trusted network.** ADB forwarding does not disable LAN listening.

## Background startup without a terminal

```bash
adb shell 'cd /data/local/tmp && nohup ./nl2sh --web-only >nl2sh-web.log 2>&1 </dev/null &'
adb shell 'cat /data/local/tmp/nl2sh-web.log'
adb forward tcp:9999 tcp:9999
```

After confirming port 9999, open `http://127.0.0.1:9999/`. This mode initializes no TUI/PTY and can survive ADB disconnection. Launchers accept `--web-only` (`-WebOnly` in the PowerShell installer). Cancel active tasks before ending the actual service process. Host launchers stop old nl2sh instances before startup.

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
