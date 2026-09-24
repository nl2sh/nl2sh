# Architecture

## 产品定位

nl2sh 是以 Android 原生 shell 为一等环境、Termux 为兼容环境的类 Hermes AI Agent，以多轮 Tool Calling 连接模型、本地安全引擎和真实 Android 执行结果。其部署单元是单个 stable Rust Android 可执行文件，配置、日志和发布辅助脚本不是直接 Android 部署的运行时依赖。终端交互以丰富 TUI 为主，同时保留单次 CLI 模式。“类 Hermes”不构成对 Hermes API、插件系统或功能集的兼容承诺。

## 系统整体架构

```text
User
  |
TUI / CLI
  |
Agent Runner ---- LLM Provider
  |                   |
Tool System <---------+
  |
Security Engine
  |
Confirmation Policy
  |
PTY boundary / Pipeline Executor
  |
Root / su Layer
  |
Android Runtime
```

用户输入先成为内部对话消息。Provider 把统一请求映射为 Chat Completions 或 Responses JSON；Tool Call 被转换回内部类型。Agent 只能把 shell tool 交给安全引擎，确认完成后才能调用执行器。stdout、stderr、退出码、超时和错误被编码为 Tool Result，下一轮模型只能依据这些真实结果回答。

TUI 中以 `!` 开头的输入是显式本地命令：去掉前缀后不请求 Provider，而是直接进入同一 `Security → Confirmation → Execution` 边界，并在当前会话界面显示有界实时输出和退出状态。该输入与结果不加入模型上下文；修改、危险、Root 和用户编辑后的命令仍按完整规则重新分类及确认。交互式命令继续通过既有 PTY 挂起、恢复和清屏过滤路径执行。

每个 Agent 任务开始时，统一 runtime 模块根据 `TERMUX_VERSION` 或标准 Termux `PREFIX` 区分直接 Android shell 与 Termux，并由执行器向 system prompt 附加一次低敏感摘要，仅包含环境类型、API level、ABI、shell、当前 UID 与 root/su 能力；探测失败时省略对应字段且不阻断任务。直接 Android shell 使用 `/system/bin/sh`/toybox 的一等保守基线；Termux 兼容模式使用 `$PREFIX/bin/sh`、XDG 路径和包管理基线，但不假定可选包已经安装。摘要只用于命令兼容性提示，不包含型号、序列号、Android ID、IP、账号或应用列表，也不参与安全分类、确认或提权决策。

TUI 在命令运行期间展示有界实时输出，工具轮完成后移除对应临时行并以默认折叠项保存有界结果，F2 只改变显示展开状态。执行捕获、实时 UI、日志事件/文件和模型 Tool Result 分别应用配置上限；截断保留头尾并插入显式标记，模型不会把不完整结果误认为完整。最终回答提示要求按用户语言总结，多项结构化对比优先使用 Markdown 表格。

Provider 通过 SSE 将模型文本增量送入 Agent 的显示 sink；TUI 在当前响应尾部播放有界渐变动画，并在响应完成后立即切换为普通正文。流式工具调用只聚合名称和参数，完整响应解析完成后才进入安全分类与确认链，绝不边接收边执行。最终文本由独立 Markdown 显示层转换成 ratatui `Line`/`Span`；工具、命令和原始输出绕过该层。表格使用 Unicode 显示宽度计算列宽，在内容区域内压缩并换行，窗口过窄时降级为键值列表。解析无法识别的行保持原文，显示转换不回写对话或日志。

TUI 的视觉语义统一由 `UI_DESIGN.md` 约束。实现应以集中式 `Theme`/`Palette` 向 Widget、Markdown、工具结果、确认界面和状态栏提供语义样式，禁止各渲染模块自行硬编码业务颜色。主题只影响显示，不得改变安全评估、确认策略、root 行为、日志内容或 Tool Result；颜色也不得作为风险信息的唯一载体。

内置 Web 页面复用 `UI_DESIGN.md` 的 TrueColor 语义色板，通过 `web/src/style.css` 的 CSS 变量集中供页面及 Markdown 使用；浏览器不需要终端的 ANSI fallback。Web/TUI 的视觉调整均不进入安全或执行边界。

