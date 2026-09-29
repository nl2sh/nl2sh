# nl2sh A2A gateway

[简体中文](README.zh-CN.md)

This optional host-side module exposes the Android nl2sh Device Runtime through the A2A 1.0 JSON-RPC binding. External agents can call registered device tools directly or optionally consult nl2sh's built-in Agent. The Android deployment remains one Rust executable; Python and the A2A SDK run on the gateway host. The gateway uses an exact `adb` serial, including wireless ADB `device IP:port`, and a narrow `nl2sh bridge` JSON interface. It does not expose an arbitrary adb shell endpoint or add a device network listener.

## Start

Prerequisites: Python 3.11+, `adb`, a connected Android device, and a compatible nl2sh binary on the device. Direct Tool Runtime calls do not need a device model provider; only the optional built-in Agent consultation does. Install the gateway in a virtual environment:

```sh
python3 -m venv .venv
.venv/bin/pip install -e .
export NL2SH_A2A_TOKEN="$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')"
.venv/bin/nl2sh-a2a --serial DEVICE_SERIAL --binary /data/local/tmp/nl2sh \
  --config /data/local/tmp/config.toml --db ./a2a-tasks.db
```

On Windows, run the equivalent commands in PowerShell from `a2a_gateway/` (with Python and `adb` on `PATH`):

```powershell
py -3 -m venv .venv
& .\.venv\Scripts\python.exe -m pip install -e .
$env:NL2SH_A2A_TOKEN = & .\.venv\Scripts\python.exe -c 'import secrets; print(secrets.token_urlsafe(32))'
& .\.venv\Scripts\nl2sh-a2a.exe --serial DEVICE_SERIAL --binary /data/local/tmp/nl2sh `
  --config /data/local/tmp/config.toml --db .\a2a-tasks.db
```

The `/data/local/tmp/...` paths are on Android and stay the same on Windows. Keep the token available for the Codex-side setup below.

The public Agent Card is at `http://127.0.0.1:8765/.well-known/agent-card.json`; A2A JSON-RPC is at `/a2a`, and Streamable HTTP MCP is at `/mcp`. Both protocol endpoints require `Authorization: Bearer <token>`. For other machines, use an HTTPS reverse proxy or explicitly bind to the network with `--host 0.0.0.0 --advertised-url http://GATEWAY_IP:8765 --allow-insecure-http` on a trusted private network. Plain HTTP carries the Bearer token over the network; use it only on a trusted LAN/VPN. The advertised URL must be reachable from the client. The single token represents one trusted owner; do not share it among mutually untrusted clients. The existing nl2sh Web UI has its own listener and access policy.

## Docker Compose and wireless ADB

Run these commands in `a2a_gateway/` on the **gateway host**. First deploy a compatible nl2sh binary and configuration to the Android device and enable wireless ADB. For classic TCP ADB, use `adb tcpip 5555` over USB first, then set `device IP:5555`. Android Wireless Debugging pairing uses its displayed **connection port** for `NL2SH_DEVICE_SERIAL`; the pairing port is only for `adb pair`. The container must be able to reach the device's ADB port.

```sh
cp .env.example .env
chmod 600 .env
python3 -c 'import secrets; print(secrets.token_urlsafe(32))'
# Put the generated token in NL2SH_A2A_TOKEN and set the device IP:port in .env.
# For Hermes on another host, set NL2SH_GATEWAY_URL=http://GATEWAY_IP:8765
# and NL2SH_GATEWAY_BIND=0.0.0.0.
docker compose up -d --build
docker compose logs -f gateway
```

For Android Wireless Debugging with a pairing code, run `docker compose run --rm --entrypoint adb gateway pair DEVICE_IP:PAIRING_PORT` before starting the service, then enter the code shown on the device. Compose persists ADB keys and the SQLite task database in separate named volumes. The gateway runs `adb connect DEVICE_IP:CONNECTION_PORT` before every tool call; a failed connection prevents that call, and a lost reply never causes an automatic write replay.

