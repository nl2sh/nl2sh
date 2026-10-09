# Projects and integration boundaries

The [nl2sh organization](https://github.com/nl2sh) maintains distinct components. Clone and build each from its own root; the main repository does not build the Android companion projects from adjacent directories or submodules.

| Project | Role | Runtime and permissions | Guide |
| --- | --- | --- | --- |
| [nl2sh](https://github.com/nl2sh/nl2sh) | Agent, TUI/Web/CLI, Tool Runtime | Single Rust device binary; privileges follow caller/root policy | [Start](../getting-started/index.md) |
| Built-in `src/protocol/` | A2A 1.0, HTTP/stdio MCP | One device Rust executable; direct tools need no model, runtime ADB or Python | [MCP/A2A](../advanced/a2a-mcp.md) |
| [android-bridge](https://github.com/nl2sh/android-bridge) | Accessibility, Unicode input and gestures | Optional installed API 26+ APK; provider/broadcasts require shell/root; services enabled manually | [Integration](../advanced/android-bridge.md) |
| [jadx-helper](https://github.com/nl2sh/jadx-helper) | One-class APK decompilation | API 26+ DEX JAR through app_process; not installed as an APK | [APK/JADX](../tools/apk-jadx.md) |
| [nl2sh-helper](https://github.com/nl2sh/nl2sh-helper) | Android ADB installer and browser launcher | API 26+ controller, TCP or target Android 11+ wireless pairing; deploys latest ARM64/ARMv7 release | [Installation](../getting-started/installation.md#nl2sh-helper) (repository access required) |

The ADB installer is not the Accessibility companion. It verifies release checksums, manages `/data/local/tmp/nl2sh-helper/`, and launches the target's Web UI on port 9999. It does not install the companion, supply model credentials, or serve A2A/MCP. Its source and bilingual guides currently require repository access; see [installation](../getting-started/installation.md#nl2sh-helper) for usage.

Companion/helper release cycles are independent. The native release aggregates the selected artifacts in an authenticated compatibility manifest. Bridge tagged releases require APK signing; local unsigned builds remain available for development. JADX defaults come from the signed embedded policy, and unsigned source builds require explicit offline/custom configuration. Preserve package names, provider authority and DEX entrypoint when coordinating interfaces.

Protocol tool and Agent mutations default to device-terminal approval. Explicit `protocol_auto_approve` permits all risk levels while retaining risk and capability checks. Full UI automation requires shell/root. Web and protocol services have separate ports and access policies.

Report bugs in the component repository with version, Android API/ABI, caller UID, backend/transport and redacted evidence. Shared organization contribution guidance is maintained in [.github](https://github.com/nl2sh/.github). The organization's TUR fork serves packaging/upstream contribution work, not Agent runtime development.
