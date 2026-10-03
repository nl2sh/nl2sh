# Web 开发

`web/` 使用 Preact、TypeScript、Vite 与纯 CSS。后端在 `src/web_ui.rs`；SSE 传会话状态，WebSocket 承载安全终端，生产资源 rust-embed 到单个二进制。

```bash
cd web
npm ci
npm test
npm run build
```

手工构建输出 `web/dist/`，Cargo 自动构建在 target/ 的副本，源码不提交 dist/node_modules。检查 npm scripts 后可使用开发服务器；开发时需显式配置到实际后端的同源/代理路径，不假定浏览器开发服务器就是 Android 服务。

配色通过 style.css 的语义变量，按 UI_DESIGN.md 保持 TUI/Web 一致。未知语言高亮必须转义为纯文本。展示统计、图表和思考区不进入模型历史或安全决策。

行为改动同时更新两种语言的 Web/配置/故障排查页面，测试断线恢复、审批编号/复核阶段、凭据脱敏与窄屏按钮可见性；页面样式测试不能替代执行安全测试。
