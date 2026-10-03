# Agent and Command modes

The default Agent plans across model steps, invokes tools, receives real results, and continues. Partial streaming tool arguments never execute. Broken tool-call JSON yields non-executable failures with at most two regeneration attempts rather than guessed arguments; rejected calls still consume budget.

`nl2sh --mode command "Show memory usage"` generates one command, then classifies/confirms/executes it. `--dry-run` only displays it. Agent suits multi-stage feedback; `!command` runs a command explicitly written by the user.

| Preset | Steps | Tool calls | Active time |
| --- | --- | --- | --- |
| `fast` | 20 | 40 | 10 minutes |
| `normal` | 50 | 100 | 30 minutes |
| `deep` | 100 | 200 | 60 minutes |

Explicit `max_agent_steps`, `max_tool_calls`, and `max_task_execution_time_secs` override presets; the example file explicitly sets Normal limits. `hard_max_agent_steps` defaults to 200 and remains bounded by the system ceiling. Approval waits exclude active time; cancellation/timeouts retain observed statistics.

Repeated identical command results eventually prevent further execution. Stalls force replanning before termination; approaching the step budget requests convergence. History evicts complete oldest turns, preserving system instructions, the current task, and complete tool rounds. `max_context_turns` remains a hard bound.

Model/tool/log/display limits are independent; see [configuration](../reference/configuration.md). Budgets and modes do not authorize changes or bypass approvals.
