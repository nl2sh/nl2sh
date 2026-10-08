# Tailcat

可选 `tailcat` 组默认关闭。开启后 `tailcat_check` 只读检查配置程序，`tailcat_install` 在确认后按 Android ARM64/ARMv7/x86_64 ABI 下载固定官方 v0.7.0，校验预置 SHA-256、ELF 架构和版本，再原子替换 `tailcat_binary_path`（默认 `/data/local/tmp/tailcat`）。下载或校验失败保留旧文件；其他 ABI 与离线环境需手工安装兼容程序并指定绝对路径。

| 工具 | 用途 / 确认 |
| --- | --- |
| `tailcat_receive_stream` | 把一次原始流写入新文件，需确认 |
| `tailcat_receive` | 文件接收箱，写入现有目录，需确认 |
| `tailcat_send_file` | 默认 `stream` 对原始接收器；显式 `copy` 对接收箱且发送端需外部 scp，强确认 |
| `tailcat_serve` | 共享一个或多个本机 TCP 端口，强确认 |
| `tailcat_adb_pair` | 无线调试引导、配对信息与多端口共享，强确认 |
| `tailcat_status` | 当前进程监听器状态，只读 |
| `tailcat_stop` | 停止当前进程监听器，需确认 |

输出返回连接地址，只发给预期连接方。接收与服务任务属于当前 TUI/Web 进程；父进程退出会终止子进程。一次性 bridge 暴露安装、检查、发送，不提供跨请求监听管理。关闭工具组不会降低 shell 中 Tailcat 命令的风险。

实测：宿主指定 `NL2SH_TAILCAT_TEST_BINARY` 后运行 `cargo test --test tailcat_live_tests -- --ignored`。连接设备与宿主都已安装 Tailcat 时，用 `TAILCAT_HOST_BIN`、`TAILCAT_DEVICE_BIN`、`ADB_SERIAL` 运行 `./test-tailcat-connected.sh`；脚本仅清理自己的临时文件。

x86_64 设备使用固定官方 Linux amd64 静态发行包，保留相同的摘要、ELF 架构和版本校验；只有 x86（32 位）的设备不在自动安装列表内。

## 无需模型的快捷入口 {#tailcat-quick}

点击 Web 右上角 **Tailcat**，或在 TUI 执行 `/tailcat`。默认勾选 Web 实际端口和 ADB 连接端口。自动检测 ADB 端口，失败时使用 **5555**；Web 可直接编辑，TUI 按 **P** 修改。W/A 切换选项，Enter 开始。

确定后直接复用现有工具安装并共享所选端口，无需 LLM 或额外安全确认。弹窗仅显示下载进度、执行状态及对端命令，完成或失败后保持打开，仅手动关闭。Web 点击复制图标；TUI 用 1–2 复制对应命令、C 复制全部（需要终端支持 OSC 52），R 返回选项重试，Esc 关闭。

对端保持 `tailcat forward` 运行；Web 映射到 `http://127.0.0.1:19999/`，ADB 映射到 `127.0.0.1:13702`。设备必须已启用对应 ADB 服务；无线 ADB 如需配对，应先单独完成配对。本入口不自动打开系统设置或配对窗口。

关闭弹窗不停止下载或共享，重新打开可查看结果。地址仅交给可信对端。安装仍核验固定版本、摘要和可执行文件；确定或重试时自动停止本进程管理的旧监听器，再共享所选端口；对端需要重新运行新的命令。临时启用快捷工具不改变持久配置，模型和通用工具调用保留原审批规则。

## 启用与工具调用

在配置中启用工具组，再启动新任务：

```toml
[tool_groups]
tailcat = true
```

以下是工具名与 JSON 参数示例，供 Agent 调用；不是在 shell 中直接执行的命令。设备路径只是示例：接收箱目录与发送文件必须已存在，原始流输出文件必须不存在且父目录已存在。完整参数见[工具目录](../reference/tool-catalog.md)。

| 工具 | 参数示例 | 使用方式 |
| --- | --- | --- |
| `tailcat_check` | `{}` | 检查配置的程序及版本；不验证网络连通性 |
| `tailcat_install` | `{}` | 确认后安装固定版本；对端需另外安装 Tailcat |
| `tailcat_serve` | `{"port":9999}` | 共享已有 TCP 服务，返回 `address=tc…` |
| `tailcat_receive_stream` | `{"path":"/data/local/tmp/incoming.bin"}` | 接收一次原始流到新文件 |
| `tailcat_receive` | `{"directory":"/data/local/tmp/inbox"}` | 启动文件接收箱 |
| `tailcat_send_file` | `{"path":"/data/local/tmp/report.txt","address":"tc…"}` | 默认向原始流接收器发送；接收箱须显式使用 `"mode":"copy"` |
| `tailcat_status` | `{}` | 查询当前进程受管理监听器的地址、状态 |
| `tailcat_stop` | `{}` | 确认后停止该监听器；不停止被共享的 Web 等目标服务 |

