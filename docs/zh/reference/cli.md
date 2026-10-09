# CLI 参数

无参数运行 TUI Agent；带 instruction 运行单次任务。`--mode command` 生成一条命令；`--dry-run` 对 Command 模式只展示。`--web-only` 只运行 HTTP 服务，不可与 instruction 或子命令组合。

```bash
nl2sh
nl2sh "查看内存"
nl2sh --mode command --dry-run "查看内存"
nl2sh --config /data/local/tmp/config.toml --no-pty --ascii
nl2sh --web-only
nl2sh update
nl2sh protocol serve
nl2sh protocol stdio
nl2sh protocol approvals
```

`--endpoint`、`--model`、`--api-type` 临时覆盖后统一校验；`--no-pty` 使用管道捕获，`--ascii` 改显示符号。设备 MCP/A2A 使用独立协议服务与本地审批；见 [A2A/MCP](../advanced/a2a-mcp.md)。

`nl2sh --config <配置路径> service status` 显示 Web 与 MCP/A2A 的独立状态及连接方式；`--json` 新增 `connections`，包含实际 HTTP 端点（仅确认运行时）和启动/stdio 命令。未启动时端点为 null，默认地址只是示例，不代表已监听。详见 [设备 MCP/A2A](../advanced/a2a-mcp.md)。

`nl2sh protocol serve` 使用默认配置，一条命令启动：默认 `0.0.0.0:8765`、允许 HTTP、自动获取公告 IPv4，缺少环境令牌时自动生成并打印连接信息。可用 `--host 127.0.0.1` 限制本机、`--allow-insecure-http=false` 禁止网络 HTTP、`--advertised-url` 覆盖代理/VPN 地址。


## 从 clap 导出的帮助

以下含根命令与全部 protocol 子命令，不手工维护参数列表。

<!-- generated:start -->

```text
Natural Language to Shell for Android

Usage: nl2sh [OPTIONS] [INSTRUCTION] [COMMAND]

Commands:
  update    Check for and install the latest compatible GitHub Release
  service   Manage the native background Web service for this configuration
  protocol  Serve device-native MCP/A2A or manage local protocol approvals
  help      Print this message or the help of the given subcommand(s)

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
Serve device-native MCP/A2A or manage local protocol approvals

Usage: protocol <COMMAND>

Commands:
  serve      Serve authenticated HTTP MCP and A2A on the device
  stdio      Serve MCP over local stdin/stdout; never starts the TUI or Web UI
  approvals  List pending protocol approvals on this device
  approve    Approve or reject one request in an interactive local terminal
  help       Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

```text
Serve authenticated HTTP MCP and A2A on the device

Usage: serve [OPTIONS]

Options:
      --host <HOST>
          [default: 0.0.0.0]

      --port <PORT>
          [default: 8765]

      --advertised-url <ADVERTISED_URL>
          Override the advertised origin; defaults to the detected device IPv4 and bound port

      --allow-insecure-http[=<ALLOW_INSECURE_HTTP>]
          Allow plaintext HTTP (default); use =false to require a loopback listener

          [default: true]
          [possible values: true, false]

  -h, --help
          Print help
```

```text
Serve MCP over local stdin/stdout; never starts the TUI or Web UI

Usage: stdio

Options:
  -h, --help
          Print help
```

```text
List pending protocol approvals on this device

Usage: approvals

Options:
  -h, --help
          Print help
```

```text
Approve or reject one request in an interactive local terminal

Usage: approve <ID>

Arguments:
  <ID>


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
