# config.toml reference

`protocol_start_with_service` defaults to `false`; enabling it lets managed `service start/restart/stop` control Web and MCP/A2A together; direct TUI and `--web-only` startup honor it as well. `protocol_service_port` defaults to `8765` (zero requests an available port). Apply changes with `service restart`. This does not enable automatic approvals. Connection details and generated tokens go to the private `config.service/service.log`. See [starting with the service](../advanced/a2a-mcp.md).

On upgrade, the retired top-level `bridge_auto_approve` setting is ignored. Its old value never enables `protocol_auto_approve`, which remains off by default. Loading leaves the source file intact; saving through a configuration editor removes the retired field. Configuration wizards and Web validation/saving follow the same rule. Other unknown fields and invalid TOML are still rejected.

`src/config/model.rs` defines fields and validation; `src/config/loader.rs` defines loading. Unknown fields are rejected. Saves use private permissions and atomic replacement.

Path priority: `--config` → nonempty `NL2SH_CONFIG` → default path. Direct Android uses `config.toml` beside the resolved executable; Termux uses XDG config `nl2sh/config.toml`. Field priority: CLI overrides → applicable environment variables → file → defaults.

Models can also inspect, change, or reset the active configuration using [nl2sh_config](../tools/configuration.md). Writes require approval, credentials remain user-managed, and the current task does not hot-reload.

## Common settings

`api_type` accepts `auto/responses/chat_completions`; auto is omitted on serialization but remains the default. `security_level` accepts `strict/balanced/unsafe`; `execute_confirm_policy` accepts `always/risk_only/never`. Permissive preferences still retain mandatory mutation and dangerous-action approval. Protocol auto-approval is a separate explicit default-off setting; see [safety approvals](../guide/security-confirmation.md).

`execute_user_mode` is `auto/normal/root`; `ui_language` is `zh_cn/en`, with Chinese terminal default. Website language selection is independent. Budget presets and explicit overrides are documented under [Agent](../advanced/agent-mode.md).

Unspecified `jadx` and `tailcat` groups are off; `tool_overrides` supersedes group settings. `tailcat_binary_path` defaults to `/data/local/tmp/tailcat` on Android and `tailcat` on development hosts. Generated defaults below come from the development host and do not imply identical platform paths.

Relative `history_log_file` paths resolve in the state directory. Default Termux uses XDG state; explicit config paths keep state beside the config. Live output, capture, model results, and logs have separate limits and explicit truncation: defaults are 256 KiB, 1 MiB, 128 KiB, 256 KiB/event, and 10 MiB/file.

Custom `[[security_rules]]` contain `id/pattern/risk/message` and only raise risk; built-in rules cannot be cleared.

## All fields and defaults

This section is updated from code. Credentials default to empty; never add real keys to examples.

<!-- generated:start -->