同一 nl2sh 进程同时只能管理一个 Tailcat 监听器；切换服务或接收模式前先调用 `tailcat_stop`。`tc…` 是占位符，实际调用须填写接收方返回的完整地址。

## 转发已有服务

例如“用 Tailcat 转发 9999”对应 `tailcat_serve({"port":9999})`：Tailcat 将隧道连接转发到 `localhost:9999` 已有的服务，不在本地重新绑定 9999。nl2sh Web 正在监听该端口时，可直接作为转发目标；无需换端口、停止 Web 或用 `nc -l` 新建监听器。缺少 Tailcat 程序时，先用 `tailcat_install` 并确认安装，然后重试共享；检查或共享工具不会自动安装。共享仍需要强确认：nl2sh Web 无登录，获得 Tailcat 地址的接入者可能编辑配置或提交任务，只把地址提供给受信任连接方。

### 对端访问 9999

远端电脑先[安装 Tailcat](https://github.com/tailscale/tailcat/blob/main/INSTALL.md)。设备端批准 `tailcat_serve({"port":9999})` 后，把返回的完整地址交给远端。在远端终端运行（将 `tcREPLACE_WITH_RETURNED_ADDRESS` 替换为该地址）：

```sh
tailcat forward tcREPLACE_WITH_RETURNED_ADDRESS 9999
```

保持此命令运行，在**远端电脑**浏览器打开 `http://127.0.0.1:9999/`，或另开终端执行 `curl http://127.0.0.1:9999/`。这里的回环地址属于运行 `forward` 的电脑，不是 Android 设备。若远端本地 9999 已被占用，使用不同本地端口：

```sh
tailcat forward tcREPLACE_WITH_RETURNED_ADDRESS 19999:9999
```

然后访问 `http://127.0.0.1:19999/`。映射左侧是远端电脑的本地监听端口，右侧是设备上已共享的服务端口；设备仍保持 9999。`forward` 默认仅监听 `127.0.0.1`。远端 Ctrl+C 只停止本地转发；设备端调用 `tailcat_stop({})` 才停止共享。连接失败时依次检查设备服务仍在运行、`tailcat_status({})` 返回的当前地址，以及双方 Tailcat 的 DNS/网络诊断。命令语法参见[上游说明](https://github.com/tailscale/tailcat#usage)与 `tailcat forward --help`。

## 文件传输：双方配套操作

以下 shell 命令在对端运行；替换完整 Tailcat 地址和本地文件名。对端须安装 Tailcat。nl2sh 默认采用不依赖 `scp` 的原始流；只有显式 `copy` 模式才要求发送端另有系统 `scp`，Android 原生 shell 通常不提供它。

### 对端发送到设备

- **原始流**：设备调用 `tailcat_receive_stream({"path":"/data/local/tmp/incoming.bin"})` 并批准后，对端运行 `tailcat tcREPLACE_WITH_RETURNED_ADDRESS < ./source.bin`。只传文件内容，不传文件名；一次传输结束后接收器退出。
- **接收箱**：设备调用 `tailcat_receive({"directory":"/data/local/tmp/inbox"})` 并批准后，对端运行 `tailcat cp ./report.txt tcREPLACE_WITH_RETURNED_ADDRESS:`。末尾冒号必需；默认接收箱会给文件加上 UTC 时间戳和随机后缀，以新名称保存，不覆盖已有文件；只接收单个文件，不接收目录树。接收箱提供写入能力，不是供对端下载或浏览文件的共享目录；完成后调用 `tailcat_stop({})`。

### 设备发送到对端

- **原始流（默认）**：对端先运行 `tailcat --key=new > ./received.bin`，把打印的地址交给设备；设备调用 `tailcat_send_file({"path":"/data/local/tmp/report.txt","address":"tc…"})` 并强确认；也可显式写 `"mode":"stream"`。shell 重定向可能覆盖对端已有文件，应自行选择合适的新路径。
- **接收箱**：对端先准备现有接收目录并运行 `tailcat --key=new recv ./inbox`，把地址交给设备；设备调用同一发送工具并使用 `"mode":"copy"`，经强确认发送。设备上需有 `scp`；`tailcat_install` 不安装它。对端完成后 Ctrl+C 停止接收箱。

原始流与接收箱使用不同协议，`stream` / `copy` 必须匹配接收端。传输成功后可比较双方文件大小与 SHA-256；失败或中断可能留下不完整文件，停止监听不会删除已接收文件。只把地址交给预期发送方。

## 无线 ADB 配对：`tailcat_adb_pair`

此工具用于 Android 11 / API 30+，要求 nl2sh 以 Android shell/root UID 运行；普通 Termux UID 不支持。它属于默认关闭的 `tailcat` 工具组，两个阶段均需强确认。对端需单独安装 Tailcat 与支持 `adb pair` 的 Android SDK Platform Tools。

### 1. 打开设备配对界面

调用 `tailcat_adb_pair({"action":"setup"})`。工具读取开发者选项、无线调试与连接端口状态，批准后打开 Settings：

- 开发者选项未开启时，打开“关于手机/设备”，返回 `needs_user_action`。用户找到版本号并点击七次，自行输入设备凭据（如要求），然后再次调用 `setup`。
- 已开启时，尝试按当前英文/中文 Settings 节点进入无线调试、开启开关、仅允许当前网络并打开“使用配对码配对设备”。工具不选择“始终允许此网络”，不通过 shell 改写设置。
- 不支持的厂商界面、缺少 Wi-Fi、UI 树失败或无法验证目标时，返回手动操作步骤；不会猜坐标。此前已完成的设置操作可能保留。

`ready_to_share` 返回当前 `pairing_code`、`pairing_port`、`connect_port`，**尚未共享端口**。按需求，配对码会发送给模型并显示在对话中，也可能随会话历史和审计配置保存；把包含该码及 Tailcat 地址的对话仅提供给预期对端。保持设备配对窗口打开。

### 2. 审批并共享端口

调用 `tailcat_adb_pair({"action":"share"})`，共享当前配对端口与 TLS 连接端口。若还要共享已有 Web 9999，显式调用：

```json
{"action":"share","web_port":9999}
```

审批预览列出实际设备端口、对端本地映射与 ADB 权限；批准后重新核对配对码、地址及连接端口，探测 localhost 服务，并用一个受管理 Tailcat 进程共享这些明确端口。启动完成还会复核配对信息；变化时停止本次新建的监听器并要求重新调用，不返回过期配对码。已有受管理监听器会拒绝新的共享：先单独批准 `tailcat_stop`，再共享；不会自动替换它。停止旧 Web 隧道可能中断当前远程入口，应预先保留设备本地操作通道。

成功返回 `sharing`、完整 Tailcat 地址、当前配对码及 `peer_commands`。例如实际设备配对端口为 37123、连接端口为 42817，包含 Web 时，对端执行返回的命令：

```sh
tailcat forward tcREPLACE_WITH_RETURNED_ADDRESS 13701:37123 13702:42817 19999:9999
```

保持运行，在另一个终端依次执行：

```sh
adb pair 127.0.0.1:13701
# 按提示输入对话中当前返回的六位配对码
adb connect 127.0.0.1:13702
adb -s 127.0.0.1:13702 shell getprop ro.build.version.sdk
```

Web 地址是对端电脑的 `http://127.0.0.1:19999/`。以上设备端口仅为示例，复制实际工具结果；无线调试/Wi-Fi 重启或重新打开配对窗口后可能变化。工具只提供命令，**不代表对端已配对或连接**；mDNS 发现不经此 TCP 隧道传输，需显式执行 `connect`。

默认对端本地端口为 13701（配对）、13702（连接）、19999（可选 Web）；占用时在 `share` 参数中设置 `local_pair_port`、`local_connect_port`、`local_web_port`。使用的端口须非零且互不相同，不能用 ADB 服务端口 5037；`web_port` 必须与两种 ADB 端口不同且已有服务可访问。

`tailcat_status({})` 查看共享状态，`tailcat_stop({})` 确认后停止整个监听器。关闭隧道不会关闭系统无线调试或撤销对端配对，需在 Settings 中自行关闭或忘记已配对设备。监听器依附当前 TUI/Web 进程；一次性 bridge 不提供此工具。

## 无线 ADB 的可行性验证

Android 15 / API 35、1080×2400 竖屏模拟器已通过界面启用开发者选项与无线调试，读取连接端口及配对窗口信息。直接运行 Tailcat v0.7.0 同时共享两个实际端口，对端通过 `tailcat forward` 映射到本地后，`adb pair`、`adb connect` 及隧道内 `adb shell getprop ro.build.version.sdk` 均成功。对端须使用支持 `adb pair` 的新版 Android SDK Platform Tools。

此结果验证 TCP 转发能够承载无线 ADB 配对与 TLS 连接；未验证完整 nl2sh Agent/审批流程，也未同时共享 Web 9999。`tailcat_serve` 仍只接受单个 `port`；新增的 `tailcat_adb_pair` 用一个受管理监听器共享配对、连接与可选 Web 端口。模拟器通过不代表所有厂商设备都支持相同设置导航；端口必须读取当前值。

新增工具的设备级验证使用 `tailcat_adb_pair_device_check` 示例，在 Android 15/API 35、1080×2400 竖屏模拟器直接调用同一 Tool Runtime：从无线调试关闭状态经批准完成 `setup`，返回当前配对码；`share` 同时共享双 ADB 及测试 HTTP 9999，按返回命令成功配对、连接、执行 shell，并访问 Web 得到 HTTP 200。拒绝审批不执行动作，已有监听器不会被替换，结束后停止受管进程。该示例不请求模型，逐项要求输入 `CONFIRM`；此结果覆盖工具准备、确认与执行路径，不代表完整 TUI/Web 审批界面、真实模型规划或厂商真机已验证。

## Android 8/9 的 DNS 限制

固定安装的 Tailcat v0.7.0 在 Android 8/9（API 26–28）上可能在获取 `https://tailcat.dev/derpmap.json` 时失败，典型诊断为：

```text
lookup tailcat.dev on [::1]:53: androiddns: bogus answer length 1131375981
```

### 协议原因与诊断边界

Tailcat 的纯 Go Android DNS 适配通过 `/dev/socket/dnsproxyd` 调用系统解析服务。原始 DNS 查询命令 `resnsend` 从 Android 10（API 29）才可用；旧系统返回文本错误 `500 Command not recognized`。缺少旧协议回退的实现把该文本按二进制应答解析，其中 `Comm` 的四个字节被当作长度，得到 `1131375981`。这与异常长度报错一致；不是目标 TCP 服务端口冲突。[上游 DNS 实现](https://github.com/tailscale/tailscale/blob/main/feature/androiddns/androiddns.go)已提供识别文本错误并回退到 `getaddrinfo` 的处理，相关报告见 [Tailcat #126](https://github.com/tailscale/tailcat/issues/126)。

版本检查通过只证明程序能启动，不代表 DNS 引导或端口转发可用。错误中的 `[::1]:53` 是 Go 解析错误所显示的地址，不能单凭它判断实际向 IPv6 回环地址发送了 DNS 请求；此适配使用的是系统 Unix socket。nl2sh 工具结果中附加的兼容性与代理建议来自 nl2sh，本身不属于 Tailcat 原始 stderr。

### 已验证的对照结果

使用同一 x86_64 Tailcat v0.7.0 二进制进行模拟器对照：

| 系统 | 屏幕 | 结果 |
| --- | --- | --- |
| Android 8.1 / API 27 | 1080×2400 竖屏 | DNS 引导报上述异常长度，无法生成服务地址 |
| Android 15 / API 35 | 1080×2400 竖屏，420 dpi | 成功生成服务地址；转发至已有 9999 测试服务，客户端收到 HTTP 200 和预期正文 |

Android 15 对照未给 Tailcat 设置 `HTTPS_PROXY`。验证覆盖直接运行 `tailcat --key=new serve 9999` 的引导与实际请求转发，不代表完整模型对话、nl2sh 审批流程或所有 Android 版本已通过；分辨率不是这次 DNS 失败的原因。

### 恢复方法

启动失败会返回有界的原始诊断；遇到此错误不要更改目标服务端口或停止已有服务。可在启动 nl2sh 前设置设备可达的 `HTTPS_PROXY`（如同时使用 HTTP，请设置 `HTTP_PROXY`），让 Tailcat 的 HTTPS 引导流量经代理解析。配置中的代理字段用于 nl2sh 自身的安装下载，不会自动转换为 Tailcat 子进程的环境变量。

没有可用代理时，手工提供包含上述旧 Android DNS 回退的兼容程序，并配置 `tailcat_binary_path`；上游修复不代表当前固定安装的 v0.7.0 已包含它，重复安装同一版本不能据此视为修复。安装和共享的确认要求仍然适用。

Agent 在 Tailcat 工具启用时优先使用内置检查、安装及转发工具；安装仍需审批，已有服务监听端口是转发目标。
