# TUI 终端界面

无参数运行 `nl2sh` 进入多轮 Agent。缺少配置仍可打开界面，但普通模型任务需先配置 Provider。界面按终端能力选择 TrueColor / ANSI 256；`--ascii` 使用 ASCII 标签。

启动页的“MCP / A2A”区域显示启动时读取的协议进程状态、已确认的公告地址、HTTP 鉴权和设备启动/本地 stdio 命令。设置 `protocol_start_with_service=true` 后 TUI 一并启动协议，并显示运行版本、完整 Bearer 令牌和实际端点；未启动或状态未知时，8765 地址明确标为启动后的默认本机示例。需要刷新状态时运行 `nl2sh --config <配置路径> service status`，或在 Web 的“MCP / A2A”窗口刷新。连接与本地审批详见 [设备 MCP/A2A](../advanced/a2a-mcp.md)。

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

Ctrl+C 会取消正在等待的模型请求并返回空闲状态，可继续发送新任务。Ctrl+Q 在取消当前任务并完成必要清理后退出、恢复终端；等待模型回复不必等到网络超时。待审批或补充信息的请求会被拒绝或取消，后续工具不会继续执行。

输入 `/` 显示候选，Up/Down 选择、Enter 补全。未知斜杠命令不会发给模型。用 Shift+拖选和宿主终端右键复制；Windows ADB 启动器使用滚轮转 Up/Down 的兼容模式。

工具运行时显示有界实时输出，结束后折叠结果；最终回答支持 Markdown、围栏代码高亮、Unicode 表格和窄屏降级。状态栏显示真实 Token 与预算，未知用量显示未知；支持的 Provider 余额只保存在内存。

## 本地命令

`!id` 不调用模型，但经过完整安全分类、确认和执行链，输出不加入模型上下文。`/shell` 暂停 TUI 并打开普通系统 shell，`exit` / Ctrl+D 返回；此 shell 的输入输出不进模型或审计日志，属于用户直接控制的 shell。全屏命令临时挂起 TUI，结束后恢复终端与完整重绘。

所有可用命令见 [Slash Commands](../reference/slash-commands.md)，审批与精确许可见 [安全确认](security-confirmation.md)。

![TUI 存储分析](../../assets/tui.png)

## 工具配置

输入 `/config`（或 `/setting`），用 Tab/Shift+Tab 切到“工具”。Up/Down 选择 APK/JADX、Tailcat 组或单项工具，Left/Right 或空格切换，Ctrl+S 保存并自动重载，Esc 放弃。长列表跟随选中项显示。单项显示实际启用状态及“继承组”或“单项”来源；切换组会清除组内单项覆盖。两组默认关闭，启用不代表批准执行，安全分类、Root 检查和确认仍按原规则执行。ima 开关和凭据位于“知识库”分类。

## Tailcat 快捷共享

执行 `/tailcat` 打开弹窗。默认选择 Web 实际端口与 ADB 连接端口（自动检测，失败默认 5555，可编辑），点击确定后直接安装并共享，不再弹出安全确认；显示下载及执行状态，完成后保留窗口和可复制对端命令。全程无需 LLM，复用现有工具的校验和执行逻辑。详见 [Tailcat](../tools/tailcat.md#tailcat-quick)。

确定或重试会自动停止旧的托管 Tailcat 监听器并重新共享；完成或失败后窗口都保留，仅手动关闭。
