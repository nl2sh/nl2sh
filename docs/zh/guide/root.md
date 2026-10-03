# Root 与执行身份

| `execute_user_mode` | 行为 |
| --- | --- |
| `auto` | UID 0 直接执行；普通命令保持身份，仅需要 Root 时尝试 `su` |
| `normal` | 不自动调用 `su` |
| `root` | 每条命令要求 UID 0 或可用 `su`，失败不降级 |

Root 不是使用 nl2sh 的前提。可读取哪些文件、系统服务和应用状态由 Android 权限决定。模型的 `requires_root` 只是提示，提升计划由本地检查确定并绑定到批准的精确命令。

Root 条件的只读命令也要求确认；任务级精确许可和运行期普通修改许可均不适用于 Root、Dangerous、Critical 或强确认动作。`su` 参数化调用，不通过拼接增加权限。

主机启动器的 root adbd / `su` 回退与 Agent 执行模式是不同层次：启动器决定程序进程身份，`execute_user_mode` 决定后续命令计划。不要通过放宽包含密钥的文件权限解决身份不匹配。

[权限排查](../troubleshooting/permissions.md)说明 Termux UID 与 shell/root 的差异。
