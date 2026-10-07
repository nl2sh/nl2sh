# ADB 与 ABI 排查

| 提示 | 操作 |
| --- | --- |
| `unauthorized` | 解锁设备、接受 USB 调试授权，再查看 adb devices |
| `offline` | 检查线缆/无线连接，重新连接指定设备 |
| 多设备 | 设 ADB_SERIAL 或在启动菜单选择 |
| 无设备 | 确认 USB 调试、主机 adb、驱动；无线用连接端口 |
| TUI 无 TTY | 用 adb shell -t 或交互启动器；后台用 --web-only |

文件存在且可执行却报 `No such file or directory`，可能是 ABI 或 ELF interpreter 不匹配：32 位 Android 没有 `/system/bin/linker64`，无法运行 AArch64。核对：

```bash
adb shell getprop ro.product.cpu.abi
adb shell getprop ro.product.cpu.abilist
adb shell ls -l /data/local/tmp/nl2sh
adb shell file /data/local/tmp/nl2sh
```

ARM64 使用 `aarch64-linux-android`；32 位 ARM 使用 `armv7-linux-androideabi`；x86_64 使用 `x86_64-linux-android`。`file` 并非每台设备都有，可在主机检查下载程序。不要复制桌面 glibc 程序。

配置不可读时核对启动身份与文件属主，不放宽密钥文件权限。滚轮异常时用 PageUp/PageDown；Windows 使用官方 BAT 启动器的 alternate-scroll 模式。正常退出用 Ctrl+Q，避免直接关闭终端；异常后可在主机终端用 `reset` 恢复显示。

## 服务目录 UID 不匹配

`service directory ... must be ... owned by current UID ...` 表示服务目录属主与当前启动身份不同，或目录类型/权限不安全。例如先以 shell（UID 2000）启动，随后源码启动器执行 `adb root`，会以 root（UID 0）访问原来的 `config.service/`；即使权限是 `0700` 也会拒绝。

先用 `adb shell id` 和 `adb shell ls -ld /data/local/tmp/config.service` 核对身份。优先继续使用原 UID 管理服务。确需切换时，先以原 UID 执行 `service stop --json` 并确认返回 `state=stopped`，再把旧服务目录移到尚不存在的备份路径，重新启动以创建新 UID 的私有目录。不要对运行中的服务目录执行 `chown`，也不要放宽权限或删除锁文件。

以下示例仅适用于已经启用 root adbd、旧目录属于 shell，且设备的 `su --help` 支持 `su UID COMMAND ARG...` 的情况；其他 su 实现应按其语法切换身份：

```bash
adb shell su shell /data/local/tmp/nl2sh --config /data/local/tmp/config.toml service stop --json
# 确认上一步返回 state=stopped，且下面的备份路径不存在，再执行：
adb shell mv /data/local/tmp/config.service /data/local/tmp/config.service.shell-backup
./android-build-run.sh
```

只移动服务运行目录，配置、会话与密钥文件保持原位。源码启动器仍会优先选择 root，不能自动接管其他 UID 的服务。
