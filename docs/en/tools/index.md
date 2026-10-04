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

APK/JADX and Tailcat default off. Enable groups or individual tools in Web or the Tools category of TUI `/config`; `tool_overrides` takes precedence over `tool_groups`. Disabled tools are absent from model definitions and direct calls. Changing a group switch clears its per-tool overrides. New Web tasks load configuration; saving TUI settings reloads configuration without a restart. Enabling a tool does not approve actions. ima needs separate credentials.

The [complete argument catalog](../reference/tool-catalog.md) is exported from code and includes optional tools. Configuration, capabilities, and entry point determine actual availability. One-shot bridge calls exclude long-lived Tailcat listener operations.
