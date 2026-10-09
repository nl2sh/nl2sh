<p align="center"><img src="docs/assets/logo.png" alt="nl2sh logo" width="180"></p>

# nl2sh

[简体中文](README.md) | [English](README_EN.md)

Let AI complete real tasks in Android Shell. nl2sh is a multi-turn tool-calling Agent built first for native Android shell, with Termux compatibility. One Rust executable includes a rich TUI and multi-session Web UI.

[Documentation](https://nl2sh.github.io/nl2sh/en/) · [Quick start](https://nl2sh.github.io/nl2sh/en/getting-started/) · [Releases](https://github.com/nl2sh/nl2sh/releases/latest)

## Capabilities

- Native Android API 26+ ARM64/ARMv7/x86_64 deployment, without Termux or device Rust/Node.js.
- OpenAI-compatible models, multi-turn Agent, streaming replies, recovery, and @ file references.
- Device/UI/file/network/audio tools; optional APK/JADX, Tailcat, and ima.
- Local TUI/Web risk classification and approvals, retained for Root and edited commands.
- Optional host A2A/MCP and Android companion; “Hermes-like” does not promise API/plugin compatibility.

```text
User → Agent → Tool Calling → Security → Confirmation → Android Shell
```

## Quick start

Install host adb, enable and authorize USB debugging, then download and verify the [Release ZIP](https://github.com/nl2sh/nl2sh/releases/latest). Extract it completely and run `android-run-linux.sh` or `android-run-windows.bat`. Configure a model with `/config`, then ask “Show how much storage this device has left.”

You can also install the [nl2sh-helper](https://github.com/nl2sh/nl2sh-helper) APK on an Android controller. Connect by TCP ADB or the target's Wireless debugging; the helper deploys the ARM64/ARMv7 build and opens its Web UI in a browser. Access to the helper repository and Releases is required. See the [installation guide](https://nl2sh.github.io/nl2sh/en/getting-started/installation/#nl2sh-helper).

In Termux:

```bash
pkg install tur-repo
pkg install nl2sh
nl2sh
```

[Installation and updates](https://nl2sh.github.io/nl2sh/en/getting-started/installation/) · [Provider setup](https://nl2sh.github.io/nl2sh/en/getting-started/configure-provider/) · [Safety approvals](https://nl2sh.github.io/nl2sh/en/guide/security-confirmation/)

Web listens on all IPv4 interfaces without login; visitors can edit configuration or submit tasks. Use trusted networks. APK/JADX and Tailcat default off.

![nl2sh TUI](docs/assets/nl2sh.gif)

![nl2sh Web](docs/assets/web.png)

Optional Android projects: [Android Bridge](https://github.com/nl2sh/android-bridge) · [JADX helper](https://github.com/nl2sh/jadx-helper), each built and released independently outside this repository.

## Development and contributions

Requires stable Rust and Node.js 22+ / npm.

```bash
cargo fmt --all -- --check
cargo check
cargo test
```

[Builds](https://nl2sh.github.io/nl2sh/en/development/build/) · [Architecture](https://nl2sh.github.io/nl2sh/en/development/architecture/) · [Contributing and documentation](https://nl2sh.github.io/nl2sh/en/development/contributing/)

User documentation lives in `docs/zh/` and `docs/en/`. User-visible code changes update both languages in the same PR; new configuration, arguments, and tools also update generated references. Agents follow [AGENTS.md](AGENTS.md). Root development documents retain internal design, plans, and validation context.

[Changelog](https://nl2sh.github.io/nl2sh/en/changelog/) · [MIT License](LICENSE) · [Issues](https://github.com/nl2sh/nl2sh/issues)

Stars, issues, and contributions are welcome. [Support the project](https://github.com/nl2sh/nl2sh) · [Donate](https://suqishuo.cn/uploads/wechatpay.png)

[Organization projects and integration boundaries](https://nl2sh.github.io/nl2sh/en/development/projects/): native Agent, built-in device MCP/A2A, Android Bridge, JADX helper and ADB installer.