| Field | Default | Description |
| --- | --- | --- |
| `api_key` | `""` | Optional bearer token; empty is valid for non-OpenAI local services. |
| `ima_enabled` | `false` | Enables the independent read-only Tencent ima connector. |
| `ima_client_id` | `""` | ima OpenAPI Client ID; never sent to the model or logs. |
| `ima_api_key` | `""` | ima OpenAPI API key; never sent to the model or logs. |
| `ima_knowledge_base_id` | `null` | Optional default knowledge-base ID used instead of account discovery. |
| `jev_api_key` | `""` | Optional TypeSafe Jev API key used only by judge_audio_quality. |
| `jev_endpoint` | `"https://api.typesafe.ai/v1/systemone"` | Jev System One endpoint. |
| `jev_model` | `"jev-latest"` | Jev model alias or version. |
| `model` | `"openrouter/free"` | Provider model identifier. |
| `model_context_window` | `null` | Optional user/provider context-window override in tokens. |
| `model_max_output_tokens` | `null` | Optional user/provider maximum output-token override. |
| `endpoint` | `"https://openrouter.ai/api/v1"` | API base URL, normally ending in `/v1`. |
| `proxy_enabled` | `false` | Master proxy switch. Disabling it preserves all proxy fields. |
| `proxy_type` | `"http"` | Proxy transport selected by the TUI. |
| `proxy_address` | `""` | Proxy host and port without credentials or scheme. |
| `proxy_username` | `""` | Optional proxy authentication username. |
| `proxy_password` | `""` | Optional proxy authentication password. |
| `proxy_bypass` | `"localhost,127.0.0.1,::1"` | Comma-separated hosts which bypass the proxy. |
| `skipped_update_version` | `null` | Release version suppressed by the user in the update prompt. |
| `max_context_turns` | `16` | Maximum complete text interaction units retained. |
| `max_agent_steps` | `50` | Maximum model/tool iterations per request. |
| `agent_mode` | `"normal"` | Named budget profile used as a user-facing hint. |
| `max_tool_calls` | `100` | Maximum tool calls attempted during one task. |
| `max_task_execution_time_secs` | `1800` | Maximum active wall-clock seconds, excluding confirmation waits. |
| `replan_after_stalled_steps` | `6` | Consecutive stalled steps before forcing a strategy change. |
| `abort_after_stalled_steps` | `12` | Consecutive stalled steps before ending the task. |
| `max_same_action_retries` | `3` | Maximum executions of the same command with the same result. |
| `hard_max_agent_steps` | `200` | Built-in absolute step ceiling applied after user configuration. |
| `llm_retry_count` | `3` | Number of retries after the initial LLM attempt. |
| `llm_retry_base_delay_ms` | `500` | Initial exponential retry delay. |
| `llm_request_timeout_secs` | `60` | HTTP request timeout. |
| `execute_timeout_secs` | `30` | Ordinary command timeout. |
| `interactive_execute_timeout_secs` | `0` | Interactive timeout; zero disables it. |
| `execute_confirm_policy` | `"risk_only"` | General confirmation preference. |
| `security_level` | `"balanced"` | Safety posture. |
| `protocol_start_with_service` | `false` | Start device HTTP MCP/A2A with Web/TUI services, including the managed background service. |
| `protocol_service_port` | `8765` | Preferred MCP/A2A port with Web/TUI; occupied ports fall back to an available port, zero assigns one directly. |
| `protocol_token` | `""` | Fixed MCP/A2A HTTP token; empty generates one at startup, NL2SH_PROTOCOL_TOKEN overrides it. |
| `protocol_auto_approve` | `false` | Automatically approve operations received through the device MCP/A2A service. |
| `tool_groups` | `{}` | Enabled state of optional tool groups. `jadx` and `tailcat` default off. |
| `tool_overrides` | `{}` | Per-tool enabled state, overriding its group. |
| `tailcat_binary_path` | `"tailcat"` | Tailcat executable path on this runtime. |
| `execute_user_mode` | `"auto"` | Command execution identity mode. |
| `enable_pty` | `true` | Enables real PTY execution instead of pipeline fallback. |
| `ascii_symbols` | `false` | Replaces Emoji labels with ASCII labels. |
| `show_buddha_ascii_art` | `true` | Shows the Buddha ASCII art in startup and help content. |
| `show_train_ascii_art` | `true` | Plays the one-shot ASCII train animation on startup. |
| `ui_language` | `"zh_cn"` | Terminal interface language; Simplified Chinese is the default. |
| `history_log_file` | `"nl2sh.log"` | JSON Lines interaction log, relative to the configuration directory by default. |
| `ui_live_output_max_bytes` | `262144` | Maximum bytes retained from live command output in the TUI. |
| `tool_output_max_bytes` | `1048576` | Maximum bytes captured for one command result. |
| `model_tool_output_max_bytes` | `131072` | Maximum bytes from one tool result sent back to the model. |
| `history_log_event_max_bytes` | `262144` | Maximum unencoded message bytes in one history event. |
| `history_log_max_bytes` | `10485760` | Maximum bytes written to one history log file per process run. |
| `security_rules` | `[]` | Additional rules that can only raise risk. |

<!-- generated:end -->
