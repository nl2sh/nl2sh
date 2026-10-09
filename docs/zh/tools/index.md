# 工具能力

工具由显式注册表向模型提供名称与 JSON Schema。参数先验证，操作先准备，再评估风险、确认并执行；工具输出标记完整/部分/失败/超时，不能只根据模型文字判断成功。

| 任务 | 文档 |
| --- | --- |
| 设备诊断与 UI | [Android](android.md) |
| 读取、搜索、补丁 | [文件系统](filesystem.md) |
| WAV / PCM 特征与质量 | [音频](audio.md) |
| ZIP / DEX / 单类反编译 | [APK / JADX](apk-jadx.md) |
| 文件传输与端口服务 | [Tailcat](tailcat.md) |
| 公网 HTTP / TLS | [网络](network.md) |
| ima、便签、图表 | [知识与呈现](knowledge.md) |
| nl2sh 配置查询、修改与重置 | [自身配置](configuration.md) |

APK/JADX 和 Tailcat 默认关闭，Web 工具页或 TUI `/config` 的“工具”分类可按组或单项启用。`tool_overrides` 优先于 `tool_groups`；关闭工具不会进入模型定义或直接调用。切换组开关会清除组内单项覆盖。新 Web 任务读取配置；TUI 保存后自动重载，无需重启。开启工具不等于批准动作。ima 需要独立凭据。

[完整工具参数目录](../reference/tool-catalog.md)由代码导出，包含所有可选项；实际可用性仍由配置、能力和入口决定。设备协议服务支持需要长期存活的 Tailcat 监听器操作。
