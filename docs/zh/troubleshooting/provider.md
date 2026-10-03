# Provider 排查

| 现象 | 检查 |
| --- | --- |
| 401 / 403 | Provider、Key 完整性、有效期和权限，不重复请求修复错误 Key |
| 429 | 服务配额、速率限制，等退避结束或降低请求频率 |
| 404 / 405 | Base URL、协议；auto 首次可回退，已选协议405有限重试 |
| 5xx | 上游服务暂时故障，保留错误证据 |
| 流提前结束 | 结果不完整，已输出内容不自动重放 |
| Tool JSON 损坏 | 看有界修复结果，程序不会执行损坏参数 |
| 模型列表空或失败 | 手工填写准确模型 ID，列表与推理是不同接口 |

Web 获取模型失败显示关联编号，在 JSONL 查 `web_model_list_started` / `web_model_list_finished`；记录服务域名、耗时、错误，不包含上游响应正文或 Key。保存后的连接测试是实际模型推理，不执行设备命令。

特殊兼容服务可试显式 `chat_completions` / `responses`，先做短请求再做只读 Agent 任务。窗口元数据缺失时可配置明确上限；不要把未知 Token 显示当零。

更多：[自定义 Provider](../advanced/custom-provider.md)、[代理](../advanced/proxy.md)。
