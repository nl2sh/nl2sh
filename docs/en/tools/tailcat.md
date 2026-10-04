# Tailcat

The optional `tailcat` group defaults off. `tailcat_check` reads the configured binary version. After approval, `tailcat_install` downloads fixed official v0.7.0 for Android ARM64/ARMv7/x86_64, verifies pinned SHA-256, ELF architecture, and version, then atomically replaces `tailcat_binary_path` (default `/data/local/tmp/tailcat`). Failures preserve the old file. Other ABIs and offline use require a manually installed compatible binary at an absolute path.

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

x86_64 devices use the pinned official static Linux amd64 archive with the same checksum, ELF architecture, and version verification. Devices advertising only 32-bit x86 remain outside the automatic installation list.

## Forward an existing service

For example, “forward 9999 with Tailcat” maps to `tailcat_serve({"port":9999})`. Tailcat forwards tunnel connections to the existing service at `localhost:9999`; it does not bind local port 9999 again. A running nl2sh Web listener is a valid destination. No port change, Web shutdown, or new `nc -l` listener is needed. If the executable is missing, approve `tailcat_install` and then retry sharing; check and serve tools do not install automatically. Sharing still requires strong confirmation. nl2sh Web has no login, so peers holding the Tailcat address may edit configuration or submit tasks. Share the address only with trusted peers.

## DNS limitation on Android 8/9

Pinned Tailcat v0.7.0 may report `androiddns: bogus answer length` on Android API 26–28 due to differences in the system DNS protocol. A successful version check does not establish network startup capability. Startup failures return bounded original diagnostics; do not change the destination service port in response to this error. Set a device-reachable `HTTPS_PROXY` before starting nl2sh (and `HTTP_PROXY` if needed) to resolve Tailcat HTTPS bootstrap traffic through a proxy. Configuration proxy fields control nl2sh installation downloads; they are not automatically converted to Tailcat child-process environment variables. Without a usable proxy, supply a compatible binary containing the [upstream older-Android DNS fix](https://github.com/tailscale/tailcat/issues/126) and configure `tailcat_binary_path`. Installation and sharing still require their normal approvals.

When Tailcat tools are enabled, the Agent prefers the built-in check, install, and forwarding tools. Installation still requires approval; an existing service listener is the forwarding destination.
