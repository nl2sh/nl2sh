# ADB / native Android

## Prepare the device

Install Android Platform Tools on the computer and put `adb` on PATH. Enable USB debugging in Developer options, connect the device, unlock it, and accept authorization.

```bash
adb devices -l
adb shell getprop ro.build.version.sdk
adb shell getprop ro.product.cpu.abi
adb shell getprop ro.product.cpu.abilist
```

The state must be `device`, with API 26 or newer. Use `getprop` for supported ABIs; `uname -m` alone cannot prove 64-bit Android support. Launchers support ARM64 and ARMv7. Set `ADB_SERIAL` for multiple devices.

Wireless Debugging uses `adb pair DEVICE_IP:PAIRING_PORT` followed by `adb connect DEVICE_IP:CONNECTION_PORT`; these ports can differ. Classic TCP ADB uses `adb tcpip 5555` while connected over USB, then a connection to the device IP on port 5555.

## Startup and daily use

Keep the complete extracted directory. Reconnect and run the same launcher later:

```bash
cd /path/to/nl2sh-android
./android-run-linux.sh
# Preselect a device
ADB_SERIAL=DEVICE_SERIAL ./android-run-linux.sh
# Background Web service only
./android-run-linux.sh --web-only
```

On Windows run `android-run-windows.bat` from the extracted directory. Preselect with `set "ADB_SERIAL=DEVICE_SERIAL"`; `--web-only` also works.

Launchers check the ABI and actual device SHA-256, pushing and verifying only changed binaries. Before startup they stop existing processes named exactly `nl2sh` using the actual launch privileges, first TERM and then remaining processes. This stops other nl2sh sessions; finish active tasks first.

The default path is `/data/local/tmp/nl2sh`; `ANDROID_DIR` changes it. Launchers try root adbd, verify UID, then fall back to `su`. Root is not needed for functions allowed by existing permissions. An unreadable private config causes explicit failure without relaxing `0600` permissions.

## Deploy configuration

Normal launches preserve device configuration. Deploy a host copy only by setting `NL2SH_CONFIG_SOURCE` explicitly:

```bash
NL2SH_CONFIG_SOURCE="$PWD/config.toml" ./android-run-linux.sh
```

Windows uses `set "NL2SH_CONFIG_SOURCE=%CD%\config.toml"`. Keep real API keys out of shared scripts.

To deploy one Android binary manually:

```bash
adb push nl2sh /data/local/tmp/nl2sh
adb shell chmod +x /data/local/tmp/nl2sh
adb shell -t /data/local/tmp/nl2sh
```

The TUI needs a real TTY. Use [Web-only](../guide/web.md) for background service. See [ADB troubleshooting](../troubleshooting/adb.md) for connection and ABI failures.
