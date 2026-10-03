# Agent 与 Command 模式

默认 Agent 按模型步骤规划、调用工具、接收真实结果并继续回答。模型工具参数未完整接收前绝不执行。解析损坏的 Tool Calling JSON 返回不可执行失败结果，最多允许两次重新生成，不猜补参数；被拒绝调用仍占预算。

Command 模式 `nl2sh --mode command "查看内存"` 只生成一条命令，先分类、确认、执行；`--dry-run` 仅展示。Agent 适合多阶段诊断与反馈，`!command` 适合用户明确写出的本地命令。

| 预设 | Step | Tool Call | 活跃时间 |
| --- | --- | --- | --- |
| `fast` | 20 | 40 | 10 分钟 |
| `normal` | 50 | 100 | 30 分钟 |
| `deep` | 100 | 200 | 60 分钟 |

文件显式设置的 `max_agent_steps`、`max_tool_calls`、`max_task_execution_time_secs` 覆盖预设，配置示例已明确写 Normal 上限。`hard_max_agent_steps` 默认 200，并受系统硬上限约束。等待确认不计活跃时间；取消与超时保留已发生统计。

相同命令重复得到相同结果达到上限后拒绝再次执行；连续停滞先要求重规划，再结束。接近 Step 预算时提示收敛。历史按完整最旧轮次淘汰，system instruction、当前任务和 Tool Calling round 不拆分，`max_context_turns` 仍是硬边界。

模型/工具/日志/显示分别有资源限额，详见 [配置](../reference/configuration.md)。任何预算与模式都不能授权修改或绕过审批。
