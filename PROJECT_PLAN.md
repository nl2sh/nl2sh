# Project Plan

状态以 2026-08-22 的工作区和实际验证为准；0.2.0 为当前发布版本；未完成项不会机械勾选。

## Phase 0 项目初始化与工程基线 — 完成

- [x] 确立 Android shell 版类 Hermes AI Agent 的产品定位，以单个可执行文件和丰富 TUI 为核心交付形态。
- [x] 创建 Cargo 工程、模块树、release profile、gitignore。
- [x] 建立 stable Rust 2021 和 rustls-only 依赖基线。
- [x] 本地 build/test/release 工程基线。

文件：`Cargo.toml`、`src/lib.rs`。依赖：无。输出：可检查工程。验收：本地 build/test。风险：Android target 尚未验证。

## Phase 1 CLI 与配置系统 — 完成

- [x] clap 参数、默认路径、符号链接处理、TOML 默认值和校验。
- [x] 初始化向导、常用 API 服务商方向键选择、API Key 可见输入、不覆盖文件和环境覆盖。
- [x] endpoint/model/api-type CLI 覆盖，并在覆盖后统一校验。

文件：`src/cli.rs`、`src/config`。依赖：Phase 0。验收：配置测试。风险：Android 无 controlling tty 时的向导体验。

## Phase 2 TUI 基础框架 — 完成

- [x] ratatui/crossterm 三区域界面和 RAII terminal guard。
- [x] 基础输入、Enter、Ctrl+C、Ctrl+Q。
- [x] 完整历史、滚动、ASCII symbol set 和终端 resize。
- [x] 单-frame Agent 后台任务、实时状态/输出和确认弹窗；全屏交互命令按需挂起并恢复。
- [x] 集中式深色 Theme/Palette、TrueColor/ANSI 256 fallback 和跨组件语义配色。

输出：可启动输入 TUI。风险：adb 终端宽度和 Emoji。

## Phase 3 LLM Provider 抽象 — 完成

- [x] 统一 trait、请求、响应、消息、tool、usage 和 finish reason。
- [x] Agent 与协议 JSON 解耦。

验收：mock trait Agent 测试。

## Phase 4 Chat Completions 与 Responses API — 基本完成

- [x] 两个 HTTP adapter 和 function tool 映射。
- [x] rustls、认证省略、timeout、可重试状态和退避。
- [x] Ctrl+C 取消请求/退避和增量 command output sink。
- [ ] 更多兼容厂商响应变体。

验收：wiremock 文本/tool/错误测试。风险：兼容 endpoint 方言。

## Phase 5 Agent 与 Tool Calling — 进行中

- [x] 默认 Tool Calling、多轮、最大 steps、结果回传、上下文 turn 上限。
- [x] Command Generation 模式及输出清理。
- [x] 同一轮多个调用逐项确认、编辑后重新分类、Agent cancellation。
- [x] 编号/快捷键审批列表，以及仅限当前任务、完整命令精确匹配的安全许可。

## Phase 6 安全分类与确认策略 — 基本完成

- [x] 四级风险、内置危险规则、自定义规则、确认与二次确认。
- [x] 覆盖要求中的安全测试矩阵。
- [x] 无 TTY 强制拒绝修改/危险命令，扩大包装、替换、转义测试语料。
- [ ] 未来以完整 shell AST 取代启发式 parser。

风险：启发式 parser 不可能覆盖全部 shell 语法。

## Phase 7 PTY 执行器 — 基本完成

- [x] 抽象边界、pipeline fallback、进程组、超时 TERM/KILL、wait。
- [x] openpty/setsid/controlling terminal、非阻塞 master、resize、ANSI 过滤。
- [x] Android NDK r28c、API 26、AArch64/ARMv7 release 交叉编译。
- [x] API 34 ARMv7 真机 Agent、PTY、TUI 基础 smoke test。
- [ ] 真机 root、修改确认、超时及全屏交互验证。

验收：Unix smoke 与 Android 真机。风险：Bionic PTY 差异。

## Phase 8 Android root 与 su — 基本完成

