# Environment variables

Device runtime, host installation, and builds are separate processes. Set variables in the appropriate process; devices do not inherit host variables automatically. Never commit keys.

| Variable | Purpose |
| --- | --- |
| `NL2SH_CONFIG` | Default config path; CLI --config wins |
| `NL2SH_API_KEY` | Override main model key |
| `NL2SH_IMA_CLIENT_ID / NL2SH_IMA_API_KEY` | Independent ima credentials; enabled when complete |
| `NL2SH_JEV_API_KEY / NL2SH_JEV_ENDPOINT / NL2SH_JEV_MODEL` | Override audio judge settings |
| `NL2SH_JADX_ANDROID_HELPER_PATH` | Offline DEX helper path |
| `NL2SH_JADX_ANDROID_HELPER_URL / NL2SH_JADX_ANDROID_HELPER_SHA256` | HTTPS source and independent digest; required together |
| `NL2SH_JADX_CACHE_DIR` | Private helper cache root |
| `TERMUX_VERSION / PREFIX` | Termux detection and shell path |
| `HOME / XDG_CONFIG_HOME / XDG_STATE_HOME` | Termux config/state bases and ~ references |
| `NL2SH_WINDOWS_SCROLL` | Windows ADB wheel compatibility flag |
| `TERM / COLORTERM` | Terminal and color capabilities |
| `ADB_SERIAL / ANDROID_DIR / NL2SH_CONFIG_SOURCE` | Host launcher device, directory, explicit config deployment |
| `ANDROID_NDK_HOME / ANDROID_NDK_ROOT` | Host NDK build path |
| `ANDROID_API_LEVEL / RUST_TARGET` | Cross-build API (default 26) and target |
| `ANDROID_HOME / ANDROID_SDK_ROOT / JAVA_HOME` | SDK / JDK for optional Android modules |
| `NL2SH_PACKAGE_MANAGER_BUILD` | Build-time 1 disables in-app self-update |
| `ANDROID_TMP_DIR / TERMUX_SSH_LOCAL_PORT / TERMUX_SSH_REMOTE_PORT / TERMUX_TMUX_SESSION` | Termux SSH/tmux development deployment |
| `NL2SH_PROTOCOL_TOKEN` | Optional fixed HTTP MCP/A2A token, 32–256 printable ASCII characters; unset generates and prints a new token per startup |
| `NL2SH_APT_GPG_KEY_ID / TERMUX_APT_GPG_PRIVATE_KEY` | APT key ID / CI secret; private keys never enter source |
| `NL2SH_WEB_DIST` | Embedded asset directory set by build.rs; not a user setting |
| `NL2SH_TAILCAT_TEST_BINARY / NL2SH_TAILCAT_TEST_PROXY` | Explicit Tailcat live tests |
| `TAILCAT_HOST_BIN / TAILCAT_DEVICE_BIN` | Bidirectional transfer test executables |

See [device protocol service](../advanced/a2a-mcp.md) and [configuration priority](configuration.md). HTTP_PROXY / HTTPS_PROXY / ALL_PROXY for host downloads do not replace nl2sh device proxy configuration.
