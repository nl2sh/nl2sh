# 贡献与双语文档维护

提交前阅读根目录 AGENTS.md、ARCHITECTURE.md、PROJECT_PLAN.md、PROJECT_STATUS.md。用户文档唯一事实源是 `docs/zh/` 与 `docs/en/`，README 和模块 README 只保留项目介绍、开发入口与链接。

## 同一个变更同步代码与文档

| 用户可感知变化 | 必须检查的两种语言页面 |
| --- | --- |
| 安装、启动、更新 | getting-started/installation、android-adb、termux |
| Provider、代理 | configure-provider、advanced/custom-provider、proxy |
| 配置、CLI、环境变量 | reference/configuration、cli、environment-variables |
| 工具/参数/启用条件 | tools 对应主题、reference/tool-catalog |
| Slash Commands | reference/slash-commands |
| 审批/Root | guide/security-confirmation、root、reference/security-model |
| Web/会话/文件引用 | guide 对应页面、troubleshooting |
| 发布 | development/release、changelog |

新页面使用相同相对路径，两种语言必须都有实质内容；不要把英文缺页回退中文当作翻译完成。导航、截图说明、例子、风险和默认值同步。PR 模板声明更新范围，或解释无需文档变化。

## 本地文档命令

```bash
python3 -m venv .venv-docs
.venv-docs/bin/python -m pip install -r requirements-docs.txt
.venv-docs/bin/python scripts/check-docs.py
.venv-docs/bin/mkdocs build --strict
.venv-docs/bin/mkdocs serve
```

Windows 创建环境后用 `.venv-docs\Scripts\python.exe` / `mkdocs.exe`。站点根路径是中文，`/en/` 英文；语言切换保留当前页面，编辑链接指向真实语言文件。

代码派生参考更新：

```bash
cargo run --quiet --example docs_export > /tmp/nl2sh-reference.json
python3 scripts/update-docs-reference.py /tmp/nl2sh-reference.json
python3 scripts/update-docs-reference.py /tmp/nl2sh-reference.json --check
```

新增配置/工具同时补 `docs/_data/reference-translations.json` 中文说明。生成区域保留协议原文；人工段落解释行为、风险与平台。CI 每个 PR 检查页面对齐、链接、派生参考与严格构建，master 合并后部署。

## 代码检查

运行 cargo fmt/check/test，按变更增加 Android、安全、PTY/恢复验证。稳定 Rust、无业务 unwrap/expect/panic，阻塞 I/O 用专门线程、资源 RAII。项目日志只写可复现命令/平台/结果，不写用户名、本地绝对路径或会话授权。
