# Tailcat

The optional `tailcat` group defaults off. `tailcat_check` reads the configured binary version. After approval, `tailcat_install` downloads fixed official v0.7.0 for Android ARM64/ARMv7/x86_64, verifies pinned SHA-256, ELF architecture, and version, then atomically replaces `tailcat_binary_path` (default `/data/local/tmp/tailcat`). Failures preserve the old file. Other ABIs and offline use require a manually installed compatible binary at an absolute path.

| Tool | Purpose / confirmation |
| --- | --- |
| `tailcat_receive_stream` | One raw transfer into a new file; confirmation |
| `tailcat_receive` | File drop box in an existing directory; confirmation |
| `tailcat_send_file` | Defaults to `stream` for raw receivers; explicit `copy` targets drop boxes and requires external scp; strong confirmation |
| `tailcat_serve` | Share one local TCP port; strong confirmation |
| `tailcat_adb_pair` | Wireless debugging guidance, pairing information, and multi-port sharing; strong confirmation |
| `tailcat_status` | Read current process listener state |
| `tailcat_stop` | Stop a current process listener; confirmation |

Results return a connection address; share only with intended peers. Receiver/service jobs belong to the current TUI/Web process and terminate when the parent exits. One-shot bridge exposes install/check/send, without cross-request listener management. Disabling the group does not lower shell Tailcat command risks.

For live testing set `NL2SH_TAILCAT_TEST_BINARY` and run `cargo test --test tailcat_live_tests -- --ignored`. With Tailcat on host and connected device, set `TAILCAT_HOST_BIN`, `TAILCAT_DEVICE_BIN`, and `ADB_SERIAL` for `./test-tailcat-connected.sh`; it removes only its own temporary files.

x86_64 devices use the pinned official static Linux amd64 archive with the same checksum, ELF architecture, and version verification. Devices advertising only 32-bit x86 remain outside the automatic installation list.

## Enable and call the tools

Enable the group in configuration, then start a new task:

```toml
[tool_groups]
tailcat = true
```

These are tool names and JSON argument examples for Agent calls, rather than shell commands. Device paths are examples: the inbox directory and source file must exist; a raw-stream destination must be a new file with an existing parent directory. See the [tool catalog](../reference/tool-catalog.md) for full parameters.

| Tool | Argument example | Usage |
| --- | --- | --- |
| `tailcat_check` | `{}` | Check the configured executable and version; does not test connectivity |
| `tailcat_install` | `{}` | Install the pinned release after approval; install Tailcat separately on the peer |
| `tailcat_serve` | `{"port":9999}` | Share an existing TCP service and return `address=tc…` |
| `tailcat_receive_stream` | `{"path":"/data/local/tmp/incoming.bin"}` | Receive one raw stream into a new file |
| `tailcat_receive` | `{"directory":"/data/local/tmp/inbox"}` | Start a file drop box |
| `tailcat_send_file` | `{"path":"/data/local/tmp/report.txt","address":"tc…"}` | Send to a raw receiver by default; explicitly use `"mode":"copy"` for a drop box |
| `tailcat_status` | `{}` | Inspect the current process's managed listener address and state |
| `tailcat_stop` | `{}` | Stop that listener after approval; leaves the destination service, such as Web, running |

Each nl2sh process manages at most one Tailcat listener at a time. Call `tailcat_stop` before switching services or receiver modes. `tc…` is a placeholder; actual calls require the complete address returned by the receiver.

## Forward an existing service

For example, “forward 9999 with Tailcat” maps to `tailcat_serve({"port":9999})`. Tailcat forwards tunnel connections to the existing service at `localhost:9999`; it does not bind local port 9999 again. A running nl2sh Web listener is a valid destination. No port change, Web shutdown, or new `nc -l` listener is needed. If the executable is missing, approve `tailcat_install` and then retry sharing; check and serve tools do not install automatically. Sharing still requires strong confirmation. nl2sh Web has no login, so peers holding the Tailcat address may edit configuration or submit tasks. Share the address only with trusted peers.

