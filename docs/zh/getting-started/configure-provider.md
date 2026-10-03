# 配置模型服务

准备服务商提供的 API Key、准确模型名称与设备可访问的 Base URL。直接工具调用不需要设备模型；普通 Agent 对话需要。

## Web 快速开始

打开启动时显示的 Web 地址。缺少 Provider 配置时自动出现快速开始：选择服务 → 填密钥、模型和可编辑的 Base URL → 保存并测试实际模型对话。可点“获取模型列表”读取未保存的表单地址和密钥，查询不保存配置；失败时保留手工输入并显示日志关联编号。

Web 引导首选 DeepSeek / `deepseek-flash`；Rust 新配置默认 OpenRouter / `openrouter/free`。这是两个不同的入口默认值，已有配置不会被引导默认值覆盖。

## TUI 设置

输入 `/config`（别名 `/setting`）。Tab/Shift+Tab 切分类，Up/Down 选字段，Left/Right 调整选择，Ctrl+S 保存。服务分类包含 OpenRouter、OpenAI、DeepSeek、Moonshot/Kimi、SiliconFlow、Ollama、Custom；切 Provider 回填地址，保留 Key、模型与协议。Ollama 和 Custom 各自保留本次面板中的地址草稿。“模型与智能体”可查询在线模型并回填窗口元数据。

## 最小配置

```toml
endpoint = "https://openrouter.ai/api/v1"
model = "openrouter/free"
api_key = "你的密钥"
ui_language = "zh_cn"
```

配置为私有 `0600` 文件。直接部署默认位于解析后的可执行文件旁；Termux 用 XDG 目录。`NL2SH_API_KEY` 覆盖文件中的 Key，CLI `--endpoint` / `--model` / `--api-type` 优先于文件值。

`api_type` 默认 `auto`，尝试 Responses，再在无输出的协议不兼容时回退 Chat Completions。鉴权、限流、超时和已输出内容后的错误不触发切换。服务可用模型、价格与限制以服务商为准。

下一步：[第一个任务](first-task.md)、[自定义 Provider](../advanced/custom-provider.md)、[Ollama](../advanced/ollama.md)、[配置参考](../reference/configuration.md)。
