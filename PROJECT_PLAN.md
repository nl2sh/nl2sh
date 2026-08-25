# Project Plan

状态以 2026-08-24 的工作区和实际验证为准；0.2.0 为当前发布版本；未完成项不会机械勾选。

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
- [x] 默认自动协商 Responses/Chat Completions，仅对协议不匹配安全回退并缓存结果。
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

## Phase 7 PTY 执行器 — 完成

- [x] 抽象边界、pipeline fallback、进程组、超时 TERM/KILL、wait。
- [x] openpty/setsid/controlling terminal、非阻塞 master、resize、ANSI 过滤。
- [x] Android NDK r28c、API 26、AArch64/ARMv7 release 交叉编译。
- [x] API 34 ARMv7 真机 Agent、PTY、TUI 基础 smoke test。
- [x] 真机 root、修改确认、超时及全屏交互验证。

验收：Unix smoke 与 Android 真机。风险：Bionic PTY 差异。

## Phase 8 Android root 与 su — 完成

- [x] geteuid、su probe、Root/SuAvailable/Normal、auto/normal/root。
- [x] 参数化 `su -c` 和 mock root 测试。
- [x] Android 主流 Magisk/su 实现真机验证。

## Phase 9 交互式命令终端切换 — 完成

- [x] 已知交互命令检测、双向 PTY、信号/resize、raw mode RAII。
- [x] Android 全屏程序真机验证及 TUI 内完整状态回放。

## Phase 10 测试、文档和 Android 验证 — 完成

- [x] 配置、安全、root、LLM mock、Agent loop 测试和核心文档。
- [x] timeout、失败、Agent interruption 和 PTY smoke 覆盖。
- [x] 隔离子进程 OS SIGINT 注入与 PTY 子进程回收测试。
- [x] 真实伪终端中的单轮 Agent TUI 保活与 Ctrl+Q 退出测试。
- [x] NDK r28c/API 26 cross-build。
- [x] API 34 ARMv7 Android device 基础 smoke。
- [x] 多设备 root/交互完整 smoke。

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
- [x] 完成 root/非 root 真机安全矩阵。
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

## Phase 15 Agent 任务运行预算 — 基本完成

- [x] 分离 Step、Tool Call 和活跃任务时长计数，并提供 Fast/Normal/Deep 预算预设与系统硬 Step 上限。
- [x] 对 LLM 请求应用任务剩余时限；命令由执行器完整回收后立即检查任务时限，等待安全确认不计入活跃时长。
- [x] 规范化 Shell 命令并以实际结果指纹识别重复动作；达到阈值后在再次执行前阻止。
- [x] 连续无进展触发强制重新规划与终止，80%/90% Step 水位提示模型收敛。
- [x] Agent 结果公开步骤、工具、时长、停滞、重规划和限制原因统计，TUI 完成状态显示步骤/工具/时长摘要。
- [ ] 增加运行中逐 Step 推送、智能结果相似度、文件变化检测和持久化任务指标。

## Phase 16 结构化文件工具 — 完成

- [x] 新增 `read_file`、`list_dir`、`search_text` 和 `apply_patch` Tool schema 与本地执行边界。
- [x] 路径不设工作区边界，允许绝对路径、父目录和符号链接；限制文件大小、目录遍历与搜索匹配数。
- [x] `apply_patch` 仅接受唯一文本替换，写入前展示 diff 并经过现有确认器，批准后原子替换。
- [x] Tool Result 继续应用捕获与模型上下文大小限制，不依赖 Android 设备端编辑命令。

## Phase 17 会话保存与恢复 — 完成

- [x] 已完成 Agent turn 自动保存到配置目录旁的私有 `sessions/`，支持 `/sessions` 列表、恢复、重命名和删除。
- [x] 保存 provider-neutral 完整 turn，恢复时重新应用上下文轮数和工具结果上限。
- [x] 会话文件不包含 Provider 配置、API Key、代理凭据、余额或任务级审批许可。

## Phase 18 长内容审批布局 — 完成

- [x] 审批弹窗按 Unicode 内容宽度和实际换行高度动态调整，并限制在输入区上方的终端可用范围内。
- [x] 超高命令或 diff 使用滚轮、PageUp/PageDown 滚动正文，审批选项与强确认/编辑输入固定可见。
- [x] 保持 Up/Down 审批选择、风险样式、强确认和终端恢复语义不变。

## Phase 19 `@` 文件与目录引用 — 完成

- [x] 输入 `@` 路径时显示有界候选，支持 Up/Down 选择与 Enter/Tab 补全。
- [x] 支持相对、绝对、`@~`、`@/`、`@.` 与父目录路径，并以 `/` 标识可继续下钻的目录。
- [x] 提交时识别最长已存在路径前缀，支持 `@test.txt写的是什么内容` 等无空格自然语言。
- [x] 只向 Agent 提供解析后的绝对路径，文件内容继续通过有界结构化工具读取，不改变安全确认链。

## Phase 20 ima 只读知识库连接器 — 完成

- [x] 新增独立无代理 rustls 客户端和 Client ID/API Key 配置，凭据不进入日志、会话或模型上下文。
- [x] 按配置动态暴露知识库发现、搜索和原文读取 Tool，不包含任何写入端点。
- [x] 支持 `get_media_info` 后读取 ima 笔记正文或受控临时 URL，限制响应大小、HTTPS 来源和重定向。
- [x] 增加协议 mock、凭据脱敏、来源白名单及显式凭据只读 smoke 测试。
