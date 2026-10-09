# CLI arguments

No arguments opens the TUI Agent; an instruction runs one task. `--mode command` generates one command and `--dry-run` displays it only in Command mode. `--web-only` runs HTTP service only and cannot combine with an instruction or subcommand.

```bash
nl2sh
nl2sh "Show memory usage"
nl2sh --mode command --dry-run "Show memory usage"
nl2sh --config /data/local/tmp/config.toml --no-pty --ascii
nl2sh --web-only
nl2sh update
nl2sh protocol serve
nl2sh protocol stdio
nl2sh protocol approvals
```

`--endpoint`, `--model`, and `--api-type` are validated after temporary overrides. `--no-pty` uses pipeline capture; `--ascii` changes labels. Device MCP/A2A uses a dedicated protocol service and local approval; see [A2A/MCP](../advanced/a2a-mcp.md).

`nl2sh --config <config-path> service status` reports independent Web and MCP/A2A states and connection methods. JSON adds `connections`, including actual HTTP endpoints only when verified running and startup/stdio commands. Stopped endpoints are null; default addresses are examples, not active listeners. See [device MCP/A2A](../advanced/a2a-mcp.md).

`nl2sh protocol serve` starts with the default configuration: `0.0.0.0:8765`, HTTP allowed, automatically detected advertised IPv4, and a generated token/connection output when the environment token is absent. Use `--host 127.0.0.1` for local only, `--allow-insecure-http=false` to refuse network HTTP, or `--advertised-url` for a proxy/VPN override.


## Help exported from clap

This includes the root command and every protocol subcommand; argument lists are not maintained manually.

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
