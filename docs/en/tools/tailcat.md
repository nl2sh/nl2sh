# Tailcat

The optional `tailcat` group defaults off. `tailcat_check` reads the configured binary version. After approval, `tailcat_install` downloads fixed official v0.7.0 for Android ARM64/ARMv7, verifies pinned SHA-256, ELF architecture, and version, then atomically replaces `tailcat_binary_path` (default `/data/local/tmp/tailcat`). Failures preserve the old file. Other ABIs and offline use require a manually installed compatible binary at an absolute path.

| Tool | Purpose / confirmation |
| --- | --- |
| `tailcat_receive_stream` | One raw transfer into a new file; confirmation |
| `tailcat_receive` | File drop box in an existing directory; confirmation |
| `tailcat_send_file` | `stream` for raw receivers; `copy` for drop boxes, requiring sender scp; strong confirmation |
| `tailcat_serve` | Share one local TCP port; strong confirmation |
| `tailcat_status` | Read current process listener state |
| `tailcat_stop` | Stop a current process listener; confirmation |

Results return a connection address; share only with intended peers. Receiver/service jobs belong to the current TUI/Web process and terminate when the parent exits. One-shot bridge exposes install/check/send, without cross-request listener management. Disabling the group does not lower shell Tailcat command risks.

For live testing set `NL2SH_TAILCAT_TEST_BINARY` and run `cargo test --test tailcat_live_tests -- --ignored`. With Tailcat on host and connected device, set `TAILCAT_HOST_BIN`, `TAILCAT_DEVICE_BIN`, and `ADB_SERIAL` for `./test-tailcat-connected.sh`; it removes only its own temporary files.
