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
