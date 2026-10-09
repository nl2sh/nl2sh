# Device-native MCP / A2A

MCP and A2A are built into the nl2sh Rust executable. External agents connect directly to the device, without Python, Docker, a host gateway, or runtime ADB. MCP calls Tool Runtime directly; A2A delegates tasks to the built-in Agent. Direct tools need no model; Agent delegation requires device model configuration. See the [protocol reference](../reference/a2a-mcp.md) for contracts and limits.

## Find connection methods

The TUI startup page, `nl2sh --config <config-path> service status` (including JSON `connections`), and the Web left vertical menu’s “MCP / A2A” button provide connection guidance. Active HTTP uses the actual advertised origin, including custom ports or HTTPS. Active stdio shows a local process without claiming HTTP availability. Stopped/unknown states show clearly labeled default loopback examples. The Web dialog refreshes discovery and copies client configuration, startup commands, and an A2A request body.

Discovery uses private `protocol/connection.json`, process start identity, and the exclusive lock. It reads no token and makes no requests to the advertised URL. Stale records do not report a stopped process as running. Discovery verifies only a process with the same UID and configuration, not remote reachability. TUI shows a startup snapshot; Web and CLI refresh the current state. None of these display entry points starts protocols automatically.

## Start the device HTTP service

Set a random token containing at least 32 characters in a device shell/root terminal, then start:

```sh
export NL2SH_PROTOCOL_TOKEN='replace-with-a-random-token-of-32-to-256-characters'
nl2sh --config /data/local/tmp/config.toml protocol serve
```

The separate listener defaults to `127.0.0.1:8765`, without starting TUI or Web. Discovery at `/.well-known/agent-card.json` is public; `/mcp` and `/a2a` require `Authorization: Bearer <token>`. Web has a separate port and access policy; protocol tokens do not change Web access. Only one protocol process may use a configuration's state directory; do not run HTTP and stdio against the same state directory simultaneously.

Explicitly expose HTTP on a reachable trusted LAN/VPN:

```sh
nl2sh --config /data/local/tmp/config.toml protocol serve \
  --host 0.0.0.0 --port 8765 \
  --advertised-url http://DEVICE_IP:8765 --allow-insecure-http
```

Replace `DEVICE_IP` with the device address clients can reach. HTTP transmits tokens in plaintext; public access should use an HTTPS reverse proxy with the actual HTTPS origin in `--advertised-url`. Non-loopback binding still requires `--allow-insecure-http` because the native listener serves HTTP. Advertised URLs cannot contain path prefixes, credentials, queries, or fragments. Preserve the advertised Host at the proxy; the server validates Host and any supplied Origin.

You can start in the background from the device terminal where the token was set:

```sh
nohup nl2sh --config /data/local/tmp/config.toml protocol serve \
  --host 0.0.0.0 --advertised-url http://DEVICE_IP:8765 --allow-insecure-http \
  </dev/null > /data/local/tmp/nl2sh-protocol.log 2>&1 &
```

Protect logs and configuration directories; keep tokens out of public scripts and version control. The device must remain reachable and the process alive; startup does not establish automatic background keepalive. SIGINT/SIGTERM requests cancellation and waits for current operations to settle before shutdown. Cancellation does not roll back completed actions.

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
