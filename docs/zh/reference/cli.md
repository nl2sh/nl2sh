# CLI 参数

无参数运行 TUI Agent；带 instruction 运行单次任务。`--mode command` 生成一条命令；`--dry-run` 对 Command 模式只展示。`--web-only` 只运行 HTTP 服务，不可与 instruction 或子命令组合。

```bash
nl2sh
nl2sh "查看内存"
nl2sh --mode command --dry-run "查看内存"
nl2sh --config /data/local/tmp/config.toml --no-pty --ascii
nl2sh --web-only
nl2sh update
nl2sh bridge inspect
nl2sh bridge tools
```

`--endpoint`、`--model`、`--api-type` 临时覆盖后统一校验；`--no-pty` 使用管道捕获，`--ascii` 改显示符号。bridge 的 ask/invoke 接收无 padding 的 base64url JSON，不提供任意 adb shell；见 [A2A/MCP](../advanced/a2a-mcp.md)。

## 从 clap 导出的帮助

以下含根命令与全部 bridge 子命令，不手工维护参数列表。

<!-- generated:start -->

```text
Natural Language to Shell for Android

Usage: nl2sh [OPTIONS] [INSTRUCTION] [COMMAND]

Commands:
  update   Check for and install the latest compatible GitHub Release
  service  Manage the native background Web service for this configuration
  bridge   Machine-readable, non-interactive interface for a trusted local bridge
  help     Print this message or the help of the given subcommand(s)

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
Manage the native background Web service for this configuration

Usage: service <COMMAND>

Commands:
  start    Start or return the healthy existing service
  stop     Stop only the service owned by this configuration
  restart  Stop and start the service, preserving configuration and sessions
  status   Report verified process identity and HTTP readiness
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

```text
Start or return the healthy existing service

Usage: start [OPTIONS]

Options:
      --json


      --port <PORT>
          [default: 9999]

      --port-strict


  -h, --help
          Print help
```

```text
Stop only the service owned by this configuration

Usage: stop [OPTIONS]

Options:
      --json


  -h, --help
          Print help
```

```text
Stop and start the service, preserving configuration and sessions

Usage: restart [OPTIONS]

Options:
      --json


      --port <PORT>
          [default: 9999]

      --port-strict


  -h, --help
          Print help
```

```text
Report verified process identity and HTTP readiness

Usage: status [OPTIONS]

Options:
      --json


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
