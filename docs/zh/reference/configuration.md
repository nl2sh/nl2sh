# config.toml 参考

`protocol_start_with_service` 默认 `false`；设为 `true` 时受管后台 `service start/restart/stop` 一并管理设备 MCP/A2A，直接 TUI 与 `--web-only` 启动也读取该开关，保存修改后需重启服务。`protocol_service_port` 默认 `8765`，可换端口或设为 `0` 自动分配。该开关不启用自动审批，完整连接信息与自动令牌写入私有 `config.service/service.log`。详见 [随服务启动](../advanced/a2a-mcp.md)。

升级时自动忽略已移除的顶层 `bridge_auto_approve`，旧值不会启用 `protocol_auto_approve`，后者仍默认关闭。加载不改写原文件；通过配置编辑器保存时移除废弃字段。配置向导和 Web 校验/保存采用同一规则，其他未知字段与 TOML 语法错误仍拒绝。

源码 `src/config/model.rs` 定义字段与验证，`src/config/loader.rs` 定义加载。未知字段拒绝，保存使用私有权限与原子替换。

路径优先级：`--config` → 非空 `NL2SH_CONFIG` → 默认路径。直接 Android 默认解析后的可执行文件旁 `config.toml`；Termux 默认 XDG config 的 `nl2sh/config.toml`。字段优先级：CLI 覆盖 → 对应环境变量 → 文件 → 默认值。

也可让模型通过 [nl2sh_config](../tools/configuration.md) 查询、修改或重置当前配置；修改需确认，凭据由用户管理，当前任务不热重载。

## 常用设置

`api_type` 可选 `auto/responses/chat_completions`；auto 省略序列化但仍为默认。`security_level` 可选 `strict/balanced/unsafe`，`execute_confirm_policy` 可选 `always/risk_only/never`；宽松设置也不能解除强制修改或危险确认。桥接自动批准是单独、默认关闭的显式设置，见 [安全确认](../guide/security-confirmation.md)。

`execute_user_mode` 是 `auto/normal/root`；`ui_language` 是 `zh_cn/en`，终端默认中文，网站语言选择独立于此配置。预算 `fast/normal/deep` 与显式覆盖见 [Agent](../advanced/agent-mode.md)。

`tool_groups` 的 `jadx`、`tailcat` 未设置时关闭；`tool_overrides` 按名覆盖。`tailcat_binary_path` 在 Android 默认为 `/data/local/tmp/tailcat`，开发主机默认为 `tailcat`。下方生成默认值来自开发主机，不代表平台路径完全相同。

`history_log_file` 相对状态目录解析；默认 Termux 使用 XDG state，显式配置路径跟随配置目录。实时输出、捕获、模型结果与日志分别设限，截断有明确标记：默认 256 KiB、1 MiB、128 KiB、每事件 256 KiB、每文件 10 MiB。

自定义 `[[security_rules]]` 包含 `id/pattern/risk/message`，只提高风险，不能清除内置规则。

## 全部字段与默认值

以下区域由代码导出更新；所有凭据默认空。不要在示例中放真实密钥。

<!-- generated:start -->

