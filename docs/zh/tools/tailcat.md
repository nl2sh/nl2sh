# Tailcat

可选 `tailcat` 组默认关闭。开启后 `tailcat_check` 只读检查配置程序，`tailcat_install` 在确认后按 Android ARM64/ARMv7 ABI 下载固定官方 v0.7.0，校验预置 SHA-256、ELF 架构和版本，再原子替换 `tailcat_binary_path`（默认 `/data/local/tmp/tailcat`）。下载或校验失败保留旧文件；其他 ABI 与离线环境需手工安装兼容程序并指定绝对路径。

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

nl2sh 的 x86_64 支持不扩展固定 Tailcat 自动下载列表；仅上报 x86_64/x86 ABI 的设备需手工提供兼容的 Tailcat 程序。
