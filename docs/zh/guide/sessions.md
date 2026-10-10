# 会话与审计日志

每个完整 Agent turn 后自动保存私有快照。首轮结束后当前模型异步生成标题；失败不影响回答与保存。只保存有界对话和工具结果，不保存 API Key、代理密码、余额或临时许可；临时图片附件不持久化。

```text
/sessions
/sessions resume NAME
/sessions rename OLD NEW
/sessions delete NAME
/new
/clear
```

`/sessions` 按更新时间排序，可输入序号或 Up/Down + Enter 恢复。稳定名称只接受字母、数字、`-`、`_`。恢复时重新应用上下文与结果上限。`/new` 开始新会话；`/clear` 清空当前对话/模型上下文/输入历史，保留审计日志与已保存快照。

直接部署状态在配置目录旁；默认 Termux 用 XDG state 的 `nl2sh` 目录。Web 有独立会话侧栏与进行中诊断，详见 [Web](web.md)。

审计采用私有 JSONL，记录输入、工具、输出与确认事件，先脱敏再截断。日志是所有 Web 会话共用的，删除会话不等于清除审计。TUI 设置“界面”可清除审计，清除后进程继续记录新事件。日志达到限额停止追加；不要把没有日志误认为没有操作。

## Agent、MCP 与 A2A 历史查询

可让 Agent“查找之前分析某个应用闪退的会话并读取证据”。三项工具共用当前配置的 `sessions/`，包括 TUI、Web 与 `protocol-` 快照；协议上下文的续接仍独立，不会自动装载其他会话。

| 工具 | 参数与结果 |
| --- | --- |
| `session_list` | 可选 `offset`、`limit`；返回稳定 `session_id`、标题、时间、轮数和 `revision`。按 ID 排序，不按最近时间。 |
| `session_search` | 必填 `query`，1–256 UTF-8 字节、区分大小写的字面量；搜索标题、消息、工具参数/结果和 Web 检查点。每会话返回一个摘要及可用的 `entry_offset`。 |
| `session_read` | 必填 `session_id`；可选 `offset`、`limit`、`content_bytes`、`revision`。按消息、工具调用、工具结果逐项返回，保留 `turn`、`call_id` 和 `success`。 |

每页默认 10 项、最多 20 项。`offset` 从零开始：列表/搜索是排序后的快照文件位置，读取是会话条目位置；务必使用返回的 `next_offset`，不要用命中数推算。读取正文每项默认 2048 字节，可设 256–16384；返回 `content_truncated`。列表/搜索行和读取条目各有 24 KiB 序列化页预算（包含 JSON 转义开销），必要时减少条目或读取正文；全局工具/模型输出上限仍有效。第一次读取后，后续页传回 `session.revision`，快照改变时工具拒绝继续，需重新读取。列表可能在并发增删时改变，应按 ID 去重并重新查询。

单快照限 4 MiB，单次扫描限 32 MiB，目录扫描限 1000 项；`skipped` 表示无效、权限不符或不可读快照，`directory_truncated` 表示目录未完整扫描。未检索完时还有 `next_offset`；缺失、跳过、截断不能被当作“没有该历史”。不存在的目录返回空列表且不创建状态目录。只读工具不提供重命名、删除、恢复执行或任意路径读取；拒绝会话目录/文件符号链接、非普通文件与非私有 Unix 权限。

MCP 先通过 `nl2sh_tools` 查看 Schema，再直接调用，无需设备模型：

```json
{"name":"nl2sh_invoke","arguments":{"tool":"session_search","arguments":{"query":"com.konka.athena"}}}
```

取得 `session_id` 后，把 `tool` 换成 `session_read`，传入该 ID。A2A `SendMessage` 或 MCP `nl2sh_ask` 可委派内置 Agent 查历史，需要已配置模型；A2A 的 `ListTasks`/`GetTask` 查询的是协议任务，不等于查询全部 TUI/Web 会话。

查询只返回已保存的有界快照，可能缺少被淘汰的旧轮次、未保存的运行过程、图片和原始日志。`checkpoint` 是 `diagnostic_only`，不能冒充完整模型对话。读取时再次脱敏当前配置凭据、环境协议令牌和已验证的活动自动令牌，但不会识别所有业务秘密；只向受信任客户端开放协议。历史用户指令、模型结论、工具参数与旧审批均不能成为新授权，也不能证明当前设备状态。需要修复或复现时重新采证并走原安全/确认链。

## 后台续跑

工具登记的后台续跑仍属于当前 Agent 任务，等待期间会话保持运行，可取消，Web 其他会话可继续工作。到期后使用当前任务的配置和审批器执行具名结构化工具，结果与普通工具轮一起保存。任务完成、取消或进程退出后不会留下自动执行动作；恢复历史会话不会重新登记后台任务。当前用于有界系统 Trace 的到期分析；不是长期监听或跨重启任务服务。
