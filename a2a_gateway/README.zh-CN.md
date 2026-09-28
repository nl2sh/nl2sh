# nl2sh A2A 网关

[English](README.md)

这个可选的主机侧模块通过 A2A 1.0 JSON-RPC 接口开放 Android 设备上的 nl2sh Device Runtime。外部 Agent 可直接调用已注册的设备工具，也可选择咨询 nl2sh 内置 Agent。Android 端仍只需部署一个 Rust 可执行文件；Python 和 A2A SDK 运行在网关主机上。网关使用指定的 `adb` 设备序列号，也支持无线 ADB `设备 IP:端口`，通过受限的 `nl2sh bridge` JSON 接口通信，不开放任意 adb shell 命令接口。设备端无需新增网络监听器。

## 启动

前提条件：Python 3.11 或更新版本、`adb`、已连接的 Android 设备，以及设备上兼容的 nl2sh 程序。直接调用 Tool Runtime 不需要设备端模型服务；只有可选的内置 Agent 咨询需要配置模型。在虚拟环境中安装网关：

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

公开的 Agent Card 位于 `http://127.0.0.1:8765/.well-known/agent-card.json`；JSON-RPC 接口位于 `/a2a`，请求必须携带 `Authorization: Bearer <token>`。其他机器可通过 HTTPS 反向代理访问，或在可信私有网络显式设置 `--host 0.0.0.0 --advertised-url http://网关IP:8765 --allow-insecure-http` 直接访问。普通 HTTP 会在网络上传输 Bearer 令牌，只应在受信任的 LAN/VPN 内使用。Agent Card 中的地址必须是客户端实际可达的网关地址。一个令牌代表一个受信任的使用者，不应共享给互不信任的客户端。nl2sh 原有 Web 界面使用独立的监听端口和访问策略。

## Docker Compose 与无线 ADB

在**网关主机**的 `a2a_gateway/` 目录执行；设备上先部署兼容的 nl2sh 可执行文件和配置，并启用无线 ADB。经典无线 ADB 可先通过 USB 执行 `adb tcpip 5555`，然后使用 `设备IP:5555`；Android 无线调试配对模式使用设备显示的**连接端口**作为 `NL2SH_DEVICE_SERIAL`，配对端口只用于 `adb pair`。容器必须能访问设备的 ADB 端口。

```sh
cp .env.example .env
chmod 600 .env
python3 -c 'import secrets; print(secrets.token_urlsafe(32))'
# 将生成的令牌填入 .env 的 NL2SH_A2A_TOKEN；填写设备 IP:端口。
# Hermes 在其他主机时，设置 NL2SH_GATEWAY_URL=http://网关IP:8765
# 并设置 NL2SH_GATEWAY_BIND=0.0.0.0。
docker compose up -d --build
docker compose logs -f gateway
```

对于需要配对码的无线调试，启动服务前可在同一目录运行 `docker compose run --rm --entrypoint adb gateway pair 设备IP:配对端口`，输入设备显示的配对码。Compose 将 ADB 密钥和 SQLite 任务库分别保存在命名卷中；重建容器后不需重新配对。网关在每次工具调用前执行 `adb connect 设备IP:连接端口`，连接失败时不会执行该次设备操作，也不会在回复丢失后自动重放写入。

`.env.example` 明确开启容器内的 HTTP 监听，但默认仅将宿主端口发布到 `127.0.0.1`。只有将 `NL2SH_GATEWAY_BIND` 改为 `0.0.0.0` 才能由其他机器按网关 IP 访问；此时应限制可信 LAN/VPN 的访问，或改用 HTTPS 反向代理。`NL2SH_GATEWAY_URL` 应与 Hermes 配置中的来源地址完全一致。可用 `curl http://网关IP:8765/.well-known/agent-card.json` 检查公开卡片；`/a2a` 仍需要令牌。Docker 和无线 ADB 不改变设备本地审批，需在设备的交互终端运行 `bridge approvals`/`bridge approve`。Docker 守护进程和 Compose 可用时，再执行上述容器命令；本机 Python 启动方式仍可使用相同的 `设备IP:端口` 作为 `--serial`。

客户端发送 `/inspect` 可获取固定的只读设备环境信息，发送 `/tools` 可获取可用工具目录，发送 `/invoke {"tool":"android.screen_dump","arguments":{}}` 可直接调用一次设备 Tool Runtime，发送普通问题则会启动可选的设备端 Agent 对话。直接调用跳过设备端 Agent 和模型请求。Agent 续问时使用相同的 A2A `contextId`。A2A 任务保存在网关的 SQLite 数据库中，Agent 对话回合保存在 nl2sh 的设备端私有会话目录中。确认成功前须检查直接调用结果的 `success` 或 Agent 结果的 `failed_tools`。

