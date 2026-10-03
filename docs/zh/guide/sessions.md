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
