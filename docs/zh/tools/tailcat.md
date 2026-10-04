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
