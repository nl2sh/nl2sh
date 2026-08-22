# Architecture

## 产品定位

nl2sh 是 Android 原生 shell 版的类 Hermes AI Agent，以多轮 Tool Calling 连接模型、本地安全引擎和真实 Android 执行结果。其部署单元是单个 stable Rust Android 可执行文件，配置、日志和发布辅助脚本不是运行时程序依赖。终端交互以丰富 TUI 为主，同时保留单次 CLI 模式。“类 Hermes”不构成对 Hermes API、插件系统或功能集的兼容承诺。

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

每个 Agent 任务开始时，Android 执行器会向 system prompt 附加一次低敏感运行环境摘要，仅包含 API level、ABI、`/system/bin/sh`、当前 UID 与 root/su 能力；探测失败时省略对应字段且不阻断任务。摘要只用于命令兼容性提示，不包含型号、序列号、Android ID、IP、账号或应用列表，也不参与安全分类、确认或提权决策。易变的内存、存储和网络状态仍必须通过工具按需查询。

TUI 在命令运行期间展示有界实时输出，工具轮完成后移除对应临时行并以默认折叠项保存有界结果，F2 只改变显示展开状态。执行捕获、实时 UI、日志事件/文件和模型 Tool Result 分别应用配置上限；截断保留头尾并插入显式标记，模型不会把不完整结果误认为完整。最终回答提示要求按用户语言总结，多项结构化对比优先使用 Markdown 表格。

Agent 最终文本由独立 Markdown 显示层转换成 ratatui `Line`/`Span`；工具、命令和原始输出绕过该层。表格使用 Unicode 显示宽度计算列宽，在内容区域内压缩并换行，窗口过窄时降级为键值列表。解析无法识别的行保持原文，显示转换不回写对话或日志。

TUI 的视觉语义统一由 `UI_DESIGN.md` 约束。实现应以集中式 `Theme`/`Palette` 向 Widget、Markdown、工具结果、确认界面和状态栏提供语义样式，禁止各渲染模块自行硬编码业务颜色。主题只影响显示，不得改变安全评估、确认策略、root 行为、日志内容或 Tool Result；颜色也不得作为风险信息的唯一载体。

网络、解析或执行错误沿 `anyhow::Result` 返回 UI。LLM 重试只覆盖传输错误、429 和 5xx，并使用有上限的指数退避；401 等配置错误立即返回。Ctrl+C 可取消 HTTP 请求、响应读取和退避。执行超时先给进程组 SIGTERM，短暂等待后给 SIGKILL；Ctrl+C 先给 SIGINT 再升级并回收子进程。Agent TUI 以异步任务驱动 LLM、确认和捕获式命令，保持同一 ratatui frame 并持续刷新历史；只有必须直接占用终端的全屏交互命令才临时离开 alternate screen。交互命令结束后恢复 alternate screen 与鼠标捕获，并清除 ratatui 的旧差分缓存以完整重绘框架。

无配置文件的 TUI 启动会先以短超时异步探测 KONKA 内部 Chat Completions 地址；收到任意 HTTP 响应后以排他创建和 `0600` 权限写入使用 `deepseek-v4-flash-0731` 模型与 Responses 协议的内置体验配置。配置文件已存在时完全跳过探测和写入，网络不可达时保持普通的未配置 TUI 流程。该启动便利逻辑只选择 Provider，不改变安全分类、确认、root 或执行策略。

终端进入 raw mode 和 alternate screen 后由 `TerminalGuard` 持有；TUI 启用鼠标追踪以稳定接收滚轮，宿主终端通过 Shift+拖选保留原生高亮与右键菜单复制。正常退出或错误展开都会恢复鼠标、屏幕、raw mode 和光标。panic hook 做尽力恢复。release 的 `panic=abort` 意味着析构不保证执行，因此生产路径避免 panic；hook 是 abort 前的最后保护。

## Rust 模块