网络、解析或执行错误沿 `anyhow::Result` 返回 UI。LLM 重试只覆盖传输错误、429 和 5xx，并使用有上限的指数退避；401 等配置错误立即返回。Ctrl+C 可取消 HTTP 请求、响应读取和退避。执行超时先给进程组 SIGTERM，短暂等待后给 SIGKILL；Ctrl+C 先给 SIGINT 再升级并回收子进程。Agent TUI 以异步任务驱动 LLM、确认和捕获式命令，保持同一 ratatui frame 并持续刷新历史；只有必须直接占用终端的全屏交互命令才临时离开 alternate screen。交互命令结束后恢复 alternate screen 与鼠标捕获，并清除 ratatui 的旧差分缓存以完整重绘框架。

终端进入 raw mode 和 alternate screen 后由 `TerminalGuard` 持有；TUI 启用鼠标追踪以稳定接收滚轮，宿主终端通过 Shift+拖选保留原生高亮与右键菜单复制。正常退出或错误展开都会恢复鼠标、屏幕、raw mode 和光标。panic hook 做尽力恢复。release 的 `panic=abort` 意味着析构不保证执行，因此生产路径避免 panic；hook 是 abort 前的最后保护。

## Rust 模块

| 模块 | 职责与主要类型 | 输入 / 输出 | 依赖与禁止事项 |
|---|---|---|---|
| `src/config` | `Config`、枚举、loader、wizard、分层校验 | 文件/缺省值/环境 → 可进入 TUI 的运行配置；完整 Provider 配置 → LLM 可用 | 不执行命令，不持有 UI 状态 |
| `src/history` | `HistoryLog`、JSON Lines 事件与安全创建 | 交互事件 → 可刷新诊断日志 | 不记录 provider 凭据，不参与安全决策 |
| `src/tools/file/domain`、`src/file_tools` | `FileToolExecutor`、结构化读取/搜索/补丁与兼容导出 | 任意可访问路径 → 有界结果或待确认 diff | 路径不设工作区边界；写入必须先确认，不调用 shell |
| `src/tools/audio`、原公共路径 | WAV/Raw PCM DSP 与 Jev/通用 LLM 质量判断 | 音频 → Feature JSON、`needs_input` 或评分 | Raw PCM 元数据不得猜测；Jev 已配置时失败不回退；不上传原始音频 |
| `src/tools/android`、原公共路径 | 固定参数诊断、UI bounds 输入、设备聚合、剪贴板与媒体工具 | 严格结构化参数 → 有界证据或待确认动作 | 不提供任意 shell；输入在确认前和执行前重读 UI 树；写入必须确认 |
| `src/tools/memory`、`src/agent_memory` | 私有有界键值便签与原子持久化 | get/list 或确认后的 set/delete/clear → JSON | 不把便签当系统指令；限制键、值和条目数，写操作必须确认 |
| `src/sessions` | `SessionStore`、私有原子快照 | 完整对话 turn → 可恢复会话 | 不序列化配置、凭据、余额或任务审批；工具结果保持有界 |
| `src/llm` | `LlmClient`、`TextDeltaSink`、统一消息/工具类型、两个 HTTP/SSE adapter、retry | `LlmRequest` → 文本增量 + `LlmResponse` | 不进行安全判断或执行工具 |
| `src/provider_metadata` | `ProviderMetadataClient`、Provider 识别、模型列表与上下文元数据归一化 | Provider 配置 → `ModelMetadata` 列表 | 只读网络访问，不记录凭据/原始账户响应，不参与模型推理与安全判断 |
| `src/provider_account` | `ProviderAccountClient`、余额结果归一化 | Provider 凭据 → 可显示余额 | 仅调用公开只读接口；不记录凭据、余额或原始响应，不参与推理、安全或执行 |
| `src/runtime` | `AndroidRuntime`、Termux 标记与 prefix 探测 | 进程环境 → Android shell/Termux | 只提供兼容性信息和 shell/path 选择，不参与安全分类、确认或 root 授权 |
| `src/network` | 统一 rustls HTTP Client、HTTP/SOCKS 代理、认证和绕过策略 | `Config` → `reqwest::Client` | 代理凭据不得进入日志、错误详情或模型上下文；关闭总开关不清理配置 |
| `src/tools/network`、原公共路径 | 公网 HTTP(S) 有界读取、确认后 POST/下载与 TLS 诊断 | URL/主机 → 有界正文、文件或证书信息 | 禁止重定向、URL 凭据、本机/私网目标和任意 header；POST、下载均须确认 |
| `src/web_ui` | Axum 0.8 HTTP/SSE/WebSocket、多 Agent 会话、LLM 自动标题、快捷运行设置、浏览器审批、rust-embed 资源 | serde JSON + SSE + WebSocket → 独立 Agent Runner、结构化显示条目、原子配置文件 | 无登录，优先监听 IPv4 9999（占用时使用可用端口）；每会话独立锁和审批通道；WebSocket 终端仍走安全分类和确认；请求有大小上限；不直接执行模型输出 |
| `src/tools/ui`、`src/ui_tools` | UIAutomator 控件树、焦点窗口、截图及模型图片附件 | 当前界面/本地图片 → 有界节点、截图或临时多模态内容 | 固定探测静默执行；超限图片有界缩放；截图写入必须确认；附件不持久化 |
| `src/update` | GitHub Release 发现、版本/ABI 选择、SHA-256 校验与原子替换 | Release 元数据与 Android ABI → 已校验的新可执行文件 | 不执行模型输出；不接受跨 ABI 或无校验资产 |
| `src/agent` | `AgentRunner`、上下文完整交互单元、`Confirmer` | 用户任务 → Tool Loop / 最终文本 | 不得绕过 security 和 confirmer |
| `src/tools` | `Tool`、显式 `ToolRegistry`、风险/能力元数据、派生 schema 与 `PreparedToolCall` | 模型调用 → 预备动作 → 审批后有界结果 | 只用本地元数据定风险；修改预览必须在统一确认入口批准后执行 |
| `crates/nl2sh-tool-macros` | 编译期 `#[tool]` 生成适配器与元数据 | 注解函数 → Rust Tool 实现 | 只在构建主机运行；不自动注册或授予执行权限 |
| `src/security` | normalize、side-effect 分类、内置/自定义规则、`SecurityAssessment` | 原始命令 → 风险和确认要求 | 不依赖 TUI、LLM 或执行器 |
| `src/shell` | `CommandExecutor`、root invocation、process group、pipeline/PTY 边界 | 已批准命令 → `ExecutionResult` | 不自行降低风险或批准命令 |
| `src/tui` | terminal guard、session 状态机、独立 output/history 生命周期、事件、输入、`@` 文件候选、中英文文案、ratatui 渲染 | key/mouse event → 用户输入 | 不解析 OpenAI JSON，不直接执行；启动帮助不进入模型上下文 |
| `src/file_references` | 识别用户输入中 `@` 后最长的已存在路径前缀并解析为绝对路径 | 原始用户文本 → 保留原文并附加有界路径提示 | 不读取文件内容、不执行命令；内容仍由结构化文件工具按上限读取 |
| `src/ima` | 腾讯 ima 知识库只读发现、搜索与原文读取 | 独立 Client ID/API Key → 有界知识库结果 | 强制直连且不使用代理；不提供任何写接口，不泄露长期凭据、临时 header 或签名 URL |

