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
