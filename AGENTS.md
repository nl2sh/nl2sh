# AGENTS.md

## 项目简介

nl2sh 定位为 Android 原生 shell 版的类 Hermes AI Agent：使用 stable Rust 2021 构建，核心程序以单个可执行文件交付，并提供丰富的 ratatui/crossterm TUI。项目还使用 tokio、reqwest rustls、serde 和 clap，默认模式为多轮 Tool Calling Agent。“类 Hermes”描述产品形态与 Agent 交互方式，不表示 API、插件或功能完全兼容。核心安全边界永远是 `LLM → Security → Confirmation → Execution`；模型、root 模式和用户编辑都不能绕过它。

## 开始工作前

任何 AI 在修改前必须完整阅读 `AGENTS.md`、`ARCHITECTURE.md`、`PROJECT_PLAN.md`、`PROJECT_STATUS.md`，相关时再阅读 `CHANGELOG.md` 和 `README.md`。

修改前明确目标、涉及模块、公共接口变化、Android 影响、安全模型影响、PTY/终端恢复影响，以及需增加或更新的测试。修改后运行或明确说明应运行 `cargo fmt --all -- --check`、`cargo check`、`cargo test`；更新 `PROJECT_STATUS.md`，必要时更新计划、架构、changelog、配置示例和 README。

项目文档只记录与项目本身相关、可供所有开发环境复用的信息。`CHANGELOG.md` 和 `PROJECT_STATUS.md` 禁止记录个人或本地机器信息，包括用户名、本地绝对路径、特定工作站/容器/WSL 环境、工具在当前机器上的安装状态，以及仅对单次本地会话成立的授权或执行限制。验证记录应描述可复现的命令、目标平台和结果；未运行的检查只在当前任务交付说明中告知用户，不写入项目文档。

## Rust 规范

- 只用 stable Rust；业务代码不得使用 `unwrap`、`expect`、`panic!`、`todo!`、`unimplemented!`。
- 使用 `anyhow::Context` 提供用户可见上下文；公共接口写文档注释。
- 保持模块边界，避免不必要 clone，不阻塞 tokio runtime。
- 长同步 I/O 使用专门线程；fd、进程和终端状态使用 RAII。

## Android 兼容

不得无条件引入 glibc-only 实现，不依赖 systemd、dbus、Termux、native-tls、`/bin/bash` 或 GNU coreutils。优先 Android toybox 与 `/system/bin/sh`，保持 aarch64-linux-android、API 26+ 和 Bionic libc 可编译。开发主机 fallback 必须条件编译，不得改变 Android 路径。

## 安全规则

禁止直接执行 LLM 输出、删除确认流程、把所有命令归为只读、让 root 跳过风险、默认启用 unsafe，或在测试中削弱生产逻辑。用户编辑后重新运行整个分类/确认链。修改类必须确认，危险类必须强确认，除非未来实现显式、醒目的危险 CLI 开关。

## PTY 规则

每次 PTY 改动审计 fd 泄漏、zombie、process group、signal、timeout、terminal restore、resize、Android NDK 和全屏程序。PTY 字节不得直接写到 ratatui 终端；先过滤清屏、光标移动和 alternate-screen 控制序列。异常路径也必须 wait 子进程并恢复终端。

## 提交规范

建议使用 `feat:`、`fix:`、`refactor:`、`docs:`、`test:`、`chore:`。提交保持单一职责，不混入无关格式化。

`konka` 分支仅允许保留在本地。任何 AI 禁止将该分支 push 或 force-push 到任何远端，也禁止在远端创建或更新同名分支；允许按用户要求在本地创建提交。