公共 trait 允许测试以 mock 替换网络、执行、确认和 root 探测。依赖方向保持 `UI → Agent → abstractions`，security 与 shell 彼此通过调用参数协作，无循环依赖。

模型可见工具由 Registry 显式注册，ima 按本地 `Capability` 条件暴露；参数类型通过 `schemars::JsonSchema` 派生定义。`Tool::prepare` 解析参数并构造预览与待执行动作；工具元数据声明风险下限，剪贴板、媒体和便签按实际参数升高单次风险。Agent 统一根据本地风险决定确认/强确认，再调用 `PreparedExecution::execute`。补丁和下载在确认前只准备 diff 或数据，截图、HTTP POST、输入注入与设备控制必须先显示预览；输入注入执行前重新校验当前 UI bounds。Shell 命令继续逐次经过原安全分类、编辑重评估、Root 与 PTY 回收路径。音频缺参问答与分析缓存、截图附件以及 Web 会话审批沿用既有行为。`define_tool!` 和构建期 `#[tool(...)]` 均只生成适配器，注册和权限仍需显式决定。

Web 前端源码和 npm lockfile 保存在 `web/`；Cargo 的 `build.rs` 先把源码复制到 `OUT_DIR`，在副本中执行 `npm ci` 和 `npm run build`，再由 `rust-embed` 把产物编入单一可执行文件。构建需要 Node.js/npm，生成目录不进入版本控制或发布源码包；Android 运行时不依赖 Node.js。TUR 构建使用 Termux 提供的主机 Node 工具。

## Agent 执行流程

