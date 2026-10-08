# Slash Commands

所有去掉前导空白后以 `/` 开头的 TUI 输入都是本地命令，不进入 LLM。未知命令只提示，可给纠错建议但不会自动执行。输入 `/`、Up/Down 选择、Enter 补全。

| 命令 | 用途 |
| --- | --- |
| `/help` | 本地帮助 |
| `/new` | 新空白会话，保留快照和审计 |
| `/clear` | 清空当前对话、模型上下文和输入历史 |
| `/config`、`/setting` | 统一设置面板 |
| `/permission` | 查看运行期普通修改许可 |
| `/permission allow`、`/permission ask` | 开启 / 关闭该内存许可 |
| `/tailcat` | 无需模型的 Web / ADB 共享弹窗，显示进度与可复制对端命令 |
| `/balance` | 查询支持 Provider 的只读余额，内存保存 |
| `/sessions` | 最近会话列表 |
| `/sessions resume NAME` | 恢复会话 |
| `/sessions rename OLD NEW` | 重命名 |
| `/sessions delete NAME` | 删除快照 |
| `/update` | 检查更新，包管理版本提示 pkg upgrade |
| `/shell` | 直接交互 shell，exit / Ctrl+D 返回 |
| `/exit` | 安全退出 |

`/provider`、`/model`、`/models`、`/proxy` 已移除，使用 `/config`；这些旧名称出现在历史 changelog 不表示当前可用。

`!command` 是本地安全执行前缀，不是 slash 命令。`/shell` 属于用户直接系统 shell；二者的模型上下文与审计范围不同，见 [TUI](../guide/tui.md)。
