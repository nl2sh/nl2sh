# nl2sh A2A 网关

[English](README.md)

这个可选的主机侧模块通过 A2A 1.0 JSON-RPC 接口开放 Android 设备上的 nl2sh Agent。Android 端仍只需部署一个 Rust 可执行文件；Python 和 A2A SDK 运行在开发主机上。网关使用指定的 `adb` 设备序列号，通过受限的 `nl2sh bridge` JSON 接口通信，不开放任意 adb shell 命令接口。

## 启动

前提条件：Python 3.11 或更新版本、`adb`、已连接的 Android 设备、设备上兼容的 nl2sh 程序，以及已配置的 nl2sh 模型服务。在虚拟环境中安装网关：

```sh
python3 -m venv .venv
.venv/bin/pip install -e .
export NL2SH_A2A_TOKEN="$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')"
.venv/bin/nl2sh-a2a --serial DEVICE_SERIAL --binary /data/local/tmp/nl2sh \
  --config /data/local/tmp/config.toml --db ./a2a-tasks.db
```

Windows 用户在 `a2a_gateway/` 目录中使用 PowerShell 执行对应命令，确保 Python 和 `adb` 已加入 `PATH`：

```powershell
py -3 -m venv .venv
& .\.venv\Scripts\python.exe -m pip install -e .
$env:NL2SH_A2A_TOKEN = & .\.venv\Scripts\python.exe -c 'import secrets; print(secrets.token_urlsafe(32))'
& .\.venv\Scripts\nl2sh-a2a.exe --serial DEVICE_SERIAL --binary /data/local/tmp/nl2sh `
  --config /data/local/tmp/config.toml --db .\a2a-tasks.db
```

`/data/local/tmp/...` 是 Android 设备路径，在 Windows 上也保持不变。后续 Codex 端配置还需使用这个令牌。

公开的 Agent Card 位于 `http://127.0.0.1:8765/.well-known/agent-card.json`；JSON-RPC 接口位于 `/a2a`，请求必须携带 `Authorization: Bearer <token>`。其他机器需要访问时，可通过 HTTPS 反向代理发布这个仅监听本机的服务，并使用 `--advertised-url` 指定客户端可访问的地址。一个令牌代表一个受信任的使用者，不应共享给互不信任的客户端。nl2sh 原有 Web 界面使用独立的监听端口和访问策略。

客户端发送 `/inspect` 可获取固定的只读设备环境信息，发送 `/tools` 可获取可用工具目录，发送普通问题则会启动一次设备端 Agent 对话。续问时使用相同的 A2A `contextId`。A2A 任务保存在网关的 SQLite 数据库中，Agent 对话回合保存在 nl2sh 的设备端私有会话目录中。A2A 任务即使标记为完成，结果中的 `failed_tools` 仍可能列出被拒绝或执行失败的工具；确认操作成功前必须检查该字段。

设备桥接入口会拒绝所有需要本地确认的操作，包括修改和危险命令。通过桥接入口调用时，即使本地配置使用 `unsafe` 或 `never`，也会在安全分类前至少提升至 `balanced` 和 `risk_only` 策略。这样仍保持 `LLM → Security → Confirmation → Execution` 安全链；A2A 调用方不能批准设备操作或改变该策略。需要确认的操作可由用户通过原有 TUI 或 Web 界面处理。网关限制请求大小和执行时间，但持有令牌的调用方仍可要求 Agent 查看设备账号有权读取的数据，因此应妥善保护令牌。

## 构建、部署并继续任务

编码 Agent 修改 nl2sh 后，可显式执行以下主机侧检查点：

```sh
python3 -m nl2sh_a2a.workflow prepare --repo /path/to/nl2sh \
  --serial DEVICE_SERIAL --state ./build-state.json --build-dir /path/to/build-target
python3 -m nl2sh_a2a.workflow deploy --state ./build-state.json
python3 -m nl2sh_a2a.workflow status --state ./build-state.json
```

Windows 主机执行这些检查点时，应在 WSL 的 Linux 项目检出目录运行上述命令，并在 WSL 中准备好 `adb`、Rust、Node.js 和 Android NDK。`prepare` 会调用 `cross-compile.sh`，检查点写入也使用 Unix 文件权限，因此此工作流不能直接在原生 PowerShell 中运行。`--repo`、`--state` 和 `--build-dir` 应使用 Linux 路径；网关服务本身仍可按上面的示例运行在 Windows PowerShell 中。

