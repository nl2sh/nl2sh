# Tailcat

可选 `tailcat` 组默认关闭。开启后 `tailcat_check` 只读检查配置程序，`tailcat_install` 在确认后按 Android ARM64/ARMv7/x86_64 ABI 下载固定官方 v0.7.0，校验预置 SHA-256、ELF 架构和版本，再原子替换 `tailcat_binary_path`（默认 `/data/local/tmp/tailcat`）。下载或校验失败保留旧文件；其他 ABI 与离线环境需手工安装兼容程序并指定绝对路径。

| 工具 | 用途 / 确认 |
| --- | --- |
| `tailcat_receive_stream` | 把一次原始流写入新文件，需确认 |
| `tailcat_receive` | 文件接收箱，写入现有目录，需确认 |
| `tailcat_send_file` | `stream` 对原始接收器；`copy` 对接收箱且发送端需 scp，强确认 |
| `tailcat_serve` | 共享一个本机 TCP 端口，强确认 |
| `tailcat_status` | 当前进程监听器状态，只读 |
| `tailcat_stop` | 停止当前进程监听器，需确认 |

输出返回连接地址，只发给预期连接方。接收与服务任务属于当前 TUI/Web 进程；父进程退出会终止子进程。一次性 bridge 暴露安装、检查、发送，不提供跨请求监听管理。关闭工具组不会降低 shell 中 Tailcat 命令的风险。

实测：宿主指定 `NL2SH_TAILCAT_TEST_BINARY` 后运行 `cargo test --test tailcat_live_tests -- --ignored`。连接设备与宿主都已安装 Tailcat 时，用 `TAILCAT_HOST_BIN`、`TAILCAT_DEVICE_BIN`、`ADB_SERIAL` 运行 `./test-tailcat-connected.sh`；脚本仅清理自己的临时文件。

x86_64 设备使用固定官方 Linux amd64 静态发行包，保留相同的摘要、ELF 架构和版本校验；只有 x86（32 位）的设备不在自动安装列表内。

## 转发已有服务

例如“用 Tailcat 转发 9999”对应 `tailcat_serve({"port":9999})`：Tailcat 将隧道连接转发到 `localhost:9999` 已有的服务，不在本地重新绑定 9999。nl2sh Web 正在监听该端口时，可直接作为转发目标；无需换端口、停止 Web 或用 `nc -l` 新建监听器。缺少 Tailcat 程序时，先用 `tailcat_install` 并确认安装，然后重试共享；检查或共享工具不会自动安装。共享仍需要强确认：nl2sh Web 无登录，获得 Tailcat 地址的接入者可能编辑配置或提交任务，只把地址提供给受信任连接方。

## Android 8/9 的 DNS 限制

固定的 Tailcat v0.7.0 在 Android API 26–28 上可能因系统 DNS 协议差异报 `androiddns: bogus answer length`；版本检查通过不代表网络启动可用。启动失败会返回有界的原始诊断，遇到此错误不要更改目标服务端口。可在启动 nl2sh 前设置设备可达的 `HTTPS_PROXY`（如同时使用 HTTP，请设置 `HTTP_PROXY`），让 Tailcat 的 HTTPS 引导流量经代理解析；配置中的代理字段用于 nl2sh 自身的安装下载，不会自动转换为 Tailcat 子进程的环境变量。没有可用代理时，需要手工提供包含[上游旧 Android DNS 修复](https://github.com/tailscale/tailcat/issues/126)的兼容程序，并配置 `tailcat_binary_path`。安装和共享的确认要求仍然适用。

Agent 在 Tailcat 工具启用时优先使用内置检查、安装及转发工具；安装仍需审批，已有服务监听端口是转发目标。