- [x] geteuid、su probe、Root/SuAvailable/Normal、auto/normal/root。
- [x] 参数化 `su -c` 和 mock root 测试。
- [ ] Android 主流 Magisk/su 实现真机验证。

## Phase 9 交互式命令终端切换 — 进行中

- [x] 已知交互命令检测、双向 PTY、信号/resize、raw mode RAII。
- [ ] Android 全屏程序真机验证及 TUI 内完整状态回放。

## Phase 10 测试、文档和 Android 验证 — 进行中

- [x] 配置、安全、root、LLM mock、Agent loop 测试和核心文档。
- [x] timeout、失败、Agent interruption 和 PTY smoke 覆盖。
- [x] 隔离子进程 OS SIGINT 注入与 PTY 子进程回收测试。
- [x] 真实伪终端中的单轮 Agent TUI 保活与 Ctrl+Q 退出测试。
- [x] NDK r28c/API 26 cross-build。
- [x] API 34 ARMv7 Android device 基础 smoke。
- [ ] 多设备 root/交互完整 smoke。

## Phase 11 0.1.0 发布基线 — 完成

- [x] CI release matrix：tag 触发并行构建 AArch64/ARMv7 Android release，合并为双 ABI 单一压缩包，附带自动选设备/ABI 的 Linux 与 Windows BAT 启动脚本及校验和，发布 GitHub Release。
- [x] Linux/Windows 本地 release 打包脚本：构建双 ABI，并生成与 GitHub Release 相同结构的统一 ZIP 和 SHA256 校验文件。
- [x] MIT license、0.1.0 changelog 基线和 Android 双 ABI 发布工作流。

## Phase 12 0.2.0 稳定性迭代 — 完成

- [x] 为实时 UI、工具结果、模型 Tool Result、日志事件和日志文件增加显式截断的资源上限。
- [x] 拆出 TUI 输出与历史生命周期模块，降低 session 控制器职责。
- [x] 增加 `/help` 本地帮助和 `/clear` 当前会话清理命令。
- [x] 缺失配置时直接进入 TUI，并提供 `/config`、`/provider`、`/model` 分层配置入口。
- [x] 在启动欢迎页和 `/help` 显示项目支持、在线赞赏链接与纯文本终端祝福图，不引入图片或二维码渲染。
- [x] 增加 `/exit` 安全退出命令和每任务一次的低敏感 Android 运行环境摘要。
- [ ] 增加普通 PR CI 质量门禁。
- [ ] 完成 root/非 root 真机安全矩阵。
- [x] 准备 0.2.0 版本、双 ABI 发布工作流与本地交叉编译验证；标签发布状态由 GitHub Actions 最终结果确认。

## Phase 13 Provider 可观测性与发现 — 完成

- [x] 跨 Agent 工具步骤累计输入/输出 Token，并在 TUI 展示任务总计。
- [x] `/models` 在线模型发现、手工回退、Provider 元数据抽象及上下文窗口覆盖/占用估算。
- [x] OpenAI、DeepSeek、SiliconFlow 与 Ollama 模型发现适配。
- [x] `/balance` 通过公开 Bearer Token 接口查询 DeepSeek 与 SiliconFlow 余额；结果不进入日志或模型上下文，其他 Provider 明确降级为不支持。
- [x] 支持余额的 Provider 在 TUI 定时刷新并常驻显示；按模型窗口、输出预留和实际输入 Token 动态收缩完整历史轮次。
- [x] `/proxy` TUI 弹窗配置 HTTP/SOCKS 代理；统一所有 Provider 网络客户端，总开关关闭时保留代理字段。

## Phase 14 自更新与统一设置 — 完成

- [x] `update` 命令按 Android ABI 获取最新 GitHub Release，校验 SHA-256 后原子替换可执行文件。
- [x] 每次 Agent TUI 启动后台检查更新，并提供立即更新、暂不更新和跳过此版本。
- [x] 配置命令统一进入分类 Tab 设置面板；最大 Agent 步数和上下文轮次显示推荐值 24/16。