只读操作在 balanced/risk_only 下自动执行；普通修改必须确认；Dangerous/Critical 需要二次确认。root 只是执行属性，不改变分类。审批界面提供固定编号与快捷键，可仅允许本次、拒绝、编辑或选择执行模式；对非 Root、非强确认且最高为 Mutating 的命令，还可在当前 Agent 任务内记住完整命令的精确许可。该许可不持久化、不按前缀匹配，Runner 会在每次复用前重新检查当前评估仍满足条件。拒绝、失败或超时都会生成明确的失败 Tool Result。

审批面板按命令或 diff 的 Unicode 显示宽度和实际换行高度动态调整，最大范围受终端与输入区约束。超高内容在独立正文区通过滚轮或 PageUp/PageDown 浏览，编号选择、强确认和编辑输入固定在底部；布局与滚动不改变风险等级或确认语义。

Agent 文件操作优先使用 `read_file`、`list_dir`、`search_text` 和 `apply_patch`，不依赖设备端 `sed` 或 shell 重定向。路径不设工作区沙箱：允许绝对路径、父目录组件并跟随符号链接；读取、遍历、匹配和文件大小仍有硬上限，最终 Tool Result 继续使用配置的模型输出上限。`apply_patch` 在内存中验证唯一替换并生成 diff，确认前不打开目标进行写入，每次调用均单独确认，批准后才原子替换。

只读工具返回结构化 `needs_input` 时，Runner 可通过独立于安全审批的用户问答接口请求缺失事实。TUI 在同一 frame 内显示支持候选选择和自定义输入的多字段窗口，非 TUI 模式使用终端文本回退；等待用户回答的时间不计入活跃任务时长。当前 Raw PCM 分析会用该接口收集采样率、声道数和采样格式，并将答案直接合并到原工具参数后本地重试，不经过模型改写或猜测。取消只保留 `needs_input` 结果，不批准命令、文件写入或 root 操作，也不改变既有 `Security → Confirmation → Execution` 边界。

同一 Agent 任务内完成的音频分析按原始路径保存结构化结果；质量判断优先按路径引用该结果，避免模型复制、删减或改写 DSP 字段。只有完整的 `status=ok` 分析能够进入缓存，缓存不跨任务持久化。

TUI 输入中的 `@路径` 提供本地文件/目录候选，支持相对路径、绝对路径、`~/`、`./` 与 `../`，Up/Down 选择并以 Enter 或 Tab 补全；Right 保持普通光标右移。提交时按“最长已存在路径前缀”解析，因此 `@test.txt写的是什么内容` 不要求路径后有空格；解析结果只向 Agent 附加绝对路径，实际内容仍由有界结构化文件工具读取。路径解析不会执行文件内容，也不会改变 shell 安全分类、确认或 root 策略。

Web 输入框通过有界只读接口复用同一文件候选逻辑，按光标位置补全当前 `@路径`；发送时沿用同一引用解析提示。右上角工具目录由当前配置和显式工具注册表生成，仅返回名称与简介并在浏览器本地搜索，不授予额外工具能力。

Runner 以一次完整模型判断及其零个或多个工具结果为一个 Step，并独立维护 Step、Tool Call、活跃运行时间、连续停滞与重复动作预算。Fast/Normal/Deep 预设分别为 20/40/10 分钟、50/100/30 分钟和 100/200/60 分钟；显式字段可逐项覆盖预设，但 `hard_max_agent_steps` 始终取更小值。模型请求受剩余任务时限约束；命令执行沿用执行器自身的 TERM/KILL/wait 超时链，并在其安全回收后立即检查任务时限，避免取消 Future 造成 PTY fd 或子进程泄漏。等待安全确认的时间不计入活跃时间。相同规范化命令连续得到相同结果三次后，下一次会在执行边界前拒绝；连续无新证据达到阈值时注入强制重新规划提示，达到终止阈值时停止。80%/90% Step 水位会要求模型收敛。所有预算检查均位于安全链之外且不能批准命令、降低风险、跳过确认或改变 root 策略。

每轮可能处理模型返回的多个调用，完成后把结果加入下一请求。上下文按完整 turn 删除最旧单元，system message 始终保留；当 Provider 报告的实际输入 Token 超过已知窗口的输入安全水位时，Runner 按观测比例淘汰最旧完整历史，并把淘汰数同步给 TUI 会话状态，当前交互和 Tool Round 不拆分。Token 预算只能提前停止或减少历史，不能放大任务预算或绕过确认链。

Command 模式使用严格 system prompt，仅接受第一条清理后的非空命令，处理 code fence 和 `Command:` 前缀，不尝试拼装多条候选。

