# Web 开发

`web/` 使用 Preact、TypeScript、Vite 与纯 CSS。后端在 `src/web/`；SSE 传会话状态，WebSocket 承载安全终端，生产资源 rust-embed 到单个二进制。

```bash
cd web
npm ci
npm test
npm run build
```

手工构建输出 `web/dist/`，Cargo 自动构建在 target/ 的副本，源码不提交 dist/node_modules。检查 npm scripts 后可使用开发服务器；开发时需显式配置到实际后端的同源/代理路径，不假定浏览器开发服务器就是 Android 服务。

配色通过 style.css 的语义变量，按 UI_DESIGN.md 保持 TUI/Web 一致。未知语言高亮必须转义为纯文本。展示统计、图表和思考区不进入模型历史或安全决策。

行为改动同时更新两种语言的 Web/配置/故障排查页面，测试断线恢复、审批编号/复核阶段、凭据脱敏与窄屏按钮可见性；页面样式测试不能替代执行安全测试。

## 后端模块

`server.rs` 管理监听、关闭、路由和嵌入资源；`auth.rs` 保留 Host/Origin 校验。`state.rs` 管理会话状态、检查点、脱敏和持久化；`agent.rs` 执行任务；`interaction.rs` 实现审批和流式适配；`websocket.rs` 推送 SSE 状态。`routes/` 按会话、配置/模型发现、文件、工具/应用、记忆、运行信息和 WebSocket 终端拆分。`src/web_ui.rs` 保留公开兼容入口。前端仍在 `web/src/`，Cargo 从 OUT_DIR 嵌入生成资源。

设备概览显示安装归属、正确升级入口、实际扩展版本和已验证运行清单中的推荐版本。缺少签名策略或未发现组件时保留未知；浏览器不声称能读取远端 Helper App 当前版本。