当前 UI 后端使用 Android shell/uiautomator，可选安装并启用 [Accessibility companion](../android-bridge/README.md) 以支持中文等 Unicode 输入、实时节点树、语义节点点击和 swipe/scroll 手势。`android.scroll` 可省略坐标，按显示尺寸默认向下滚动，也可设置 `direction: "up"`。没有 companion 时，`android.input_text` 仅接受可打印 ASCII。操作后可再调用 `android.screen_dump` 或 `nl2sh_read_screen` 检查实际屏幕内容。

直接 `/invoke` 调用遇到需要确认的操作时，最多等待 120 秒，让用户在另一个设备交互终端作出一次性决定。在设备上运行 `nl2sh --config /data/local/tmp/config.toml bridge approvals` 查看请求，再运行 `nl2sh --config /data/local/tmp/config.toml bridge approve REQUEST_ID`。审批命令展示完整操作和风险；危险操作要求再次输入精确短语。拒绝或超时均不执行。网关不暴露审批命令；旧的 Agent `/ask` 调用仍拒绝待确认操作。桥接调用会把本地 `unsafe`/`never` 设置至少提升到 `balanced`/`risk_only`，令牌持有者可请求设备账号有权读取的数据，应妥善保护令牌。

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

## Hermes 等外部 Agent 的直接工具调用

外部 Agent 可用 A2A 1.0 客户端连接经过鉴权的 `/a2a` 接口，或用 stdio MCP 客户端连接 `nl2sh-a2a-mcp`，并在适配器环境中设置 `NL2SH_A2A_URL`、`NL2SH_A2A_TOKEN`。MCP 适配器只需要 Python 与到网关的网络连接，不需要本机 `adb` 或设备端模型凭据。先用 `nl2sh_tools` 取得工具名称及参数 Schema，再通过 `nl2sh_invoke` 传入注册工具名称与结构化参数。视觉步骤可用 `nl2sh_read_screen`，可访问节点可用 `android.screen_dump`。外部 Agent 自己负责规划和生成语言；`nl2sh_ask` 会启动独立、可选的设备端内置 Agent，需设备端配置模型服务。

Hermes Agent 可在运行主机的 Python 虚拟环境中安装本网关包，再把下面的配置加入 `~/.hermes/config.yaml`。将命令路径替换为该虚拟环境中的 `nl2sh-a2a-mcp` 可执行文件，并在 Hermes 私有的 `~/.hermes/.env` 或进程环境设置 `NL2SH_A2A_TOKEN`。Hermes 与 A2A 网关同机时可用示例中的 loopback 地址；分机运行时首选 HTTPS。若网关按上面的可信私网 HTTP 方式发布，把 `NL2SH_A2A_URL` 改为 `http://网关IP:8765`，并在 Hermes MCP 服务的 `env` 中加入 `NL2SH_A2A_ALLOW_INSECURE_HTTP: "1"`；默认客户端会拒绝远程明文 HTTP。

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

Hermes 与网关分机、通过可信私网 IP 直连时，将上例的 `env` 改为：

```yaml
    env:
      NL2SH_A2A_URL: "http://192.168.1.10:8765"
      NL2SH_A2A_TOKEN: "${NL2SH_A2A_TOKEN}"
      NL2SH_A2A_ALLOW_INSECURE_HTTP: "1"
```

其中 `192.168.1.10` 替换为网关主机 IP，必须与网关 `.env` 的 `NL2SH_GATEWAY_URL` 一致；Android 设备 IP 只填写在网关的 `NL2SH_DEVICE_SERIAL` 中。

工具白名单让 Hermes 只看到直接调用路径，不加载可选的内置 Agent `nl2sh_ask`。210 秒 MCP 调用超时覆盖网关的 200 秒请求上限及设备端 120 秒审批窗口。修改配置后重启 Hermes 或重新加载 MCP 连接。以上字段和环境变量引用见 Hermes 官方 [MCP 配置参考](https://hermes-agent.nousresearch.com/docs/reference/mcp-config-reference)。

例如先以 `{}` 调用 `android.screen_dump`，确认出现目标节点后，再以 `{"text":"搜索"}` 调用 `android.tap_text`。第二步需设备本地审批；操作后重新读取 UI 并检查直接结果的 `success`。这条流程不绑定具体 App，也不向外部 Agent 提供批准入口。

## 供 Codex 使用的 A2A→MCP 适配层

同一个 Python 包还会安装 `nl2sh-a2a-mcp`，这是一个运行在本机、通过标准输入输出通信的 MCP 服务。它向网关发送带认证的 A2A 1.0 请求，本身不直接连接 Android 或 `adb`。它提供 `nl2sh_inspect`、`nl2sh_tools`、`nl2sh_invoke`、`nl2sh_read_screen`、`nl2sh_ask` 和 `nl2sh_get_task` 六个工具。`nl2sh_read_screen` 返回视觉模型可读取的 MCP 图像块。每次结果都包含 `task_id`、`context_id` 和任务 `state`；已完成的任务还包含设备返回结果。续问时将 `context_id` 传给 `nl2sh_ask`，确认成功前应检查直接调用的 `result.success` 或 Agent 的 `result.failed_tools`。

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
