# 自定义 Provider 与协议

Provider 必须提供 nl2sh 支持的 Responses 或 Chat Completions Tool Calling 结构。Base URL 通常包含 `/v1`；不要把完整 `/chat/completions` 请求路径当 Base URL。

```toml
endpoint = "https://provider.example.com/v1"
model = "exact-model-id"
api_key = "你的密钥"
api_type = "auto"
```

本地非 OpenAI 服务可留空 Key，此时不发空 Authorization header；官方 OpenAI 需要 Key。模型名称、窗口和最大输出量用服务商事实核对；必要时覆盖 `model_context_window`、`model_max_output_tokens`。元数据查询失败仍允许手工输入。

自动协商优先 Responses，仅在无内容输出时的不兼容端点或结构回退 Chat 并缓存协议。已成功协商或显式指定的协议遇偶发 405，按 `llm_retry_count` 有限退避重试；首次自动探测的 405 可立即回退。401 不重试，输出后的流错误不自动重放。

可用 `--endpoint`、`--model`、`--api-type chat_completions` 做临时覆盖，统一校验后使用。若服务只实现普通聊天而无 Tool Calling，不能保证 Agent 工作；先用短模型测试，再运行小型只读任务。

排查见 [Provider](../troubleshooting/provider.md)，内网服务见 [Ollama](ollama.md)。
