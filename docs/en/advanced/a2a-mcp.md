# Device-native MCP / A2A

MCP and A2A are built into the nl2sh Rust executable. External agents connect directly to the device, without Python, Docker, a host gateway, or runtime ADB. MCP calls Tool Runtime directly; A2A delegates tasks to the built-in Agent. Direct tools need no model; Agent delegation requires device model configuration. See the [protocol reference](../reference/a2a-mcp.md) for contracts and limits.

## Find connection methods

The TUI startup page, `nl2sh --config <config-path> service status` (including JSON `connections`), and the Web left vertical menu’s “MCP / A2A” button provide connection guidance. Active HTTP uses the actual advertised origin, including custom ports or HTTPS. Active stdio shows a local process without claiming HTTP availability. Stopped/unknown states show clearly labeled default loopback examples. The Web dialog refreshes discovery and copies client configuration, startup commands, and an A2A request body.

Discovery uses private `protocol/connection.json`, process start identity, and the exclusive lock. It reads no token and makes no requests to the advertised URL. Stale records do not report a stopped process as running. Discovery verifies only a process with the same UID and configuration, not remote reachability. TUI shows a startup snapshot; Web and CLI refresh the current state. None of these display entry points starts protocols automatically.

## Start device HTTP with one command

In a device shell/root terminal:

```sh
nl2sh protocol serve
```

This uses the default configuration path; add `--config /data/local/tmp/config.toml` for another configuration. Defaults are `0.0.0.0:8765` with HTTP allowed; TUI and Web are not started. Without `NL2SH_PROTOCOL_TOKEN`, the system secure random source generates a 64-character token. Startup prints MCP, A2A and public Agent Card URLs, the token, and MCP client configuration. Give this connection block to an external Agent, or set its Bearer credential/client `NL2SH_PROTOCOL_TOKEN`. Generated tokens change on every startup and are not saved in configuration or protocol connection records, or exposed through TUI, Web or status discovery. Startup output contains credentials; share it only with trusted callers.

The default advertised IPv4 comes from local route source selection, without sending probe packets or querying DNS. If that fails, active non-loopback IPv4 interfaces are enumerated. With no usable address, advertisement falls back to `127.0.0.1`, usable only locally. The actual bound port is used, including `--port 0`. IP selection occurs at startup; restart and update clients after network/IP changes.

`--host` controls binding: `0.0.0.0` means all IPv4 interfaces and is not a client destination. `--advertised-url` sets the client-facing HTTP(S) origin. The Agent Card derives its A2A URL from it; MCP guidance uses the same origin and Host/Origin validation restricts requests accordingly. Ordinary device networking needs no manual value. Override it for a different reachable interface, VPN, NAT mapping or HTTPS proxy:

```sh
nl2sh protocol serve --advertised-url https://agent.example.com
```

Detection does not guarantee remote reachability or configure port forwarding, NAT or HTTPS. HTTP sends tokens in plaintext; use an HTTPS reverse proxy for public access and preserve the advertised Host. Advertised URLs cannot contain path prefixes, credentials, queries or fragments, or use `0.0.0.0`/`::`. For local-only use, select `--host 127.0.0.1`. `--allow-insecure-http` defaults to true; `--allow-insecure-http=false` requires a loopback listener.

For a stable token across restarts, explicitly override it with 32–256 printable ASCII characters. Configured values are hidden in startup output; empty or invalid values reject startup:

```sh
export NL2SH_PROTOCOL_TOKEN='replace-with-a-fixed-random-token-of-32-to-256-characters'
nl2sh protocol serve
```

Discovery at `/.well-known/agent-card.json` is public; `/mcp` and `/a2a` require `Authorization: Bearer <token>`. Web has an independent port and access policy. Only one protocol process may use a state directory; HTTP and stdio cannot share it concurrently.

For background startup, keep output private and read the generated token and URLs from its log:

```sh
umask 077
nohup nl2sh protocol serve </dev/null >nl2sh-protocol.log 2>&1 &
```

