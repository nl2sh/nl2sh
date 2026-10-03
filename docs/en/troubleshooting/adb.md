# ADB and ABI troubleshooting

| Symptom | Action |
| --- | --- |
| `unauthorized` | Unlock, accept debugging authorization, then inspect adb devices |
| `offline` | Check cable/wireless connectivity and reconnect the selected device |
| Multiple devices | Set ADB_SERIAL or choose from launcher menu |
| No device | Check USB debugging, host adb/drivers; wireless uses connection port |
| No TTY | Use adb shell -t or an interactive launcher; --web-only for background |

An existing executable reporting `No such file or directory` can mean ABI/interpreter mismatch: 32-bit Android lacks `/system/bin/linker64` and cannot run AArch64. Check:

```bash
adb shell getprop ro.product.cpu.abi
adb shell getprop ro.product.cpu.abilist
adb shell ls -l /data/local/tmp/nl2sh
adb shell file /data/local/tmp/nl2sh
```

ARM64 uses `aarch64-linux-android`; 32-bit ARM uses `armv7-linux-androideabi`. Not every device has `file`; inspect the downloaded binary on the host if needed. Never deploy desktop glibc executables.

For unreadable config, check identity/ownership instead of weakening key-file permissions. Use PageUp/PageDown for wheel trouble; official Windows BAT enables alternate-scroll compatibility. Quit normally with Ctrl+Q. A host `reset` can restore display after abnormal termination.