The same `gateway` container serves A2A and HTTP MCP on port 8765; no second MCP container or external Docker network is needed. The example explicitly opts into HTTP inside the container, but publishes the host port only on `127.0.0.1` by default. Set `NL2SH_GATEWAY_BIND=0.0.0.0` only when the gateway host is reachable on a trusted LAN/VPN, or use an HTTPS reverse proxy. Set `NL2SH_GATEWAY_URL` to the exact origin configured in A2A clients. Check the public card with `curl http://GATEWAY_IP:8765/.well-known/agent-card.json`; `/a2a` and `/mcp` still require the token. Docker and wireless ADB preserve device-local approval: use `bridge approvals` and `bridge approve` in an interactive device terminal. The non-Docker Python launch also accepts a device `IP:port` as `--serial`.

Clients can send `/inspect` for fixed read-only environment facts, `/tools` for the available tool catalog, `/invoke {"tool":"android.screen_dump","arguments":{}}` for one direct Tool Runtime call, or a normal question for an optional device Agent turn. Direct calls skip the device Agent and its model request. Use the same A2A `contextId` for Agent follow-up messages. A2A tasks persist in the gateway SQLite database and Agent turns persist in nl2sh's private device session store. Check `success` in direct call results and `failed_tools` in Agent results before claiming that an action succeeded.

The UI backend uses Android shell/uiautomator and can optionally use the [Accessibility companion](../android-bridge/README.md) for Unicode input, live nodes, semantic node clicks, and swipe/scroll gestures. `android.scroll` can omit coordinates and defaults to a downward content scroll; use `direction: "up"` to reverse it. Without the companion, `android.input_text` accepts printable ASCII only. Verify the resulting UI with `android.screen_dump` or `nl2sh_read_screen` after an action.

By default, direct `/invoke` calls that need confirmation wait up to 120 seconds for a one-time decision in a separate interactive device terminal. On the device, run `nl2sh --config /data/local/tmp/config.toml bridge approvals`, then `nl2sh --config /data/local/tmp/config.toml bridge approve REQUEST_ID`. The approval command displays the exact action and risk; dangerous actions require a second exact phrase. A rejected or expired request does not execute. The gateway does not expose approval commands. Agent `/ask` calls reject operations needing confirmation by default. Set `bridge_auto_approve = true` in the device's `config.toml` to automatically approve every A2A/MCP `ask` and `invoke` operation, including dangerous and critical ones, without a local prompt. This applies when the device process loads the updated config on its next bridge call; it does not affect TUI, Web, or CLI calls. Tool argument validation, risk classification, command binding, and root capability checks still run. For bridge calls, local `unsafe` and `never` settings are raised to balanced/risk-only before classification. The gateway bearer token then grants unattended execution at the device process's privileges; give it only to fully trusted clients.

## Build, deploy, and continue a task

After a coding Agent changes nl2sh, run explicit host-side checkpoints:

```sh
python3 -m nl2sh_a2a.workflow prepare --repo /path/to/nl2sh \
  --serial DEVICE_SERIAL --state ./build-state.json --build-dir /path/to/build-target
python3 -m nl2sh_a2a.workflow deploy --state ./build-state.json
python3 -m nl2sh_a2a.workflow status --state ./build-state.json
```

On Windows, run these checkpoints in WSL from a Linux checkout with `adb`, Rust, Node.js, and the Android NDK available there. `prepare` invokes `cross-compile.sh`, and the checkpoint writer uses Unix file permissions; this workflow does not run in native PowerShell. Use Linux paths for `--repo`, `--state`, and `--build-dir`. The gateway itself may run in native Windows PowerShell as shown above.

`prepare` runs Rust formatting, checks and tests, detects the device ABI, and cross-compiles. `deploy` verifies the recorded binary digest and ABI, pushes only a separate `/data/local/tmp/nl2sh-a2a-*` candidate, and checks its version. It does not replace the device's existing `nl2sh`. Point `--binary` at the candidate and continue the same A2A context to validate the new function against the device. Keep the checkpoint file and task database private. The coding Agent, or the user, must explicitly invoke these host commands; they are not A2A skills available to external callers.