`prepare` 执行 Rust 格式检查、编译检查和测试，探测设备 ABI 并交叉编译。`deploy` 核对构建产物摘要和设备 ABI，只把程序推送到独立的 `/data/local/tmp/nl2sh-a2a-*` 候选路径，并检查程序版本；不会替换设备上现有的 `nl2sh`。随后将网关的 `--binary` 指向候选程序，沿用同一个 A2A 上下文，在设备上验证新功能。检查点文件和任务数据库应保持私有。主机侧命令必须由编码 Agent 或用户显式调用，外部 A2A 客户端不能把它们当作网关技能调用。

运行网关测试：Unix 使用 `.venv/bin/python -m unittest discover -s tests -v`；Windows PowerShell 使用 `& .\.venv\Scripts\python.exe -m unittest discover -s tests -v`。

## 供 Codex 使用的 A2A→MCP 适配层

同一个 Python 包还会安装 `nl2sh-a2a-mcp`，这是一个运行在本机、通过标准输入输出通信的 MCP 服务。它向网关发送带认证的 A2A 1.0 请求，本身不直接连接 Android 或 `adb`。它提供 `nl2sh_inspect`、`nl2sh_tools`、`nl2sh_ask` 和 `nl2sh_get_task` 四个工具。每次结果都包含 `task_id`、`context_id` 和任务 `state`；已完成的任务还包含设备返回结果。续问时将 `context_id` 传给 `nl2sh_ask`，声称操作成功前应检查 `result.failed_tools`。

### 在 Codex 所在机器安装

以下命令在 **Codex 所在机器** 执行。A2A 网关应先在连接 Android 设备的机器上启动；Codex 所在机器只需 Python 和到网关的网络连接，不需要 `adb`、Android SDK 或 NDK。

1. 取得源码中的 `a2a_gateway` 目录。若这些改动尚未推送到 Git 仓库，可从保存当前工作区的机器复制：

   ```sh
   mkdir -p "$HOME/nl2sh-a2a-gateway"
   rsync -a --exclude '.venv/' --exclude '__pycache__/' \
     USER@GATEWAY_HOST:/path/to/nl2sh/a2a_gateway/ "$HOME/nl2sh-a2a-gateway/"
   cd "$HOME/nl2sh-a2a-gateway"
   ```

   替换 `USER@GATEWAY_HOST` 和网关主机上的项目路径。改动推送后，也可以用 `git clone` 检出项目，再进入其 `a2a_gateway/` 目录。只需这个目录，不要复制其他机器的 `.venv/`；以下命令均从该目录执行。

2. 确认 Python 至少为 3.11，创建该机器自己的虚拟环境并安装 Python 包：

   ```sh
   python3 --version
   python3 -m venv .venv
   .venv/bin/python -m pip install .
   test -x .venv/bin/nl2sh-a2a-mcp
   ```

   `pip install .` 会安装依赖和 `nl2sh-a2a-mcp` 命令。修改本地适配层源码并希望立即生效时，可改用 `.venv/bin/python -m pip install -e .`。若 `python3 -m venv` 不可用，先安装该操作系统提供的 Python venv 组件。

   Windows PowerShell 中，第 1、2 步改用下列命令。改动尚未推送时可通过 `scp` 复制；复制后需在本机重新创建虚拟环境，不能沿用另一台机器的环境：

   ```powershell
   scp -r USER@GATEWAY_HOST:/path/to/nl2sh/a2a_gateway .\nl2sh-a2a-gateway
   cd .\nl2sh-a2a-gateway
   py -3 --version
   if (Test-Path .\.venv) { Remove-Item .\.venv -Recurse -Force }
   py -3 -m venv .venv
   & .\.venv\Scripts\python.exe -m pip install .
   Test-Path .\.venv\Scripts\nl2sh-a2a-mcp.exe
   ```

   确认 Python 版本至少为 3.11，且最后一条命令输出 `True`。删除命令仅清理刚复制到这个新目录中的虚拟环境。若要以可编辑模式安装，使用 `& .\.venv\Scripts\python.exe -m pip install -e .`。改动推送后也可在 Windows 上 `git clone`，再进入 `a2a_gateway` 目录。

   如果安装时使用的软件包镜像报告 `No matching distribution found for hatchling>=1.25`，在当前目录指定官方 PyPI 源重试：

   ```powershell
   & .\.venv\Scripts\python.exe -m pip install --index-url https://pypi.org/simple .
   ```

   此参数只覆盖本次命令使用的软件包源，包含隔离构建环境所需的依赖，不会修改全局 pip 配置。如果仍显示镜像地址，可运行 `& .\.venv\Scripts\python.exe -m pip config debug` 和 `Get-ChildItem Env:PIP*` 检查生效的配置。

