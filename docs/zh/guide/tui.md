# TUI 终端界面

无参数运行 `nl2sh` 进入多轮 Agent。缺少配置仍可打开界面，但普通模型任务需先配置 Provider。界面按终端能力选择 TrueColor / ANSI 256；`--ascii` 使用 ASCII 标签。

| 操作 | 按键或命令 |
| --- | --- |
| 发送任务 | Enter |
| 编辑输入 | Left/Right/Home/End/Delete，UTF-8 安全 |
| 输入历史 | Up/Down |
| 查看历史 | 滚轮、PageUp/PageDown |
| 展开/收起工具结果 | F2 |
| 取消任务 / 空闲清空输入 | Ctrl+C |
| 安全退出 | Ctrl+Q、`/exit` |
| 设置 | `/config`、`/setting` |
| 新会话 / 恢复 | `/new`、`/sessions` |

输入 `/` 显示候选，Up/Down 选择、Enter 补全。未知斜杠命令不会发给模型。用 Shift+拖选和宿主终端右键复制；Windows ADB 启动器使用滚轮转 Up/Down 的兼容模式。

工具运行时显示有界实时输出，结束后折叠结果；最终回答支持 Markdown、围栏代码高亮、Unicode 表格和窄屏降级。状态栏显示真实 Token 与预算，未知用量显示未知；支持的 Provider 余额只保存在内存。

## 本地命令

`!id` 不调用模型，但经过完整安全分类、确认和执行链，输出不加入模型上下文。`/shell` 暂停 TUI 并打开普通系统 shell，`exit` / Ctrl+D 返回；此 shell 的输入输出不进模型或审计日志，属于用户直接控制的 shell。全屏命令临时挂起 TUI，结束后恢复终端与完整重绘。

所有可用命令见 [Slash Commands](../reference/slash-commands.md)，审批与精确许可见 [安全确认](security-confirmation.md)。

![TUI 存储分析](../../assets/tui.png)
