# CLI arguments

No arguments opens the TUI Agent; an instruction runs one task. `--mode command` generates one command and `--dry-run` displays it only in Command mode. `--web-only` runs HTTP service only and cannot combine with an instruction or subcommand.

```bash
nl2sh
nl2sh "Show memory usage"
nl2sh --mode command --dry-run "Show memory usage"
nl2sh --config /data/local/tmp/config.toml --no-pty --ascii
nl2sh --web-only
nl2sh update
nl2sh bridge inspect
nl2sh bridge tools
```

`--endpoint`, `--model`, and `--api-type` are validated after temporary overrides. `--no-pty` uses pipeline capture; `--ascii` changes labels. Bridge ask/invoke accepts unpadded base64url JSON rather than an arbitrary adb shell endpoint; see [A2A/MCP](../advanced/a2a-mcp.md).

## Help exported from clap

This includes the root command and every bridge subcommand; argument lists are not maintained manually.

<!-- generated:start -->

```text
Natural Language to Shell for Android

Usage: nl2sh [OPTIONS] [INSTRUCTION] [COMMAND]

Commands:
  update  Check for and install the latest compatible GitHub Release
  bridge  Machine-readable, non-interactive interface for a trusted local bridge
  help    Print this message or the help of the given subcommand(s)

Arguments:
  [INSTRUCTION]
          

Options:
      --config <CONFIG>
          

      --mode <MODE>
          [default: agent]
          [possible values: agent, command]

      --endpoint <ENDPOINT>
          

      --model <MODEL>
          

      --api-type <API_TYPE>
          [possible values: auto, chat_completions, responses]

      --no-pty
          

      --ascii
          

      --dry-run
          

      --web-only
          Run only the browser UI without initializing a terminal interface

  -h, --help
          Print help

  -V, --version
          Print version
```

```text
Check for and install the latest compatible GitHub Release

Usage: update

Options:
  -h, --help
          Print help
```

```text
Machine-readable, non-interactive interface for a trusted local bridge

Usage: bridge <COMMAND>

Commands:
  inspect    Return bounded, read-only Android environment facts
  tools      Return the configured model-facing tool catalog
  ask        Run the Agent with a bounded base64url JSON request and stored history
  invoke     Invoke one registered tool directly with base64url {tool,arguments} JSON
  approvals  List pending direct-tool approvals on this device
  approve    Approve or reject one pending direct-tool request from an interactive terminal
  help       Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

```text
Return bounded, read-only Android environment facts

Usage: inspect

Options:
  -h, --help
          Print help
```

```text
Return the configured model-facing tool catalog

Usage: tools

Options:
  -h, --help
          Print help
```

```text
Run the Agent with a bounded base64url JSON request and stored history

Usage: ask --payload-base64 <PAYLOAD_BASE64>

Options:
      --payload-base64 <PAYLOAD_BASE64>
          Base64url without padding, containing {session,message} JSON

  -h, --help
          Print help
```

```text
Invoke one registered tool directly with base64url {tool,arguments} JSON

Usage: invoke --payload-base64 <PAYLOAD_BASE64>

Options:
      --payload-base64 <PAYLOAD_BASE64>
          Base64url without padding, containing {tool,arguments} JSON

  -h, --help
          Print help
```

```text
List pending direct-tool approvals on this device

Usage: approvals

Options:
  -h, --help
          Print help
```

```text
Approve or reject one pending direct-tool request from an interactive terminal

Usage: approve <ID>

Arguments:
  <ID>
          Request identifier printed by `bridge approvals`

Options:
  -h, --help
          Print help
```

```text
Print this message or the help of the given subcommand(s)

Usage: help [COMMAND]...

Arguments:
  [COMMAND]...
          Print help for the subcommand(s)
```

```text
Print this message or the help of the given subcommand(s)

Usage: help [COMMAND]...

Arguments:
  [COMMAND]...
          Print help for the subcommand(s)
```

<!-- generated:end -->
