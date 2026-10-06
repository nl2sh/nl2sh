# ADB / 原生 Android

## 准备设备

在电脑安装 Android Platform Tools，让 `adb` 位于 PATH；在设备开发者选项开启 USB 调试，连接后解锁并接受授权。

```bash
adb devices -l
adb shell getprop ro.build.version.sdk
adb shell getprop ro.product.cpu.abi
adb shell getprop ro.product.cpu.abilist
```

设备状态必须为 `device`，API 至少 26。以 `getprop` 的 ABI 为准；`uname -m` 不能证明 Android 支持 64 位程序。启动器支持 ARM64、ARMv7 与 x86_64。多设备可设置 `ADB_SERIAL`。

Linux 和 Windows 启动器自动选择唯一可用设备，支持含空格的无线调试 mDNS 序列号；多个可用设备时提示选择，只有没有可用设备时才提示输入 IP。指定 `ADB_SERIAL` 时保留完整序列号。

无线调试先用 `adb pair DEVICE_IP:PAIRING_PORT` 配对，再 `adb connect DEVICE_IP:CONNECTION_PORT`；这两个端口可能不同。传统 TCP ADB 在 USB 连接时执行 `adb tcpip 5555`，再连接设备 IP 的 5555 端口。

## 启动和日常使用

保留完整解压目录，以后连接设备并运行同一启动器即可：

```bash
cd /path/to/nl2sh-android
./android-run-linux.sh
# 多设备时可预选
ADB_SERIAL=DEVICE_SERIAL ./android-run-linux.sh
# 只运行后台 Web 服务
./android-run-linux.sh --web-only
```

Windows 在解压目录运行 `android-run-windows.bat`，可用 `set "ADB_SERIAL=DEVICE_SERIAL"` 预选；`--web-only` 同样可用。

启动器检查 ABI、比较设备端实际 SHA-256，只有程序不同才推送并复核。启动前会以实际权限停止进程名精确为 `nl2sh` 的旧实例，先 TERM、再清理未退出进程。已有其他 nl2sh 会话会被停止，请先结束正在进行的任务。

默认位置是 `/data/local/tmp/nl2sh`，`ANDROID_DIR` 可改变目录。启动器尝试 root adbd 并核对 UID，失败后尝试 `su`；无 Root 仍可使用权限允许的功能。不可读的私有配置会明确失败，不会放宽 `0600` 权限。

## 配置部署

普通启动保留设备配置；只有显式设置 `NL2SH_CONFIG_SOURCE` 时才部署主机配置：

```bash
NL2SH_CONFIG_SOURCE="$PWD/config.toml" ./android-run-linux.sh
```

Windows 使用 `set "NL2SH_CONFIG_SOURCE=%CD%\config.toml"`。不要把真实 API Key 写进共享脚本。

手工部署单个 Android 二进制可运行：

```bash
adb push nl2sh /data/local/tmp/nl2sh
adb shell chmod +x /data/local/tmp/nl2sh
adb shell -t /data/local/tmp/nl2sh
```

TUI 需要真实 TTY；后台服务使用 [Web-only](../guide/web.md)。连接或 ABI 错误见 [ADB 排查](../troubleshooting/adb.md)。
