# nl2sh 自身配置管理

`nl2sh_config` 让模型通过结构化参数查询、修改或重置当前 nl2sh 的配置。可以直接提问“查看 nl2sh 的任务预算”“把最大工具调用次数设为 120”或“启用 APK 分析工具，先让我确认”。工具固定使用当前进程加载的 `--config`、`NL2SH_CONFIG` 或默认配置路径，不接受模型指定其他文件。

接口参考 [Hermes 的配置管理](https://hermes-agent.nousresearch.com/docs/user-guide/configuration)中的按键查询、修改和点分路径方式；nl2sh 使用自身 TOML 字段和安全策略，不承诺 Hermes API 兼容。

## 动作与参数

| 动作 | 参数 | 行为 |
| --- | --- | --- |
| `list` | 无 | 列出支持的键、默认值、磁盘值、加载解析值、当前任务值及写入风险；枚举字段列出合法取值 |
| `get` | `key` | 查询一个键；工具单项额外返回继承组开关后的有效启用状态 |
| `set` | `key`、`value` | 校验原生 JSON 值并准备修改预览，获批后保存 |
| `reset` | `key` | 删除文件中该键的覆盖，重新使用加载器的默认值或继承规则 |

`value` 使用真正的数字、布尔、字符串、数组或对象，不把数字/布尔包装成字符串；`null` 不作为 set 值，需要清除可选项时使用 reset。顶层键与 [配置参考](../reference/configuration.md)一致；点分键只支持 `tool_groups.jadx`、`tool_groups.tailcat` 和已注册可选工具的 `tool_overrides.<工具名>`。未知键、未知动作、多余参数、错误类型、无效枚举及违反上限的值会在审批前拒绝。

```json
{"action":"get","key":"max_tool_calls"}
```

```json
{"action":"set","key":"max_tool_calls","value":120}
```

```json
{"action":"set","key":"tool_groups.jadx","value":true}
```

```json
{"action":"reset","key":"tool_overrides.tailcat_check"}
```

`persisted` 表示文件中实际写入的值，缺省为 `null`；`resolved` 表示磁盘配置经过默认值、预算预设和环境覆盖后的解析值；`current_task` 表示当前任务启动时使用的快照，含该入口的 CLI/运行覆盖。默认关闭的工具组在 resolved/current_task 中以 `false` 表示。工具单项返回 `enabled_after_reload` 和 `enabled_current_task`，避免把未设置的覆盖误认为关闭。

设置 `agent_mode` 不删除已有显式预算字段；例如已有 `max_agent_steps` 时它继续优先。reset 删除显式预算字段后才恢复对应模式预设。设置工具组也保留原有单项覆盖；需要恢复组继承时对对应 `tool_overrides.<工具名>` 使用 reset，或在用户设置页切换组开关。

## 审批与凭据

list/get 是只读操作。普通设置和 reset 都按修改类确认；涉及安全策略、确认策略、Root 身份、桥接自动批准、自定义安全规则、工具组/单项开关、Tailcat 程序路径、Provider/Jev 地址、代理设置及审计日志路径/限额时按 Dangerous 强确认。风险由本地字段策略决定，模型不能降低；当前入口已有的显式运行许可或桥接自动审批仍遵守原规则。修改 `bridge_auto_approve` 不会立即替换当前调用的确认器。

`api_key`、`ima_client_id`、`ima_api_key`、`jev_api_key`、`proxy_username` 和 `proxy_password` 只返回是否已配置，不返回明文，且不能经工具 set/reset。请用户在 TUI `/config` 或 Web 配置页管理凭据。其他显示值中的已知凭据和 HTTP(S) URL 的认证、查询及 fragment 会脱敏；工具拒绝设置含认证、查询或 fragment 的 Provider/Jev 地址。解析错误不会回显包含密钥的 TOML 行。

文件与单值分别限制为 256 KiB 和 16 KiB。工具保留其他字段、文件中的凭据及未修改内容的注释，不把环境变量或 CLI 覆盖值写回文件。保存使用私有临时文件、`0600` 权限和同目录原子替换。工具写入通过固定锁文件串行化；审批后检测到配置内容变化会拒绝保存，要求重新准备预览。不支持符号链接、目录及其他非普通配置文件；缺失文件只能在现有父目录内经确认创建，读取失败或 TOML 损坏不会被当成空文件覆盖。

## 何时生效

写入结果明确报告修改已保存及需要重新加载。当前任务的模型客户端、工具列表、审批器、预算和终端状态继续使用原快照。新 Web 任务与新 bridge 进程会加载磁盘配置；TUI 请安全退出并重新启动后应用。环境变量和 CLI 参数仍可能覆盖刚保存的字段。工具不自动重启程序、不修改环境变量，也不因开启工具而自动批准后续动作。

完整参数 Schema 见 [工具参数目录](../reference/tool-catalog.md)。该工具同时用于内置 Agent 与 bridge 直接调用，两者共用准备、安全评估、确认和执行流程。
