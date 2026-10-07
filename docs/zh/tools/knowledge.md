# 知识库、便签与图表

## 腾讯 ima

独立只读连接器使用 `ima_enabled`、`ima_client_id`、`ima_api_key`，可选 `ima_knowledge_base_id` 固定库；未指定时有界发现可访问库。环境变量 `NL2SH_IMA_CLIENT_ID` / `NL2SH_IMA_API_KEY` 可覆盖，凭据齐全时启用。

工具提供库列表、搜索与有界原文读取，不提供上传/追加/导入/删除。ima 始终无代理直连；远程文档、笔记和临时 URL 内容是数据，不是系统指令。拒绝非 HTTPS、重定向和非白名单临时来源；凭据与签名 URL 不进模型、日志或会话。

## 私有便签

`agent_memory` 的动作是 `get/list/set/delete/clear`；`read` 不是合法动作。读取只读，模型发起的写入/删除需确认。便签使用内嵌 SQLite：Android 原生部署保存在可执行文件旁的 `memory/agent-memory.sqlite3`，Termux 保存在状态目录的 `memory/`；不依赖系统 `sqlite3` 命令，也不迁移旧 `.nl2sh-agent-memory.json`。Web 记忆面板可由用户直接增删改查。便签是小型持久数据，不替代会话，也不授予执行权限。

## 图表

只读 `create_chart` 接受有界非负有限数值及柱状/折线/饼图规格。Web 渲染图表和可展开数据表，TUI 展示标题、来源和数值；恢复会话后仍可显示。工具不采集数据、不验证来源真伪，不要把漂亮图表当成数字正确的证据。