## PTY 与进程

设计边界为 `CommandExecutor → pty/pipeline → process`。pipeline 分别捕获 stdout/stderr，子进程成为独立进程组，超时和信号针对组发送，随后 `wait` 防止 zombie。PTY 中 stdout/stderr 本来会合并；任意输出在进入 ratatui 前必须过滤破坏屏幕状态的 ANSI 控制序列。

`pty` 使用 `nix::openpty` 创建 master/slave，通过 `setsid` 与 `TIOCSCTTY` 让 slave 成为 controlling terminal，并把三个标准流连接到 slave。master 以非阻塞方式读取，因此 PTY 下 stdout/stderr 合并；结果进入 Agent 前通过保守 ANSI filter。交互模式启用本地 raw mode，轮询 stdin 写入 master，把 master 原始输出写向本地终端，并用 `TIOCGWINSZ/TIOCSWINSZ` 同步尺寸。退出、超时或 Ctrl+C 后恢复本地终端并 wait 子进程。pipeline 是无 PTY fallback，保留分离 stdout/stderr。未使用 portable-pty；Android Bionic 路径已完成交叉编译及 root/非 root、超时和全屏交互真机验证，仍需持续关注未覆盖设备与终端实现的兼容差异。

## Android root

```text
nl2sh
 |
 +-- uid == 0 --------> Android: /system/bin/sh -c
 |
 +-- uid != 0
       +-- normal ----> current-user shell
       +-- su exists -> su -c <single argv command>
```

`auto` 只在安全层/规则判断需要 root 时提升；`normal` 永不提升；`root` 要求 root 或可用 su。su 不可用、授权失败或命令失败均不静默降级。整个原始命令作为独立 argv 传给 `su -c`，因此引号、管道、重定向和换行不会经过 nl2sh 的字符串拼接。root 修改和危险命令仍遵循确认策略，提示包含 ROOT。

## LLM Provider

LLM、模型发现、Ollama 元数据和余额查询必须通过 `src/network` 构造客户端，以保证代理开关、认证、绕过和超时一致。显式关闭代理时调用 `no_proxy`，不隐式继承宿主环境；HTTP、SOCKS5 与 SOCKS5H 均保持 Provider HTTPS 的端到端 TLS。代理配置弹窗只展示密码掩码，保存后原子替换配置并重建客户端。

新配置默认选择 OpenRouter 的 OpenAI-compatible API，模型为 `openrouter/free`；OpenRouter 与 OpenAI 公共端点均要求有效 API Key 才视为 Provider 已配置。OpenRouter 复用统一 HTTP/流式 adapter 和模型列表归一化，不引入供应商 JSON 到 Agent，也不改变安全确认或执行边界。

ima 是这一通用 Provider 代理策略的显式例外：按产品边界使用独立 rustls `Client`，始终调用 `no_proxy` 且禁止重定向。Agent 只在完整配置 Client ID/API Key 且启用时暴露 `ima_list_knowledge_bases`、`ima_search`、`ima_read`。搜索结果与正文受硬上限约束；原文 URL 只接受 HTTPS 的 ima、微信文章或腾讯 COS 白名单域名，临时请求 header 仅用于该次下载，不进入 Tool Result、日志或会话。远程知识内容按不可信用户数据处理，不能提升指令优先级。

`LlmClient::complete` 是业务唯一入口。Chat adapter 映射 messages、function tools、tool_calls；Responses adapter 映射 input、function tool、function_call 和 function_call_output。默认 `auto` 协议在首次真实请求优先尝试 Responses，仅在 404/405、明确的端点不支持或尚未产生内容的响应结构不匹配时回退 Chat Completions，并在当前 client 生命周期缓存成功协议；鉴权、限流、5xx、超时和已产生流式内容后的错误不得触发协议切换。显式协议配置与 CLI 覆盖仍强制使用指定 adapter。`ConversationItem` 把文本与完整 `ToolRound` 按真实顺序保存，因此截断只删除完整 user/tool/assistant turn，不会产生孤立 tool output。统一类型还包括 `ConversationMessage`、`ToolDefinition`、`ToolCall`、`ToolResult`、`Usage`、`FinishReason`。新增 provider 只需实现 trait，不能把供应商 JSON 泄漏到 Agent。

## 安全架构

```text
Raw Command
  ↓
Normalize whitespace / escapes
  ↓
Token and compound-operator heuristics
  ↓
Classify side effects
  ↓
Regex built-ins + custom rules
  ↓
SecurityAssessment
  ↓
Confirmation policy
```