Run the gateway tests with `.venv/bin/python -m unittest discover -s tests -v` on Unix or `& .\.venv\Scripts\python.exe -m unittest discover -s tests -v` in Windows PowerShell.

## Direct tools for Hermes and other agents

Connect the external agent's A2A 1.0 client to `/a2a`, its HTTP MCP client to `/mcp`, or its stdio MCP client to `nl2sh-a2a-mcp` with `NL2SH_A2A_URL` and `NL2SH_A2A_TOKEN` in the adapter's environment. The stdio adapter needs Python and network access to the gateway, but no local `adb` or device model credentials. Use `nl2sh_tools` to read tool names and argument schemas, then `nl2sh_invoke` with a registered tool name and structured arguments. For a visual step, use `nl2sh_read_screen`; use `android.screen_dump` for accessible nodes. The external agent handles planning and language generation; `nl2sh_ask` starts the separate, optional built-in device Agent and requires a configured device model provider.

For Hermes Agent, put `NL2SH_A2A_TOKEN` in its private `~/.hermes/.env` and add the HTTP MCP connection to `~/.hermes/config.yaml`:

```yaml
mcp_servers:
  nl2sh_android:
    url: "http://127.0.0.1:8765/mcp"
    headers:
      Authorization: "Bearer ${NL2SH_A2A_TOKEN}"
    skip_preflight: true
    timeout: 210
    tools:
      include: [nl2sh_inspect, nl2sh_tools, nl2sh_invoke, nl2sh_read_screen]
      resources: false
      prompts: false
```

`skip_preflight` lets Hermes proceed to the MCP `initialize` POST when this endpoint's HEAD check does not return an MCP content type. Use `https://GATEWAY_HOST/mcp` through a reverse proxy when Hermes runs on another host. On a trusted private LAN/VPN, HTTP also works after setting `NL2SH_GATEWAY_BIND=0.0.0.0` and `NL2SH_GATEWAY_URL=http://GATEWAY_IP:8765` in the gateway's `.env`; replace `127.0.0.1` in the MCP URL with `GATEWAY_IP`. HTTP sends the MCP bearer token in cleartext. The URL is the Streamable HTTP `/mcp` endpoint, not legacy SSE `/sse`. A request without the token receives 401; successful tool discovery followed by `nl2sh_inspect` verifies the device path. The Compose health check probes the public Agent Card and does not prove MCP authentication or device access.

If Hermes runs in another container on the same host, attach it to this Compose project's default network and use `http://gateway:8765/mcp` with the same Authorization header. The gateway's published host port can remain bound to loopback. The HTTP MCP tools call A2A inside the gateway process, so the Agent Card's advertised origin need not be `gateway:8765` for this case.

Alternatively, install this gateway package in a Python virtual environment on the Hermes host and use the local stdio adapter. Replace the command path below with that environment's `nl2sh-a2a-mcp` executable. Set `NL2SH_A2A_TOKEN` in Hermes' private `~/.hermes/.env` or its process environment. The loopback URL works when Hermes and the A2A gateway run on the same host; use the gateway's advertised HTTPS origin when they run on different hosts. For the trusted private-network HTTP setup above, set `NL2SH_A2A_URL` to `http://GATEWAY_IP:8765` and add `NL2SH_A2A_ALLOW_INSECURE_HTTP: "1"` to this MCP server's `env`; the client rejects remote plain HTTP by default.

```yaml
mcp_servers:
  nl2sh_android:
    command: "/path/to/nl2sh-a2a-mcp"
    env:
      NL2SH_A2A_URL: "http://127.0.0.1:8765"
      NL2SH_A2A_TOKEN: "${NL2SH_A2A_TOKEN}"
    timeout: 210
    tools:
      include: [nl2sh_inspect, nl2sh_tools, nl2sh_invoke, nl2sh_read_screen]
      resources: false
      prompts: false
```

When Hermes uses the gateway's IP on a trusted private network, replace that example's `env` with:

```yaml
    env:
      NL2SH_A2A_URL: "http://192.168.1.10:8765"
      NL2SH_A2A_TOKEN: "${NL2SH_A2A_TOKEN}"
      NL2SH_A2A_ALLOW_INSECURE_HTTP: "1"
```