Protect logs/configuration; never put tokens in public scripts or version control. Keep the device network and process available; startup does not establish background keepalive. SIGINT/SIGTERM cancels and waits for current operations to settle before shutdown. Cancellation does not undo existing effects.

## Connect an MCP client

HTTP clients use `http://DEVICE_IP:8765/mcp`; prefer HTTPS remotely. This is Streamable HTTP, not `/sse`. Set `NL2SH_PROTOCOL_TOKEN` in the client environment to the same token used by the device service.

Clients supporting these MCP configuration fields can use:

```toml
[mcp_servers.nl2sh]
url = "http://DEVICE_IP:8765/mcp"
bearer_token_env_var = "NL2SH_PROTOCOL_TOKEN"
tool_timeout_sec = 210
```

Call `nl2sh_tools` to discover current tool names and schemas, then `nl2sh_invoke`, for example:

```json
{"tool":"android.screen_dump","arguments":{}}
```

Use `nl2sh_read_screen` for text and PNG/JPEG MCP image blocks. Check `success`, `isError`, and fresh device evidence before claiming completion. `nl2sh_ask` runs the device Agent and returns a Task. Save `contextId` and pass it as `context_id` in subsequent calls. MCP execution does not go through A2A or add direct tool calls to Agent history.

When the local MCP client and nl2sh run on the same device or host, start stdio directly:

```sh
nl2sh --config /path/to/config.toml protocol stdio
```

stdio needs no Bearer token and uses the launching user's local privileges. stdout is MCP-only; diagnostics use stderr. This command cannot start a process on another device; remote clients use HTTP.

## Delegate through A2A

The Agent Card advertises A2A 1.0 JSON-RPC. Send ordinary user text to `/a2a`, without the old slash tool commands:

```json
{
  "jsonrpc": "2.0",
  "id": "request-1",
  "method": "SendMessage",
  "params": {
    "message": {
      "messageId": "unique-message-1",
      "role": "ROLE_USER",
      "parts": [{"text": "Read and summarize the device environment"}]
    },
    "configuration": {"returnImmediately": true}
  }
}
```

Send `Content-Type: application/json`, `Authorization: Bearer <token>`, and `A2A-Version: 1.0`. `returnImmediately` returns the submitted task; omitted or false waits for a terminal state. Use `GetTask` to poll, `ListTasks` to paginate by context/status, and `CancelTask` to request cancellation. Agent tasks sharing a `contextId` are serialized. Follow-ups create new messages/tasks without an old `taskId`. Use MCP for direct tools.

## Device-local approvals

Both direct tools and delegated Agent mutations wait for one-time device-local approval by default. In another interactive device terminal with the **same UID and configuration path**, run:

```sh
nl2sh --config /data/local/tmp/config.toml protocol approvals
nl2sh --config /data/local/tmp/config.toml protocol approve REQUEST_ID
```

Approval shows the exact action and local risk; dangerous actions additionally require the exact phrase bound to the request ID. Expiry after 120 seconds, rejection, and cancellation never approve execution. At most eight approvals may be pending; neither protocol has a remote approval endpoint.

Explicit `protocol_auto_approve = true` approves every risk level, including Dangerous/Critical, and is intended only for fully trusted callers. It defaults off. New protocol execution loads configuration; running tasks retain their snapshot. Parameter validation, risk assessment, command binding, and root capability checks still run. TUI/Web/ordinary CLI retain their own confirmers.

## Update and verify

Restart the protocol service after upgrading device nl2sh. The old `a2a_gateway/`, `nl2sh-a2a`, `nl2sh-a2a-mcp`, `bridge` command, and `bridge_auto_approve` are removed without compatibility entry points or automatic migration. Remove the old configuration field and configure the service as described above.

Run built-in protocol regressions:

```sh
cargo test --lib protocol::
cargo test --test protocol_stdio_tests
```

Full Android UI automation requires shell/root UID; ordinary Termux UIDs have different capabilities. Optional [Android Bridge](android-bridge.md) supports live node trees, Unicode input, and gestures, with nl2sh approvals and target revalidation still enforced.
