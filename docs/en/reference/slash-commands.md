# Slash commands

Every TUI input starting with `/` after leading whitespace is local and excluded from the LLM. Unknown commands display hints only, including suggestions without automatic execution. Type `/`, select with Up/Down, and complete with Enter.

| Command | Purpose |
| --- | --- |
| `/help` | Local help |
| `/new` | New blank session, preserving snapshots/audit |
| `/clear` | Clear current conversation, context, and input recall |
| `/config`, `/setting` | Unified settings |
| `/permission` | Inspect process ordinary-mutation grants |
| `/permission allow`, `/permission ask` | Enable / disable that memory-only grant |
| `/tailcat` | Model-free Web / ADB dialog with progress and copyable peer commands |
| `/balance` | Read supported provider balances, kept in memory |
| `/sessions` | Recent sessions |
| `/sessions resume NAME` | Restore |
| `/sessions rename OLD NEW` | Rename |
| `/sessions delete NAME` | Delete snapshot |
| `/update` | Check updates; package builds suggest pkg upgrade |
| `/shell` | Direct interactive shell; exit / Ctrl+D returns |
| `/exit` | Quit safely |

`/provider`, `/model`, `/models`, and `/proxy` were removed in favor of `/config`; historical changelog mentions do not indicate current availability.

`!command` is a local safe-execution prefix, not a slash command. `/shell` is a directly controlled system shell. Their context/audit scopes differ; see [TUI](../guide/tui.md).