Replace `192.168.1.10` with the gateway host IP. This origin must match `NL2SH_GATEWAY_URL` in the gateway's `.env`; put the Android device IP only in the gateway's `NL2SH_DEVICE_SERIAL`.

The tool allowlist keeps the optional `nl2sh_ask` Agent path out of Hermes' direct-tool workflow. The 210-second MCP timeout covers the gateway's 200-second request limit and the device's 120-second approval window. Restart Hermes or reload its MCP connections after changing the config. Hermes' [MCP configuration reference](https://hermes-agent.nousresearch.com/docs/reference/mcp-config-reference) documents these keys and environment-variable references.

For example, invoke `android.screen_dump` with `{}` and then `android.tap_text` with `{"text":"Search"}` when that exact node is present. The second call waits for local device approval. After any action, read the UI again and check the direct result's `success` value. This flow is app-independent and does not give the external agent an approval channel.

## A2A to MCP adapter for Codex

The same Python package installs `nl2sh-a2a-mcp`, a local stdio MCP server. It sends authenticated A2A 1.0 requests to the gateway; it does not connect to Android or `adb` itself. It exposes `nl2sh_inspect`, `nl2sh_tools`, `nl2sh_invoke`, `nl2sh_read_screen`, `nl2sh_ask`, and `nl2sh_get_task`. `nl2sh_read_screen` returns an MCP image block for vision models. Each result includes `task_id`, `context_id`, and task `state`; completed tasks also include the device result. Reuse `context_id` with `nl2sh_ask` for follow-up questions, and check direct `result.success` or Agent `result.failed_tools` before claiming success.

### Install on the Codex machine

Run the following steps on the **Codex machine**. Start the A2A gateway on the machine connected to Android first. The Codex machine needs Python and network access to the gateway; it does not need `adb`, the Android SDK, or the NDK.

1. Get the repository's `a2a_gateway` directory. If these changes have not been pushed to Git yet, copy them from the machine holding the current workspace:

   ```sh
   mkdir -p "$HOME/nl2sh-a2a-gateway"
   rsync -a --exclude '.venv/' --exclude '__pycache__/' \
     USER@GATEWAY_HOST:/path/to/nl2sh/a2a_gateway/ "$HOME/nl2sh-a2a-gateway/"
   cd "$HOME/nl2sh-a2a-gateway"
   ```

   Replace `USER@GATEWAY_HOST` and the project path on that machine. After the changes are pushed, you can instead clone the repository and enter its `a2a_gateway/` directory. Only this directory is needed; do not copy another machine's `.venv/`. Run the following commands from this directory.

2. Check that Python is at least 3.11, create a virtual environment, and install the package:

   ```sh
   python3 --version
   python3 -m venv .venv
   .venv/bin/python -m pip install .
   test -x .venv/bin/nl2sh-a2a-mcp
   ```

   `pip install .` installs the dependencies and `nl2sh-a2a-mcp` command. If you are editing the adapter locally, use `.venv/bin/python -m pip install -e .` instead. If `python3 -m venv` is unavailable, install your operating system's Python venv component first.

   On Windows PowerShell, use the following commands instead for steps 1 and 2. `scp` is an option for unpublished changes; after the copy, create a fresh local virtual environment, never use one copied from another machine:

   ```powershell
   scp -r USER@GATEWAY_HOST:/path/to/nl2sh/a2a_gateway .\nl2sh-a2a-gateway
   cd .\nl2sh-a2a-gateway
   py -3 --version
   if (Test-Path .\.venv) { Remove-Item .\.venv -Recurse -Force }
   py -3 -m venv .venv
   & .\.venv\Scripts\python.exe -m pip install .
   Test-Path .\.venv\Scripts\nl2sh-a2a-mcp.exe
   ```

   Confirm the Python version is at least 3.11 and the last command prints `True`. The removal only discards a virtual environment copied into this new directory. For editable installs, use `& .\.venv\Scripts\python.exe -m pip install -e .`. Once the changes are pushed, `git clone` and entering `a2a_gateway` work on Windows as well.

   If installation reports `No matching distribution found for hatchling>=1.25` while using a package mirror, retry from this directory with the official PyPI index:

   ```powershell
   & .\.venv\Scripts\python.exe -m pip install --index-url https://pypi.org/simple .
   ```

   This overrides the configured index for this command, including its isolated build dependencies. It does not change your global pip configuration. If the command still uses the mirror, inspect the active settings with `& .\.venv\Scripts\python.exe -m pip config debug` and `Get-ChildItem Env:PIP*`.

3. Connect to the gateway. For an SSH tunnel, keep this command running in another terminal on the Codex machine:

   ```sh
   ssh -N -L 8765:127.0.0.1:8765 USER@GATEWAY_HOST
   ```

   Replace `USER@GATEWAY_HOST` with the gateway host's SSH address. A trusted HTTPS reverse proxy can be used instead of the tunnel.
   The same `ssh -N -L 8765:127.0.0.1:8765 USER@GATEWAY_HOST` command works in Windows PowerShell when OpenSSH Client is installed.

4. In the terminal that will **launch Codex**, set the gateway origin and the same bearer token used by the gateway, then check Agent Card access:

   ```sh
   export NL2SH_A2A_URL=http://127.0.0.1:8765
   read -r -s -p 'A2A token: ' NL2SH_A2A_TOKEN
   printf '\n'
   export NL2SH_A2A_TOKEN
   curl -fsS "$NL2SH_A2A_URL/.well-known/agent-card.json" | python3 -m json.tool
   ```

   For an HTTPS reverse proxy, set `NL2SH_A2A_URL` to the origin in the gateway's `--advertised-url`, such as `https://agent.example.com`. Use the same `NL2SH_A2A_TOKEN` that started the gateway. This input method keeps the token out of shell history.

   In Windows PowerShell, use this instead; the secure prompt does not echo the token or save it in command history:

   ```powershell
   $env:NL2SH_A2A_URL = 'http://127.0.0.1:8765'
   $secureToken = Read-Host 'A2A token' -AsSecureString
   $env:NL2SH_A2A_TOKEN = [System.Net.NetworkCredential]::new('', $secureToken).Password
   Remove-Variable secureToken
   (Invoke-RestMethod "$env:NL2SH_A2A_URL/.well-known/agent-card.json") | ConvertTo-Json -Depth 20
   ```

5. Add this stdio server to `~/.codex/config.toml` on the Codex machine. Replace `command` with the **absolute path** to the executable created in step 2:

```toml
[mcp_servers.nl2sh_a2a]
command = "/absolute/path/to/a2a_gateway/.venv/bin/nl2sh-a2a-mcp"
env_vars = ["NL2SH_A2A_URL", "NL2SH_A2A_TOKEN"]
```

   Do not use the literal `/absolute/path/to/a2a_gateway` placeholder; run `pwd` in the directory to find the actual path. `env_vars` passes the two variables from Codex's launch environment to its MCP child process. Do not put the bearer token value in the TOML file.

   On Windows, the file is at `$HOME\.codex\config.toml`. Use the absolute path to the `.exe` entry point, with forward slashes in TOML. For example:

   ```toml
   [mcp_servers.nl2sh_a2a]
   command = "C:/projects/nl2sh-a2a-gateway/.venv/Scripts/nl2sh-a2a-mcp.exe"
   env_vars = ["NL2SH_A2A_URL", "NL2SH_A2A_TOKEN"]
   ```

   Run `(Resolve-Path .\.venv\Scripts\nl2sh-a2a-mcp.exe).Path` to find the actual path; replace `C:/projects/nl2sh-a2a-gateway` in the example.

6. Start or restart Codex from the terminal in step 4. Run `codex mcp list` to check for `nl2sh_a2a`, then ask Codex to call `nl2sh_inspect`. A device result proves the MCP → A2A → Android path is connected; a tool listing alone does not prove that gateway authentication or the device connection works.

The adapter rejects non-HTTPS remote URLs and Agent Cards that redirect the bearer token to another origin. The device-side confirmation policy remains in force, so MCP tools cannot approve writes.