3. 建立到网关的连接。使用 SSH 隧道时，在 Codex 所在机器的另一个终端保持下列命令运行：

   ```sh
   ssh -N -L 8765:127.0.0.1:8765 USER@GATEWAY_HOST
   ```

   将 `USER@GATEWAY_HOST` 替换为网关主机的 SSH 地址。如果已有受信任的 HTTPS 反向代理，则无需 SSH 隧道。
   安装 OpenSSH Client 后，Windows PowerShell 也可使用同一条 `ssh -N -L 8765:127.0.0.1:8765 USER@GATEWAY_HOST` 命令。

4. 在**即将启动 Codex 的终端**设置网关地址和同一个 Bearer 令牌，并检查 Agent Card 可访问：

   ```sh
   export NL2SH_A2A_URL=http://127.0.0.1:8765
   read -r -s -p 'A2A token: ' NL2SH_A2A_TOKEN
   printf '\n'
   export NL2SH_A2A_TOKEN
   curl -fsS "$NL2SH_A2A_URL/.well-known/agent-card.json" | python3 -m json.tool
   ```

   使用 HTTPS 反向代理时，把 `NL2SH_A2A_URL` 改为网关 `--advertised-url` 所用的来源地址，例如 `https://agent.example.com`。令牌须与启动网关时的 `NL2SH_A2A_TOKEN` 相同；上述输入方式不会把令牌写入 shell 历史。

   Windows PowerShell 中改用以下命令；安全输入不会回显令牌或将其写入命令历史：

   ```powershell
   $env:NL2SH_A2A_URL = 'http://127.0.0.1:8765'
   $secureToken = Read-Host 'A2A token' -AsSecureString
   $env:NL2SH_A2A_TOKEN = [System.Net.NetworkCredential]::new('', $secureToken).Password
   Remove-Variable secureToken
   (Invoke-RestMethod "$env:NL2SH_A2A_URL/.well-known/agent-card.json") | ConvertTo-Json -Depth 20
   ```

5. 在该机器的 `~/.codex/config.toml` 中添加 MCP 服务，将 `command` 改为第 2 步生成的可执行文件**绝对路径**：

```toml
[mcp_servers.nl2sh_a2a]
command = "/absolute/path/to/a2a_gateway/.venv/bin/nl2sh-a2a-mcp"
env_vars = ["NL2SH_A2A_URL", "NL2SH_A2A_TOKEN"]
```

   `command` 示例中的 `/absolute/path/to/a2a_gateway` 不能原样使用；在该目录运行 `pwd` 可取得实际路径。`env_vars` 会把启动 Codex 时已有的两个变量传给本地 MCP 子进程，不要把令牌值写进 TOML 文件。

   Windows 上配置文件位于 `$HOME\.codex\config.toml`。`command` 应填入 `.exe` 入口的绝对路径，并在 TOML 中使用正斜杠。例如：

   ```toml
   [mcp_servers.nl2sh_a2a]
   command = "C:/projects/nl2sh-a2a-gateway/.venv/Scripts/nl2sh-a2a-mcp.exe"
   env_vars = ["NL2SH_A2A_URL", "NL2SH_A2A_TOKEN"]
   ```

   可运行 `(Resolve-Path .\.venv\Scripts\nl2sh-a2a-mcp.exe).Path` 查找实际路径，并替换示例中的 `C:/projects/nl2sh-a2a-gateway`。

6. 从第 4 步的终端启动或重启 Codex，运行 `codex mcp list` 确认出现 `nl2sh_a2a`，然后请 Codex 调用 `nl2sh_inspect`。能返回设备信息才表示 MCP → A2A → Android 链路实际连通；仅能看到工具名称还不足以证明网关鉴权或设备连接正常。

适配层会拒绝非 HTTPS 的远程地址，也会拒绝试图把令牌引向其他来源地址的 Agent Card。设备端确认策略继续生效，MCP 工具不能批准写入。
