# Projects and integration boundaries

The [nl2sh organization](https://github.com/nl2sh) maintains distinct components. Clone and build each from its own root; the main repository does not build the Android companion projects from adjacent directories or submodules.

| Project | Role | Runtime and permissions | Guide |
| --- | --- | --- | --- |
| [nl2sh](https://github.com/nl2sh/nl2sh) | Agent, TUI/Web/CLI, Tool Runtime | Single Rust device binary; privileges follow caller/root policy | [Start](../getting-started/index.md) |
| `a2a_gateway/` in nl2sh | A2A 1.0, HTTP/stdio MCP | Python 3.11+ on host; adb targets one device; direct tools need no device model | [A2A/MCP](../advanced/a2a-mcp.md) |
| [android-bridge](https://github.com/nl2sh/android-bridge) | Accessibility, Unicode input and gestures | Optional installed API 26+ APK; provider/broadcasts require shell/root; services enabled manually | [Integration](../advanced/android-bridge.md) |
| [jadx-helper](https://github.com/nl2sh/jadx-helper) | One-class APK decompilation | API 26+ DEX JAR through app_process; not installed as an APK | [APK/JADX](../tools/apk-jadx.md) |
| [nl2sh-helper](https://github.com/nl2sh/nl2sh-helper) | Android ADB installer and browser launcher | API 26+ controller, TCP or target Android 11+ wireless pairing; deploys latest ARM64/ARMv7 release | Repository access required |

The ADB installer is not the Accessibility companion. It verifies release checksums, manages `/data/local/tmp/nl2sh-helper/`, and launches the target's Web UI on port 9999. It does not install the companion, supply model credentials, or serve A2A/MCP. Its source and bilingual guides currently require repository access; the public main installation paths remain documented under [installation](../getting-started/installation.md).

Companion/helper release cycles are independent. The native JADX default remains the historical `nl2sh/nl2sh` v1.0.4 asset with a fixed digest; it does not automatically select a new helper release. Android Bridge release APKs are unsigned and need signing before installation. Preserve package names, provider authority and DEX entrypoint when coordinating interfaces.

Default bridge approval differs from local TUI/Web: direct calls wait for device-terminal approval; Agent consultation rejects pending-confirmation actions. Explicit `bridge_auto_approve` permits unattended bridge operations at all risk levels. Risk assessment and capability checks still run. Full UI automation requires shell/root; ordinary Termux UIDs have a different permission boundary. The built-in Web UI currently has no login and binds all IPv4 interfaces; it does not inherit gateway Bearer authentication.

Report bugs in the component repository with version, Android API/ABI, caller UID, backend/transport and redacted evidence. Shared organization contribution guidance is maintained in [.github](https://github.com/nl2sh/.github). The organization's TUR fork serves packaging/upstream contribution work, not Agent runtime development.