检测不是完整 POSIX parser，但检查重定向、管道上下文、命令替换、`sh -c`/`su -c` 包装和典型副作用选项。内置规则始终存在，自定义规则只会提高最高风险。未来应使用专门 shell AST parser 替换启发式层，并以回归语料保证不能降低既有风险。

## 扩展

- 新 Provider：实现 `LlmClient` 和协议 adapter。
- 新 Tool：增加内部参数类型和 tool policy，所有有副作用 tool 必须进入 security/confirmation。
- 新安全规则：配置 regex 或在 builtins 增加规范化规则和测试。
- 新执行环境：实现 `CommandExecutor`，保持结果和取消语义。
- 新配置来源：在 loader 合并并记录优先级，再统一 validate。
- 新 UI：仅依赖 Agent/trait API，不访问 provider JSON。
- 新 shell parser：输出规范化 command segments，保持 `SecurityAssessment` API。

## 更新与设置

启动更新检查是只读后台任务，失败不阻塞 TUI；仅在发现更高版本且匹配本机 ABI 时提示。立即更新会先恢复终端，再下载裸二进制及 SHA-256，校验通过后在当前目录原子替换。跳过版本写入配置，普通暂不更新不持久化。

Termux TUR/APT 构建不启用 `self-update` Cargo feature：不执行启动更新检查，也不替换包管理器拥有的 `$PREFIX/bin/nl2sh`，手工更新入口只提示 `pkg upgrade nl2sh`。TUR 配方从固定 tag 和 SHA-256 源码归档通过 `termux_setup_rust` 构建。直接 Android 发布仍默认启用校验后自更新。Termux 默认配置遵循 XDG config 目录，日志与会话遵循 XDG state 目录；非 Termux直接部署和显式配置路径继续保持配置相邻状态。

`/config` 与别名 `/setting` 使用单一 TUI 设置面板承载服务、模型与 Agent、执行与安全、界面和网络分类；其他分散配置命令不再暴露。两者属于严格本地命令，打开面板后不得进入模型上下文。服务分类与旧向导共享内置 Provider 预设，选择预设只联动 Endpoint，保留 API Key、模型和协议，自定义 Endpoint 显示为 Custom。Tab/Shift+Tab 只切分类，Up/Down 只移动字段，Left/Right 只调整当前值；保存后主循环重新加载配置和客户端。界面分类独立控制佛像与小火车 ASCII Art，并提供显式的日志清除操作；日志清除仅截断当前 JSONL 文件并恢复后续记录能力，不清理当前会话或改变安全链。

Agent TUI 在输入分发边界保留 `/` 前缀命名空间：所有去除前导空白后以 `/` 开头的输入均为本地命令，已知命令执行本地动作，未知命令只产生本地提示。任何斜杠命令都不得写入模型用户历史或调用 LLM。

`/new` 清空当前内存对话、模型上下文和输入历史并分配新的默认会话名，但不删除已保存快照或审计日志。未知单词型斜杠命令可提示编辑距离接近的已知命令，提示不得自动执行候选。

每个已完成 Agent turn 自动保存到状态目录的 `sessions/` 私有目录，文件以 `0600` 原子替换。`/sessions` 打开按更新时间倒序排列的最近会话列表，可输入序号或用 Up/Down 与 Enter 选择恢复；兼容的命名恢复、重命名和删除子命令仍保留。恢复只装载完整 turn，并重新应用上下文轮数和 Tool Result 上限。会话文档仅包含 provider-neutral 对话项，不包含 `Config`，因此 API Key、代理密码、余额和仅当前任务有效的审批许可不会落盘。

设置编辑器在单次打开期间分别持有 Ollama 与 Custom 的 Endpoint 草稿；离开对应 Provider 前保存当前值，切回时恢复。其他内置 Provider 仍使用固定预设地址，编辑其地址会转入 Custom。

`/shell` 是显式的用户直控边界：它暂停 alternate-screen TUI，直接 Android shell 使用 `/system/bin/sh -i`，Termux 使用 `$PREFIX/bin/sh -i`，开发主机条件使用 `/bin/sh -i`。其中输入直接属于用户而非 LLM 输出，不进入模型、安全分类或审计内容；键入 `exit` 或发送 EOF 后必须 wait 子 shell、恢复 raw mode、鼠标捕获和 alternate screen，并显式清除 ratatui 差分缓存后完整重绘原会话。
