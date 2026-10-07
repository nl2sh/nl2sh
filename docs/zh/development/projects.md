# 项目与集成边界

[nl2sh 组织](https://github.com/nl2sh) 分别维护各组件。从各自仓库根目录构建，主仓库不会通过相邻目录或 submodule 构建 Android 伴侣项目。

| 项目 | 用途 | 运行环境与权限 | 指南 |
| --- | --- | --- | --- |
| [nl2sh](https://github.com/nl2sh/nl2sh) | Agent、TUI/Web/CLI、Tool Runtime | 单个 Rust 设备程序，权限由调用用户/root 策略决定 | [开始](../getting-started/index.md) |
| nl2sh 中的 `a2a_gateway/` | A2A 1.0、HTTP/stdio MCP | 主机 Python 3.11+，adb 固定连接一台设备，直接工具无需设备模型 | [A2A/MCP](../advanced/a2a-mcp.md) |
| [android-bridge](https://github.com/nl2sh/android-bridge) | 无障碍、Unicode 输入与手势 | 可选安装的 API 26+ APK，provider/广播要求 shell/root，手工启用服务 | [集成](../advanced/android-bridge.md) |
| [jadx-helper](https://github.com/nl2sh/jadx-helper) | APK 单类反编译 | API 26+ DEX JAR，通过 app_process 运行，不作为 APK 安装 | [APK/JADX](../tools/apk-jadx.md) |
| [nl2sh-helper](https://github.com/nl2sh/nl2sh-helper) | Android ADB 安装与浏览器启动助手 | API 26+ 控制端，TCP 或目标 Android 11+ 无线配对，部署 ARM64/ARMv7/x86_64 Release | [安装](../getting-started/installation.md#nl2sh-helper)（需仓库权限） |

ADB 安装助手与无障碍伴侣用途不同。助手验证签名发布，管理原生 service 并使用实际 Web 端口，不配置模型凭据，也不提供 A2A/MCP。使用方式见[安装](../getting-started/installation.md#nl2sh-helper)。

伴侣/helper 独立发布，原生发布把所选资产收录到认证的兼容性 Manifest。Bridge 标签发布必须使用 APK 签名，本地可生成未签名开发产物。JADX 默认资产来自内嵌签名策略，未签名源码构建需显式配置离线或自定义来源。协调接口时保持包名、provider authority 与 DEX 入口兼容。

默认 bridge 审批与本地 TUI/Web 不同：直接调用等待设备交互终端批准，Agent 咨询拒绝待确认动作。显式 `bridge_auto_approve` 可自动批准所有风险等级的桥接操作，风险评估与能力检查仍运行。完整 UI 自动化要求 shell/root，普通 Termux UID 权限不同。内置 Web 当前无需登录并监听所有 IPv4 接口，不继承网关 Bearer 鉴权。

在对应组件仓库反馈问题，附版本、Android API/ABI、调用 UID、后端/传输及脱敏证据。共享组织贡献说明在 [.github](https://github.com/nl2sh/.github) 维护。组织中的 TUR fork 用于打包及上游贡献，不是 Agent 运行时开发仓库。
