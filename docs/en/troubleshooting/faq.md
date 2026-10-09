# Frequently asked questions

## Installation and permissions

**Is Termux or Root required?** No. Direct ADB deployment requires Android API 26+ and a matching ABI. Permissions determine available capabilities. See [installation](../getting-started/installation.md) and [permissions](permissions.md).

**Which ABIs are supported?** Direct releases contain ARM64/ARMv7/x86_64; independent Termux packages contain aarch64/arm/x86_64. Hardware CPU alone does not prove 64-bit system support. See [ADB](adb.md).

## Models and Agent

**Is OpenAI required?** No. Use compatible services such as OpenRouter/DeepSeek, custom providers, or Ollama, with supported tool calling. See [model setup](../getting-started/configure-provider.md).

**Why multiple tools?** Agent plans from results; Command mode generates one command. See [Agent modes](../advanced/agent-mode.md).

**Where are keys stored?** Private config.toml or environment variables. Web can edit configuration without login, so use trusted networks.

## Safety and Web

**Can a model run dangerous commands automatically?** Defaults require strong confirmation, including Root. Explicit protocol_auto_approve permits every risk level for protocol calls only. See [approvals](../guide/security-confirmation.md).

**Why is port 9999 unavailable?** Check actual startup port, device address, network, and ADB forwarding; occupied ports can change the listener. See [network troubleshooting](network.md).

**Does Web have login?** No. It listens on all IPv4 interfaces and visitors can edit config and submit tasks. See [Web](../guide/web.md).

## Optional tools and integration

**Why are APK/JADX or Tailcat absent?** Groups default off; enable a group or tool. Disabled tools also cannot run through direct calls. See [tools](../tools/index.md).

**Why does raw PCM ask for metadata?** Headerless PCM lacks reliable sampling information; actual values are required instead of guesses. See [audio](../tools/audio.md).

**Can external agents control Android?** A2A/MCP provides gated tools; writes default to device-terminal decisions, and clients must check success/evidence. See [A2A/MCP](../advanced/a2a-mcp.md).
