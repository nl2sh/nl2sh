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

ARM64 uses `aarch64-linux-android`; 32-bit ARM uses `armv7-linux-androideabi`; x86_64 uses `x86_64-linux-android`. Not every device has `file`; inspect the downloaded binary on the host if needed. Never deploy desktop glibc executables.

For unreadable config, check identity/ownership instead of weakening key-file permissions. Use PageUp/PageDown for wheel trouble; official Windows BAT enables alternate-scroll compatibility. Quit normally with Ctrl+Q. A host `reset` can restore display after abnormal termination.

## Service directory UID mismatch

`service directory ... must be ... owned by current UID ...` means the directory belongs to a different startup identity, or its type/permissions are unsafe. For example, starting as shell (UID 2000), then running the source launcher with `adb root`, makes root (UID 0) access the existing `config.service/`. Even permissions of `0700` cannot resolve this ownership mismatch.

Check identities with `adb shell id` and `adb shell ls -ld /data/local/tmp/config.service`. Prefer managing the service with its original UID. To switch users, first run `service stop --json` as the original UID and confirm `state=stopped`, then move the old service directory to a backup path that does not already exist. Start again to create a private directory for the new UID. Do not chown an active service directory, relax its permissions, or delete lock files.

This example requires root adbd, a shell-owned old directory, and a device whose `su --help` supports `su UID COMMAND ARG...`. For other su implementations, use their documented identity-switching syntax:

```bash
adb shell su shell /data/local/tmp/nl2sh --config /data/local/tmp/config.toml service stop --json
# Confirm state=stopped above and that the backup path below does not exist:
adb shell mv /data/local/tmp/config.service /data/local/tmp/config.service.shell-backup
./android-build-run.sh
```

Move only the service runtime directory; leave configuration, sessions, and credentials in place. The source launcher still prefers root and cannot automatically take ownership of another UID's service.