| 模块 | 职责与主要类型 | 输入 / 输出 | 依赖与禁止事项 |
|---|---|---|---|
| `src/config` | `Config`、枚举、loader、wizard、分层校验 | 文件/缺省值/环境 → 可进入 TUI 的运行配置；完整 Provider 配置 → LLM 可用 | 不执行命令，不持有 UI 状态 |
| `src/history` | `HistoryLog`、JSON Lines 事件与安全创建 | 交互事件 → 可刷新诊断日志 | 不记录 provider 凭据，不参与安全决策 |
| `src/llm` | `LlmClient`、统一消息/工具类型、两个 HTTP adapter、retry | `LlmRequest` → `LlmResponse` | 不进行安全判断或执行工具 |
| `src/agent` | `AgentRunner`、上下文完整交互单元、工具 schema、`Confirmer` | 用户任务 → Tool Loop / 最终文本 | 不得绕过 security 和 confirmer |
| `src/security` | normalize、side-effect 分类、内置/自定义规则、`SecurityAssessment` | 原始命令 → 风险和确认要求 | 不依赖 TUI、LLM 或执行器 |
| `src/shell` | `CommandExecutor`、root invocation、process group、pipeline/PTY 边界 | 已批准命令 → `ExecutionResult` | 不自行降低风险或批准命令 |
| `src/tui` | terminal guard、session 状态机、独立 output/history 生命周期、事件、输入、中英文文案、ratatui 渲染 | key/mouse event → 用户输入 | 不解析 OpenAI JSON，不直接执行；启动帮助不进入模型上下文 |

公共 trait 允许测试以 mock 替换网络、执行、确认和 root 探测。依赖方向保持 `UI → Agent → abstractions`，security 与 shell 彼此通过调用参数协作，无循环依赖。

## Agent 执行流程

只读操作在 balanced/risk_only 下自动执行；普通修改必须确认；Dangerous/Critical 需要二次确认。root 只是执行属性，不改变分类。审批界面提供固定编号与快捷键，可仅允许本次、拒绝、编辑或选择执行模式；对非 Root、非强确认且最高为 Mutating 的命令，还可在当前 Agent 任务内记住完整命令的精确许可。该许可不持久化、不按前缀匹配，Runner 会在每次复用前重新检查当前评估仍满足条件。拒绝、失败或超时都会生成明确的失败 Tool Result。每轮可能处理模型返回的多个调用，完成后把结果加入下一请求；达到 `max_agent_steps` 立即停止并返回原因。上下文按完整 turn 删除最旧单元，system message 始终保留。

Command 模式使用严格 system prompt，仅接受第一条清理后的非空命令，处理 code fence 和 `Command:` 前缀，不尝试拼装多条候选。

## PTY 与进程

设计边界为 `CommandExecutor → pty/pipeline → process`。pipeline 分别捕获 stdout/stderr，子进程成为独立进程组，超时和信号针对组发送，随后 `wait` 防止 zombie。PTY 中 stdout/stderr 本来会合并；任意输出在进入 ratatui 前必须过滤破坏屏幕状态的 ANSI 控制序列。

`pty` 使用 `nix::openpty` 创建 master/slave，通过 `setsid` 与 `TIOCSCTTY` 让 slave 成为 controlling terminal，并把三个标准流连接到 slave。master 以非阻塞方式读取，因此 PTY 下 stdout/stderr 合并；结果进入 Agent 前通过保守 ANSI filter。交互模式启用本地 raw mode，轮询 stdin 写入 master，把 master 原始输出写向本地终端，并用 `TIOCGWINSZ/TIOCSWINSZ` 同步尺寸。退出、超时或 Ctrl+C 后恢复本地终端并 wait 子进程。pipeline 是无 PTY fallback，保留分离 stdout/stderr。未使用 portable-pty；Android Bionic 兼容仍需交叉编译与真机验证。

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

`LlmClient::complete` 是业务唯一入口。Chat adapter 映射 messages、function tools、tool_calls；Responses adapter 映射 input、function tool、function_call 和 function_call_output。`ConversationItem` 把文本与完整 `ToolRound` 按真实顺序保存，因此截断只删除完整 user/tool/assistant turn，不会产生孤立 tool output。统一类型还包括 `ConversationMessage`、`ToolDefinition`、`ToolCall`、`ToolResult`、`Usage`、`FinishReason`。新增 provider 只需实现 trait，不能把供应商 JSON 泄漏到 Agent。

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