### Access port 9999 from the peer

[Install Tailcat](https://github.com/tailscale/tailcat/blob/main/INSTALL.md) on the remote computer. After approving `tailcat_serve({"port":9999})` on the device, give the returned complete address to the peer. Run this in the peer's terminal, replacing `tcREPLACE_WITH_RETURNED_ADDRESS` with that address:

```sh
tailcat forward tcREPLACE_WITH_RETURNED_ADDRESS 9999
```

Keep it running and open `http://127.0.0.1:9999/` in the **remote computer's** browser, or run `curl http://127.0.0.1:9999/` in another terminal. Loopback refers to the computer running `forward`, not the Android device. If local port 9999 is occupied on that computer, choose another local port:

```sh
tailcat forward tcREPLACE_WITH_RETURNED_ADDRESS 19999:9999
```

Then open `http://127.0.0.1:19999/`. The left port is the peer computer's local listener; the right port is the shared service port on the device, which remains 9999. `forward` binds to `127.0.0.1` by default. Ctrl+C on the peer stops only local forwarding; call `tailcat_stop({})` on the device to stop sharing. For connection failures, check that the device service is running, obtain the current address with `tailcat_status({})`, and inspect DNS/network diagnostics on both sides. See [upstream usage](https://github.com/tailscale/tailcat#usage) and `tailcat forward --help` for syntax.

## File transfer: operations on both sides

Run the following shell commands on the peer, substituting the complete Tailcat address and local file names. The peer needs Tailcat. nl2sh defaults to the raw stream that does not depend on `scp`; only explicit `copy` mode requires a system `scp` executable on the sender, which stock Android shells normally lack.

### Send from the peer to the device

- **Raw stream**: approve `tailcat_receive_stream({"path":"/data/local/tmp/incoming.bin"})` on the device, then run `tailcat tcREPLACE_WITH_RETURNED_ADDRESS < ./source.bin` on the peer. Only contents are transferred, without a file name. The receiver exits after one transfer.
- **Drop box**: approve `tailcat_receive({"directory":"/data/local/tmp/inbox"})` on the device, then run `tailcat cp ./report.txt tcREPLACE_WITH_RETURNED_ADDRESS:` on the peer. The trailing colon is required. The default drop box saves each file under a new name with a UTC timestamp and random suffix, without overwriting existing files; it accepts individual files rather than directory trees. A drop box allows uploads rather than peer downloads or directory browsing. Call `tailcat_stop({})` when finished.

### Send from the device to the peer

- **Raw stream (default)**: on the peer, run `tailcat --key=new > ./received.bin` and give the printed address to the device. Call `tailcat_send_file({"path":"/data/local/tmp/report.txt","address":"tc…"})` on the device with strong confirmation; `"mode":"stream"` may still be supplied explicitly. Shell redirection may overwrite an existing peer file, so choose an appropriate new path.
- **Drop box**: prepare an existing inbox directory on the peer, run `tailcat --key=new recv ./inbox`, and give its address to the device. Call the same sending tool with `"mode":"copy"` and strong confirmation. The device needs `scp`; `tailcat_install` does not install it. Stop the peer's drop box with Ctrl+C when finished.

Raw streams and drop boxes use different protocols: `stream` / `copy` must match the receiver. Compare file sizes and SHA-256 hashes after transfer. Failures or interruptions may leave partial files; stopping a listener does not delete received files. Give addresses only to intended senders.

## Wireless ADB pairing: `tailcat_adb_pair`

This tool requires Android 11 / API 30+ and an nl2sh process running as Android shell/root UID. Ordinary Termux UID is unsupported. It belongs to the optional `tailcat` group, disabled by default; both phases require strong confirmation. Install Tailcat and Android SDK Platform Tools supporting `adb pair` separately on the peer.

### 1. Open the device pairing screen

Call `tailcat_adb_pair({"action":"setup"})`. The tool reads Developer options, Wireless debugging, and the connection port, then opens Settings after approval:

- If Developer options are disabled, it opens About phone/device and returns `needs_user_action`. Find Build number and tap seven times; enter any device credential yourself, then call `setup` again.
- Otherwise it attempts to navigate current English/Chinese Settings nodes, enable Wireless debugging, allow the current network, and open Pair device with pairing code. It does not select Always allow on this network or write settings through shell commands.
- Unsupported vendor screens, missing Wi-Fi, failed UI trees, or unverifiable targets return manual instructions rather than guessed coordinates. Settings changes already completed may remain.

`ready_to_share` returns the current `pairing_code`, `pairing_port`, and `connect_port`; **no ports are shared yet**. As requested, the code is sent to the model and displayed in the conversation. It may also be saved according to conversation history and audit settings. Share conversations containing the code and Tailcat address only with the intended peer. Keep the pairing dialog open.

### 2. Approve and share the ports

Call `tailcat_adb_pair({"action":"share"})` to share the current pairing and TLS connection ports. To include an existing Web service on 9999, explicitly call:

```json
{"action":"share","web_port":9999}
```

The approval preview lists actual device ports, peer-local mappings, and ADB access. After approval, the tool rechecks the code, address, and connection port, probes localhost services, and shares only these explicit ports through one managed Tailcat process. It rechecks pairing information after startup too; if it changed, it stops the newly created listener and requests a fresh call rather than returning a stale code. An existing managed listener prevents sharing: approve `tailcat_stop` separately first. It is never replaced automatically. Stopping a previous Web tunnel may interrupt your remote entry point, so retain local device access beforehand.

A successful `sharing` result returns the complete Tailcat address, current pairing code, and `peer_commands`. For example, if actual device pairing and connection ports are 37123 and 42817, a result including Web tells the peer to run:

```sh
tailcat forward tcREPLACE_WITH_RETURNED_ADDRESS 13701:37123 13702:42817 19999:9999
```

Keep it running and execute these commands in another terminal:

```sh
adb pair 127.0.0.1:13701
# Enter the current returned six-digit conversation code at the prompt
adb connect 127.0.0.1:13702
adb -s 127.0.0.1:13702 shell getprop ro.build.version.sdk
```

Web is available on the peer computer at `http://127.0.0.1:19999/`. Device ports above are examples: copy actual tool results. Restarting Wireless debugging/Wi-Fi or reopening the pairing dialog may change them. Providing commands **does not mean the peer is paired or connected**. mDNS discovery is not carried by this TCP tunnel, so run `connect` explicitly.

Default peer-local ports are 13701 for pairing, 13702 for connecting, and 19999 for optional Web. If occupied, set `local_pair_port`, `local_connect_port`, and `local_web_port` in the `share` arguments. Used local ports must be nonzero, distinct, and different from ADB server port 5037. `web_port` must differ from both ADB ports and have an accessible existing service.

Use `tailcat_status({})` to inspect sharing and approve `tailcat_stop({})` to stop the entire listener. Stopping the tunnel does not disable system Wireless debugging or revoke paired computers; turn it off or forget devices in Settings yourself. The listener belongs to the current TUI/Web process; this tool is unavailable in one-shot bridge processes.

## Wireless ADB feasibility test

An Android 15 / API 35 emulator with a 1080×2400 portrait display passed UI activation of Developer options and Wireless debugging, including reading the connection port and pairing dialog information. A direct Tailcat v0.7.0 invocation shared both actual ports. After the peer mapped them locally with `tailcat forward`, `adb pair`, `adb connect`, and tunneled `adb shell getprop ro.build.version.sdk` all succeeded. The peer needs Android SDK Platform Tools with support for `adb pair`.

This verifies that TCP forwarding can carry wireless ADB pairing and TLS connections. It does not validate the full nl2sh Agent/approval flow or simultaneous sharing of Web port 9999. `tailcat_serve` still accepts one `port`; the new `tailcat_adb_pair` shares pairing, connection, and optional Web ports through one managed listener. Emulator success does not establish identical Settings navigation across vendor devices; read the current port values.

Device-level verification of the new tool used the `tailcat_adb_pair_device_check` example on an Android 15/API 35 emulator with a 1080×2400 portrait display, invoking the same Tool Runtime directly. With Wireless debugging initially off, approved `setup` returned the current code. `share` exposed both ADB ports and a test HTTP service on 9999; the returned commands successfully paired, connected, executed shell, and accessed Web with HTTP 200. Rejected approvals performed no actions, an existing listener was not replaced, and the managed process was stopped afterward. This example does not call a model and requires `CONFIRM` for each approval. These results cover tool preparation, confirmation, and execution, rather than the full TUI/Web approval interfaces, real model planning, or vendor devices.

## DNS limitation on Android 8/9

The pinned Tailcat v0.7.0 may fail while fetching `https://tailcat.dev/derpmap.json` on Android 8/9 (API 26–28), with diagnostics such as:

```text
lookup tailcat.dev on [::1]:53: androiddns: bogus answer length 1131375981
```

### Protocol cause and diagnostic limits

Tailcat's pure Go Android DNS adapter calls the system resolver through `/dev/socket/dnsproxyd`. The raw DNS command `resnsend` is available from Android 10 (API 29); older systems reply with the text error `500 Command not recognized`. An implementation without the older-protocol fallback parses this text as a binary reply, interpreting the four bytes `Comm` as the length `1131375981`. This explains the reported length and is unrelated to a destination TCP port conflict. The [upstream DNS implementation](https://github.com/tailscale/tailscale/blob/main/feature/androiddns/androiddns.go) now detects text errors and falls back to `getaddrinfo`; see also [Tailcat #126](https://github.com/tailscale/tailcat/issues/126).

A successful version check establishes only that the executable can run, not that DNS bootstrap or forwarding works. `[::1]:53` is an address displayed by Go's resolver error; it does not establish that a DNS request was actually sent to IPv6 loopback. This adapter uses the system Unix socket. Compatibility and proxy advice appended to nl2sh tool results comes from nl2sh, rather than Tailcat's original stderr.

### Verified comparison

Emulator tests used the same x86_64 Tailcat v0.7.0 binary:

| System | Display | Result |
| --- | --- | --- |
| Android 8.1 / API 27 | 1080×2400 portrait | DNS bootstrap fails with the length error above, without producing a service address |
| Android 15 / API 35 | 1080×2400 portrait, 420 dpi | Produces a service address; forwarding to an existing test service on 9999 returns HTTP 200 and the expected body to the client |

The Android 15 comparison did not set `HTTPS_PROXY` for Tailcat. Tests covered bootstrap and actual request forwarding through a direct `tailcat --key=new serve 9999` invocation, without validating a complete model conversation, nl2sh's approval flow, or every Android version. Display resolution did not cause this DNS failure.

### Recovery

Startup failures return bounded original diagnostics. Do not change the destination service port or stop its existing service in response to this error. Set a device-reachable `HTTPS_PROXY` before starting nl2sh (and `HTTP_PROXY` if needed) to resolve Tailcat HTTPS bootstrap traffic through a proxy. Configuration proxy fields control nl2sh installation downloads; they are not automatically converted to Tailcat child-process environment variables.

Without a usable proxy, supply a compatible binary containing the older-Android DNS fallback described above and configure `tailcat_binary_path`. An upstream fix does not mean the pinned v0.7.0 already includes it; reinstalling that same version cannot be assumed to fix the issue. Installation and sharing still require their normal approvals.

When Tailcat tools are enabled, the Agent prefers the built-in check, install, and forwarding tools. Installation still requires approval; an existing service listener is the forwarding destination.
