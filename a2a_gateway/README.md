# nl2sh A2A gateway

[简体中文](README.zh-CN.md)

This optional host-side module exposes an Android nl2sh Agent through the A2A 1.0 JSON-RPC binding. The Android deployment remains one Rust executable; Python and the A2A SDK run on the development host. The gateway uses an exact `adb` serial and a narrow `nl2sh bridge` JSON interface. It does not expose an arbitrary adb shell endpoint.

## Start

Prerequisites: Python 3.11+, `adb`, a connected Android device, a compatible nl2sh binary on the device, and a configured nl2sh model provider. Install the gateway in a virtual environment:

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

The public Agent Card is at `http://127.0.0.1:8765/.well-known/agent-card.json`; JSON-RPC is at `/a2a` and requires `Authorization: Bearer <token>`. For other machines, publish the loopback service through an HTTPS reverse proxy and pass its URL with `--advertised-url`. The single token represents one trusted owner; do not share it among mutually untrusted clients. The existing nl2sh Web UI has its own listener and access policy.

Clients can send `/inspect` for fixed read-only environment facts, `/tools` for the available tool catalog, or a normal question for a device Agent turn. Use the same A2A `contextId` for follow-up messages. A2A tasks persist in the gateway SQLite database and Agent turns persist in nl2sh's private device session store. An A2A task can complete with `failed_tools` in its result: that field reports tool denials or errors and must be checked before claiming a requested action succeeded.

The device bridge always rejects requests that need local confirmation, including modifications and dangerous commands. For bridge calls, local `unsafe` and `never` confirmation settings are raised to the balanced risk policy before classification. This retains `LLM → Security → Confirmation → Execution`; an A2A caller cannot approve a device operation or change the policy. A human can use the existing TUI or Web interface for operations requiring confirmation. The gateway limits request size and execution time, but the authorized owner can still ask the Agent to inspect data the device account can read, so protect the bearer token.

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

## A2A to MCP adapter for Codex

The same Python package installs `nl2sh-a2a-mcp`, a local stdio MCP server. It sends authenticated A2A 1.0 requests to the gateway; it does not connect to Android or `adb` itself. It exposes `nl2sh_inspect`, `nl2sh_tools`, `nl2sh_ask`, and `nl2sh_get_task`. Each result includes `task_id`, `context_id`, and task `state`; completed tasks also include the device result. Reuse `context_id` with `nl2sh_ask` for follow-up questions, and check `result.failed_tools` before claiming success.

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
