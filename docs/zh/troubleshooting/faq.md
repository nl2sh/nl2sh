# 常见问题

## 安装与权限

**必须 Termux 或 Root 吗？** 不必。直接 ADB 部署只需 Android API 26+ 与匹配 ABI；权限决定可用能力。[安装](../getting-started/installation.md) / [权限排查](permissions.md)。

**支持哪些 ABI？** 官方直接部署包为 ARM64/ARMv7；自建 Termux 包为 aarch64/arm。不要从硬件 CPU 推断系统支持 64 位。[ADB 排查](adb.md)。

## 模型与 Agent

**必须 OpenAI 吗？** 不必，可选 OpenRouter、DeepSeek 等兼容服务、自定义 Provider、Ollama。需要支持当前协议的 Tool Calling。[模型配置](../getting-started/configure-provider.md)。

**为什么调用多个工具？** Agent 根据结果继续规划；Command 模式只生成单条命令。[Agent 模式](../advanced/agent-mode.md)。

**密钥存在哪？** 私有 config.toml 或环境变量；Web 能编辑配置且无登录，应在信任网络使用。

## 安全与 Web

**模型能自动执行危险命令吗？** 默认需强确认，Root 不跳过。显式 bridge_auto_approve 仅桥接入口可自动批准所有等级。[确认](../guide/security-confirmation.md)。

**9999 打不开？** 查看实际端口、设备地址、网络和 ADB forward；端口占用可改变监听端口。[网络排查](network.md)。

**Web 有登录吗？** 没有；默认监听全部 IPv4，接入者可编辑配置与发起任务。[Web](../guide/web.md)。

## 可选工具与集成

**APK/JADX、Tailcat 为什么找不到？** 默认组关闭，按组或单项启用；直接调用也不能运行关闭项。[工具](../tools/index.md)。

**音频 PCM 为何追问参数？** 无头 PCM 无法确定采样元数据，程序要求真实值而不猜测。[音频](../tools/audio.md)。

**外部 Agent 能控制 Android 吗？** A2A/MCP 提供受控工具入口；默认写入由设备交互终端决定，调用结果需检查成功与证据。[A2A/MCP](../advanced/a2a-mcp.md)。