| 字段 | 默认值 | 说明 |
| --- | --- | --- |
| `api_key` | `""` | 主模型访问密钥，可由环境变量覆盖 |
| `ima_enabled` | `false` | 启用独立只读 ima 连接器 |
| `ima_client_id` | `""` | ima Client ID，保密 |
| `ima_api_key` | `""` | ima 密钥，保密 |
| `ima_knowledge_base_id` | `null` | 可选固定知识库 ID |
| `jev_api_key` | `""` | 可选 Jev 音质模型密钥 |
| `jev_endpoint` | `"https://api.typesafe.ai/v1/systemone"` | Jev System One 地址 |
| `jev_model` | `"jev-latest"` | Jev 模型名称 |
| `model` | `"openrouter/free"` | 主模型标识 |
| `model_context_window` | `null` | 可选上下文窗口 Token 覆盖 |
| `model_max_output_tokens` | `null` | 可选最大输出 Token 覆盖 |
| `endpoint` | `"https://openrouter.ai/api/v1"` | Provider Base URL |
| `proxy_enabled` | `false` | 代理总开关，关闭保留字段 |
| `proxy_type` | `"http"` | http/socks5/socks5h |
| `proxy_address` | `""` | 不带协议/凭据的代理主机:端口 |
| `proxy_username` | `""` | 可选代理用户名 |
| `proxy_password` | `""` | 可选代理密码 |
| `proxy_bypass` | `"localhost,127.0.0.1,::1"` | 逗号分隔的绕过主机 |
| `skipped_update_version` | `null` | 忽略的更新版本 |
| `max_context_turns` | `16` | 保留完整对话轮次上限 |
| `max_agent_steps` | `50` | 单任务模型步骤上限 |
| `agent_mode` | `"normal"` | fast/normal/deep 预算预设 |
| `max_tool_calls` | `100` | 单任务工具尝试上限 |
| `max_task_execution_time_secs` | `1800` | 活跃时长秒数，不含确认等待 |
| `replan_after_stalled_steps` | `6` | 停滞达到此步数要求重规划 |
| `abort_after_stalled_steps` | `12` | 停滞达到此步数终止任务 |
| `max_same_action_retries` | `3` | 同命令同结果的执行上限 |
| `hard_max_agent_steps` | `200` | 配置步骤上限，仍受系统硬限制 |
| `llm_retry_count` | `3` | 初始模型请求后的重试次数 |
| `llm_retry_base_delay_ms` | `500` | 指数退避初始毫秒数 |
| `llm_request_timeout_secs` | `60` | 模型 HTTP 请求超时秒数 |
| `execute_timeout_secs` | `30` | 普通命令超时秒数 |
| `interactive_execute_timeout_secs` | `0` | 交互命令超时，0 不限制 |
| `execute_confirm_policy` | `"risk_only"` | 一般确认偏好，服从强制策略 |
| `security_level` | `"balanced"` | strict/balanced/unsafe 安全偏好 |
| `protocol_start_with_service` | `false` | 默认关闭；Web/TUI 与后台 service 启停时管理 MCP/A2A，重启生效 |
| `protocol_service_port` | `8765` | Web/TUI 随服务启动 MCP/A2A 的首选端口，默认 8765；占用时选择空闲端口，0 直接自动分配 |
| `protocol_auto_approve` | `false` | 默认关闭；显式自动批准设备 MCP/A2A 全部风险操作 |
| `tool_groups` | `{}` | 可选组开关，jadx/tailcat 未设置时关闭 |
| `tool_overrides` | `{}` | 按名覆盖组开关 |
| `tailcat_binary_path` | `"tailcat"` | Tailcat 路径；Android 默认 /data/local/tmp/tailcat |
| `execute_user_mode` | `"auto"` | auto/normal/root 执行身份 |
| `enable_pty` | `true` | 使用真实 PTY；关闭使用管道 |
| `ascii_symbols` | `false` | 用 ASCII 标签替代 Emoji |
| `show_buddha_ascii_art` | `true` | 启动/帮助佛像装饰开关 |
| `show_train_ascii_art` | `true` | 启动小火车装饰开关 |
| `ui_language` | `"zh_cn"` | zh_cn/en 终端语言 |
| `history_log_file` | `"nl2sh.log"` | JSONL 日志路径，相对状态目录 |
| `ui_live_output_max_bytes` | `262144` | TUI 实时输出保留字节上限 |
| `tool_output_max_bytes` | `1048576` | 单流执行捕获字节上限 |
| `model_tool_output_max_bytes` | `131072` | 模型单工具结果字节上限 |
| `history_log_event_max_bytes` | `262144` | 单日志事件未编码消息字节上限 |
| `history_log_max_bytes` | `10485760` | 日志文件字节上限 |
| `security_rules` | `[]` | 仅提高风险的自定义规则 |

<!-- generated:end -->
