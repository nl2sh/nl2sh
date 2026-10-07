# Architecture

## 产品定位

nl2sh 是以 Android 原生 shell 为一等环境、Termux 为兼容环境的类 Hermes AI Agent，以多轮 Tool Calling 连接模型、本地安全引擎和真实 Android 执行结果。其部署单元是单个 stable Rust Android 可执行文件，内置 TUI、Web 服务和设备桥接入口；可选 A2A/MCP 网关运行在主机侧。配置、日志和发布辅助脚本不是直接 Android 部署的运行时依赖。终端交互以丰富 TUI 为主，同时提供 Web 多会话界面和单次 CLI 模式。“类 Hermes”不构成对 Hermes API、插件系统或功能集的兼容承诺。

预编译包的主机启动器在选定 ABI 后分别计算主机文件与设备端实际 ELF 的 SHA-256；摘要一致时保留设备文件并直接启动，不一致时推送并再次校验。Bootstrap 脚本从用户显式选择的 GitHub 或 Gitee 最新 Release 同时下载 ZIP 与 `SHA256SUMS`，校验后解压；Gitee 路径通过公开 Release API 解析最新 tag，不在 GitHub 失败后隐式切换来源。脚本可根据显式 Provider、模型、Endpoint 和 API Key 生成最小配置；Linux 脚本在 `curl | bash` 模式完成脚本读取后把启动器 stdin 重新连接到主机 controlling terminal，使 ADB 能为 TUI 分配远端 PTY。Windows CMD Bootstrap 作为交互批处入口，复用 PowerShell 安装核心执行 HTTPS 下载、SHA-256 校验和 ZIP 解压，并保留 CMD 控制台输入。只有显式设置 `NL2SH_CONFIG_SOURCE` 时启动器才覆盖设备配置，设备文件继续保持 `0600`。这些主机侧便利流程不改变设备内的 Agent 安全与确认链。

Android companion 与 JADX helper 源码分别由 `nl2sh/android-bridge` 和 `nl2sh/jadx-helper` 独立 Git 仓库维护，采用独立 Gradle 根工程、CI 和标签发布；主仓库不通过相邻目录或 submodule 构建它们。包名、ContentProvider authority 与 `com.nl2sh.jadx.Main` 入口保持兼容。主仓库 Release 汇总独立发布的指定版本资产，生成使用现有 APT GPG 信任根认证的兼容性 Manifest。运行时默认 JADX 来自内嵌签名策略，未签名本地构建需显式离线或自定义 HTTPS+SHA 配置。

Android API 26+ 原生发行支持 `arm64-v8a`、`armeabi-v7a` 与 `x86_64`，分别映射 `aarch64-linux-android`、`armv7-linux-androideabi`、`x86_64-linux-android`。主机启动器优先选择 x86_64 原生 ABI，统一包、自更新资产与 Termux aarch64/arm/x86_64 deb/APT 使用相同发布矩阵；文档合并继续兼容既有双架构签名快照。

## 系统整体架构

```text
User
  |
TUI / Web / CLI / bridge
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

可选 `a2a_gateway/` 是主机侧独立 Python 模块：A2A 1.0 Agent Card/JSON-RPC、Bearer 认证和 SQLite Task Store → 固定设备序列号的 `adb exec-out` → Android 单文件程序的 `bridge inspect|tools|ask|invoke`。设备序列号为 IPv4 `地址:端口` 时，网关在每次调用前建立无线 ADB 连接，不在回复丢失后重放操作。`ask` 进入内置 Agent；`invoke` 将具名工具和结构化参数直接交给设备 Tool Runtime，不进行设备端模型请求。两条路径复用工具注册、准备、安全评估和执行边界。A2A `contextId` 映射到私有设备 Agent 会话。默认情况下，`ask` 的无终端确认器拒绝待确认操作，`invoke` 在私有一次性 Unix socket 上等待设备交互终端决定。显式设置设备配置 `bridge_auto_approve = true` 时，两条路径都由桥接确认器自动批准全部风险等级；安全评估、参数校验、命令绑定与提权能力检查继续执行。该配置只在 bridge 入口选择确认器，TUI/Web/CLI 不读取它来决定审批。网关本身不提供批准接口。构建、交叉编译和候选部署是主机侧显式工作流，不作为远程 A2A 技能暴露；候选文件不会覆盖现有设备程序。网关默认仅监听 loopback；网络监听与远程明文 HTTP 需要显式选择，远程客户端默认要求 HTTPS。Compose 镜像持久化 ADB 密钥和 SQLite 任务库，不在设备端添加网络服务。

本地一次性审批的私有目录使用跨进程文件锁串行化请求计数与发布，最多同时保留八个待决请求。强制终止留下的 `live` 标记只有在对应 Unix socket 已不再监听时才清理；正常请求的目录和 socket 仍由 RAII 释放。审批者必须在设备交互终端查看完整动作，危险操作还要输入与请求 ID 绑定的二次确认短语。

同一 Python 包的 MCP 适配层既可作为编码 Agent 所在机器的 stdio 服务，也可由网关进程在 `/mcp` 提供 Streamable HTTP 服务。HTTP MCP 与 A2A 共用监听端口和 Bearer 令牌；网关内的 MCP 工具通过进程内 ASGI 传输执行 Agent Card 同来源校验及带 Bearer 鉴权的 A2A JSON-RPC，不依赖网关的外部公告地址回连。两种传输公开相同的环境盘点、工具目录、直接工具调用、咨询和任务查询，返回 A2A 任务与设备结果；MCP 不直接连接 adb，也不增加写入批准入口。

逻辑上分为 Agent Layer（TUI/Web/CLI 的内置 Agent）、Tool Runtime（注册、参数验证、风险评估、确认和执行）及 Platform Adapters（Android shell、可选 Accessibility companion、Termux 与开发主机条件路径）。直接工具调用只跳过 Agent Layer；安全与确认层仍在设备端。`android.*` 语义工具默认可使用 Android shell 的 `am`、`input`、`uiautomator` 和 `screencap`；应用启动使用限定包名的 MAIN/LAUNCHER Intent，不发送随机输入事件；可选 companion 通过 Android ContentProvider 的 Binder 调用提供实时节点树、按文字或 bounds 点击节点、Unicode 输入和单笔 swipe/scroll 手势，Manifest 的 DUMP 权限与 `Binder.getCallingUid()` 双重限制调用方为 shell/root。文字与节点点击在确认后重读完整 UI 树，核对节点所属包名、类名、资源 ID、文字、描述和 bounds；Accessibility 点击在 companion 内再次复核这些字段。Unicode 输入在确认前后及写入前核对焦点控件的包名、类名、资源 ID 和 bounds。缺失包名的语义写入目标会被拒绝。节点树截断或读取失败时不执行语义点击。手势在确认前固定坐标和时长，确认后重新计算并核对；companion 等待 Android 的完成或取消回调再返回结果，确认后不静默切换到 shell。无路径截图在 Android shell 的 `/data/local/tmp` 或 Termux HOME 建立私有临时目录，并以有界图片附件返回。Shell `input text` 只接受可打印 ASCII，并把字面量 `%s` 拆成独立输入命令以避免被 Android 解码为空格；Unicode 需要安装并启用 companion。远程调用方不能代替设备交互终端批准动作。

Companion 的节点文字和描述最多展示 256 个字符，同时携带完整 UTF-16 内容的 SHA-256 摘要；`android.tap_text` 可以用调用方提供的全文匹配摘要，确认前后及 Accessibility 点击前也校验摘要，避免不同长文本共享展示前缀时误点。旧版本原生程序没有摘要时，companion 只接受不超过展示上限的节点文字和描述。

Companion 的 UI 树同时按节点数和序列化字节数设限；Binder 回复达到预算时返回明确的部分树，语义查找和点击拒绝把部分树当作唯一性证据。原生端对 `content call` 等固定机器协议使用静默管道捕获，避免默认 PTY 影响较长的编码回复；普通交互命令的 PTY、进程组、超时和终端恢复路径不变。

Companion 同时提供可选输入方法 `nl2sh Keyboard`，让 `android.input_text` 能在字段既不报告 `isEditable` 也不报告任何文本动作时，经应用的 `InputConnection` 提交 Unicode 文字，与 ADBKeyboard 相同。用户在系统键盘设置中自行选择该键盘，程序不代改；nl2sh 不为每次写入改后端，确认前确定的动作只有三种：字段可编辑时用 `ACTION_SET_TEXT`，字段隐藏该标记但 `focused_target` 的只读 `can_write` 探针为真时用剪贴板粘贴，两者都不成立时用输入法提交。输入法动作在确认前后校验编辑器的包名与 field id，密码编辑器一律拒绝；接受提交却无法读回文字的窗口级连接（例如只暴露搜索框的 WebView）报失败而不是静默成功。`mode: "replace"` 表示先清空再写入，只有输入法通道能做到，shell 与无障碍路径在准备阶段直接拒绝。

输入法的 shell 广播入口（`com.nl2sh.bridge.IME_TEXT` / `IME_TEXT_B64` / `IME_CLEAR`）由输入法服务在运行时注册，以 `android.permission.DUMP` 作为发送方权限，只接受 shell/root，且只有键盘已加载时存在；Android 会跳过发给后台应用的 manifest receiver，所以用上下文注册而不是 manifest 声明。该入口不经过 nl2sh 确认链，其权限等同 `adb shell` 本身，仅用于显式的本地调试。两个服务互相独立：只启用键盘即可输入中文，只启用无障碍即可使用树、点击和手势。

用户输入先成为内部对话消息。Provider 把统一请求映射为 Chat Completions 或 Responses JSON；Tool Call 被转换回内部类型。Agent 只能把 shell tool 交给安全引擎，确认完成后才能调用执行器。stdout、stderr、退出码、超时和错误被编码为 Tool Result，下一轮模型只能依据这些真实结果回答。

TUI 中以 `!` 开头的输入是显式本地命令：去掉前缀后不请求 Provider，而是直接进入同一 `Security → Confirmation → Execution` 边界，并在当前会话界面显示有界实时输出和退出状态。该输入与结果不加入模型上下文；修改、危险、Root 和用户编辑后的命令仍按完整规则重新分类及确认。交互式命令继续通过既有 PTY 挂起、恢复和清屏过滤路径执行。

每个 Agent 任务开始时，统一 runtime 模块根据 `TERMUX_VERSION` 或标准 Termux `PREFIX` 区分直接 Android shell 与 Termux，并由执行器向 system prompt 附加一次低敏感摘要，仅包含环境类型、API level、设备 ABI、进程架构、shell、当前 UID 与 root/su 能力；探测失败时省略对应字段且不阻断任务。直接 Android shell 使用 `/system/bin/sh`/toybox 的一等保守基线；Termux 兼容模式使用 `$PREFIX/bin/sh`、XDG 路径和包管理基线，但不假定可选包已经安装。摘要只用于命令兼容性提示，不包含型号、序列号、Android ID、IP、账号或应用列表，也不参与安全分类、确认或提权决策。

TUI 在命令运行期间展示有界实时输出，工具轮完成后移除对应临时行并以默认折叠项保存有界结果，F2 只改变显示展开状态。执行捕获、实时 UI、日志事件/文件和模型 Tool Result 分别应用配置上限；截断保留头尾并插入显式标记，模型不会把不完整结果误认为完整。最终回答提示要求按用户语言总结，多项结构化对比优先使用 Markdown 表格。

统计对比或趋势适合图形时，模型可调用只读 `create_chart`，把已取得的数值、标签和来源交给本地校验。工具只接受有界的非负有限数值及柱状、折线、饼图类型；它不采集数据，也不证明模型提供的来源或数字正确。结构化规格作为普通 Tool Result 随会话保存；Web 根据对应工具调用渲染图表与可展开数据表，TUI 把结果显示为标题、来源和逐项数值。图表呈现不改变数据获取工具的安全分类和确认链。

Provider 通过 SSE 将模型文本增量送入 Agent 的显示 sink；TUI 在当前响应尾部播放有界渐变动画，并在响应完成后立即切换为普通正文。流式工具调用只聚合名称和参数，完整响应解析完成后才进入安全分类与确认链，绝不边接收边执行。最终文本由独立 Markdown 显示层转换成 ratatui `Line`/`Span`；工具、命令和原始输出绕过该层。围栏代码按声明的常见语言使用主题语义色做轻量语法高亮，未识别语言保持纯文本。表格使用 Unicode 显示宽度计算列宽，在内容区域内压缩并换行，窗口过窄时降级为键值列表。解析无法识别的行保持原文，显示转换不回写对话或日志。

TUI 的视觉语义统一由 `UI_DESIGN.md` 约束。实现应以集中式 `Theme`/`Palette` 向 Widget、Markdown、工具结果、确认界面和状态栏提供语义样式，禁止各渲染模块自行硬编码业务颜色。主题只影响显示，不得改变安全评估、确认策略、root 行为、日志内容或 Tool Result；颜色也不得作为风险信息的唯一载体。

内置 Web 页面复用 `UI_DESIGN.md` 的 TrueColor 语义色板，通过 `web/src/style.css` 的 CSS 变量集中供页面及 Markdown 使用；浏览器不需要终端的 ANSI fallback。Web Markdown 围栏代码仅对已注册语言语法高亮，未知语言转义后按纯文本显示。Web/TUI 的视觉调整均不进入安全或执行边界。`--web-only` 在 HTTP 服务启动后直接等待服务或进程信号，不初始化 TUI、raw mode、alternate screen 或 PTY，适合由 Android shell 后台守护；构建、预编译和安装启动器的显式 Web-only 选项通过设备端 `nohup` 启动该模式且不请求 ADB PTY。Web 内的工具执行仍使用原有捕获式执行、安全分类和浏览器确认流程。

Web 会话状态由 Agent Runner 经流式显示 sink 报告模型请求、逐步计数、Token 使用和工具调用阶段，审批入口单独报告等待阶段。会话轮次按接受的用户任务累计，本轮步骤按模型请求启动计数，本轮工具按预算准入计数，失败、取消与拒绝也保留已发生的计数。单调时钟分别累计模型请求（含传输重试）、工具处理（含准备和安全检查、排除人工等待）及审批/补充信息等待时间；总耗时还包含任务准备等本地开销。SSE 快照携带当前任务统计，浏览器每秒更新运行时长，完成后冻结并保存显示摘要。统计与有界显示历史作为独立 Web presentation 持久化，不进入模型上下文；旧快照缺失耗时则保持未知。

Chat SSE 的 `reasoning_content`/`reasoning` 与 Responses 的 reasoning/summary 文本增量仅供 Web 单独折叠显示，不拼入最终回答、工具参数或模型历史；已产生思考增量后也禁止自动协议回退重放。Web 用户输入、模型输出、工具参数/结果、阶段、审批决定与任务统计进入共享 JSONL 审计日志，事件标明会话 ID，先脱敏再截断；文件写入由独立队列在阻塞线程处理，所有打开同一日志路径的句柄共享限额与清空状态。安全与执行决策不读取这些显示统计。

Web 在接受消息时先保存私有、脱敏的进行中检查点，工具结果、等待审批、取消与失败会更新有界显示历史和阶段事件。未完成检查点仅用于恢复可见诊断，不进入模型历史，也不恢复或批准待审批操作；进程重启后标记为中断。失败、中断或取消检查点保留可识别的原始用户输入时，Web 可显式重新提交该输入；重试不把检查点工具输出注入模型，不恢复旧审批，脱敏后无法还原的输入不提供重试。完整回答先写入会话并结束运行状态，标题模型请求随后在后台更新；浏览器断线重连时重新核对会话列表，清除已经不存在的运行状态。Web 会话摘要包含独立于最后更新时间的创建时间，浏览器在一天内显示相对时间，其后显示本地日期和时刻。

TUI 在完整 Agent turn 保存后异步请求同一无工具短标题生成器；标题写入私有会话快照，`/sessions` 选择器显示标题，稳定会话 ID 继续用于恢复和重命名。后续自动保存保留标题；标题请求失败不影响回答与会话保存。本地 `!` 命令不触发标题生成。旧快照缺少创建时间时从系统生成的会话 ID 恢复，无法恢复时使用旧快照的最后更新时间。

Web 快速开始复用现有配置解析、渲染和原子保存接口；模型列表查询从当前表单接收未保存的 Base URL 与 API Key，复用 Provider 元数据客户端且不写入配置；每次查询在共享审计日志写入关联编号、服务域名、服务类别、凭据是否存在、代理是否启用、耗时与结果，不记录密钥和上游响应正文；保存后通过短模型请求检查推理连接；设置是否完成由现有 Provider 配置判断提供的布尔值决定。示例任务只填充用户输入，发送仍进入通常的 Agent 链；首次发送可自动创建会话。Web 审批显示本地风险等级、检查依据与完整待执行内容，批准按钮使用警告色；危险操作先在服务端标记本次待决请求已进入复核阶段，再次点击才可批准，无需输入确认短语。审批决定携带待决请求编号，旧弹窗不能操作后续请求。编辑操作继续提交原有重新评估入口。高级视图只改变显示，不改变工具注册、安全分类、确认或执行。内置 Web 仍默认监听所有 IPv4 接口且无需登录。

Web 快速开始在保存配置后发送无工具的短模型请求，直接验证推理接口；该请求不进入设备 Agent 会话。独立设备概览复用固定、静默、只读的 Android 环境探测，强制普通用户和捕获式执行，避免依赖模型配置。工具目录读取 Registry 本地分类与风险下限，并以中文说明组织展示；具体调用仍按本地参数和安全评估决定实际确认等级。Web 停止任务使用会话级取消信号：模型等待可取消，捕获式 shell 在进程组信号升级和 wait 后返回，其他工具在当前操作完成后检查取消。取消不批准待确认操作，也不把未完成工具结果标为成功。浏览器结果摘要只根据本地工具结果状态生成，不改变模型上下文或安全策略。

快速开始在缺少现有 Provider 凭据时首选 DeepSeek 与 `deepseek-flash`，保留已配置的服务与模型；OpenAI 使用固定官方地址，自定义服务可编辑 Base URL。该引导只改变 Web 入口中的预填值，不改变 `Config::default` 的 OpenRouter 默认值。

TUI 启动欢迎内容把 Web 浏览器入口放在末尾，以专用显示标记渲染高辨识度地址和访问说明；该标记仅用于本地显示，不进入模型上下文、审计或 Web 会话持久化。

网络、解析或执行错误沿 `anyhow::Result` 返回 UI。LLM 重试覆盖传输错误、429、5xx，以及协议已显式选择或成功协商后兼容网关偶发返回的 405，并使用有上限的指数退避；自动协议首次探测的 404/405 仍立即切换 adapter，401 等配置错误立即返回。Provider 返回的函数参数不是完整 JSON 时，协议层保留调用 ID、工具名、参数字节数和解析错误，但不猜测修补内容；Runner 把它作为未执行的失败 Tool Result 反馈给模型，初次失败后最多允许两次重新生成。每次拒绝都计入 Step 与 Tool Call 预算，连续失败超过上限后终止当前任务；重新生成的参数仍从工具准备开始经过完整安全分类、确认和执行链。Ctrl+C 可取消 HTTP 请求、响应读取和退避。执行超时先给进程组 SIGTERM，短暂等待后给 SIGKILL；Ctrl+C 先给 SIGINT 再升级并回收子进程。Agent TUI 以异步任务驱动 LLM、确认和捕获式命令，保持同一 ratatui frame 并持续刷新历史；只有必须直接占用终端的全屏交互命令才临时离开 alternate screen。交互命令结束后恢复 alternate screen 与鼠标捕获，并清除 ratatui 的旧差分缓存以完整重绘框架。

终端进入 raw mode 和 alternate screen 后由 `TerminalGuard` 持有；TUI 启用鼠标追踪以稳定接收滚轮，宿主终端通过 Shift+拖选保留原生高亮与右键菜单复制。正常退出或错误展开都会恢复鼠标、屏幕、raw mode 和光标。panic hook 做尽力恢复。release 的 `panic=abort` 意味着析构不保证执行，因此生产路径避免 panic；hook 是 abort 前的最后保护。

## Rust 模块

| 模块 | 职责与主要类型 | 输入 / 输出 | 依赖与禁止事项 |
|---|---|---|---|
| `src/config` | `Config`、枚举、loader、wizard、分层校验 | 文件/缺省值/环境 → 可进入 TUI 的运行配置；完整 Provider 配置 → LLM 可用 | 不执行命令，不持有 UI 状态 |
| `src/history` | `HistoryLog`、JSON Lines 事件与安全创建 | 交互事件 → 可刷新诊断日志 | 不记录 provider 凭据，不参与安全决策 |
| `src/tools/file/domain` | `FileToolExecutor`、结构化读取/搜索/补丁 | 任意可访问路径 → 有界结果或待确认 diff | 路径不设工作区边界；写入必须先确认，不调用 shell |
| `src/tools/audio` | WAV/Raw PCM DSP 与 Jev/通用 LLM 质量判断 | 音频 → Feature JSON、`needs_input` 或评分 | Raw PCM 元数据不得猜测；Jev 已配置时失败不回退；不上传原始音频 |
| `src/tools/chart` | 有界图表规格校验及终端文字回退 | 模型提供的数值 → 结构化 Tool Result | 只读呈现，不采集或验证统计证据 |
| `src/tools/android` | 固定参数诊断、只读环境盘点、语义 UI 操作、设备聚合、剪贴板与媒体工具 | 严格结构化参数 → 有界证据或待确认动作 | 不提供任意 shell；文字/节点点击在确认前和执行前重读 UI 树；写入必须确认；Unicode 写入后端在确认前固定，不在确认后切换 |
| `src/tools/memory` | 私有有界键值便签与内嵌 SQLite 事务持久化 | get/list 或确认后的 set/delete/clear → JSON；Web 显式 CRUD → 条目列表 | 不依赖系统 sqlite3；不把便签当系统指令；限制键、值和条目数；模型写操作必须确认 |
| `src/sessions` | `SessionStore`、私有原子快照 | 完整对话 turn → 可恢复会话 | 不序列化配置、凭据、余额或任务审批；工具结果保持有界 |
| `src/llm` | `LlmClient`、`TextDeltaSink`、统一消息/工具类型、两个 HTTP/SSE adapter、retry | `LlmRequest` → 文本增量 + `LlmResponse` | 不进行安全判断或执行工具 |
| `src/provider_metadata` | `ProviderMetadataClient`、Provider 识别、模型列表与上下文元数据归一化 | Provider 配置 → `ModelMetadata` 列表 | 只读网络访问，不记录凭据/原始账户响应，不参与模型推理与安全判断 |
| `src/provider_account` | `ProviderAccountClient`、余额结果归一化 | Provider 凭据 → 可显示余额 | 仅调用公开只读接口；不记录凭据、余额或原始响应，不参与推理、安全或执行 |
| `src/runtime` | `AndroidRuntime`、Termux 标记与 prefix 探测 | 进程环境 → Android shell/Termux | 只提供兼容性信息和 shell/path 选择，不参与安全分类、确认或 root 授权 |
| `src/network` | 统一 rustls HTTP Client、HTTP/SOCKS 代理、认证和绕过策略 | `Config` → `reqwest::Client` | 代理凭据不得进入日志、错误详情或模型上下文；关闭总开关不清理配置 |
| `src/tools/network` | 公网 HTTP(S) 有界读取、确认后 POST/下载与 TLS 诊断 | URL/主机 → 有界正文、文件或证书信息 | 禁止重定向、URL 凭据、本机/私网目标和任意 header；POST、下载均须确认 |
| `src/tools/apk` | APK ZIP 概览、条目检索、DEX 类索引与单类反编译适配 | 本地 APK → 有界 JSON 证据或 Java 源码 | 前三项纯 Rust 只读；反编译为 Dangerous 强确认，不执行 APK 内容 |
| `src/tools/tailcat` | 可选 tailcat 检查、一次性接收、文件发送、端口服务和状态/停止 | 校验后的参数 → argv 子进程与当前进程管理的监听器 | 组默认关闭；接收确认，发送及开放端口强确认；临时密钥和父进程退出信号限制监听器生命周期 |
| `src/runtime_dependencies/jadx` | Android DEX helper 校验/按需下载、私有缓存和受控子进程 | 含 `classes.dex` 的 helper → 带私有 Java 临时目录的 `app_process` → 单类源码 | 仅在反编译强确认后下载；普通 JVM JAR 拒绝；helper 只处理目标类并拒绝 XML 解析；无默认未验证资产 |
| `src/web_ui` | Axum 0.8 HTTP/SSE/WebSocket、多 Agent 会话、LLM 自动标题、快捷运行设置、记忆 CRUD、浏览器审批、rust-embed 资源 | serde JSON + SSE + WebSocket → 独立 Agent Runner、结构化显示条目、SQLite 记忆、原子配置文件 | 无登录，优先监听 IPv4 9999（占用时使用可用端口）；每会话独立锁和审批通道；直接记忆管理仅代表用户操作，模型写入仍确认；WebSocket 终端仍走安全分类和确认；请求有大小上限；不直接执行模型输出 |
| `src/tools/ui` | UIAutomator 控件树、焦点窗口、截图及模型图片附件 | 当前界面/本地图片 → 有界节点、截图或临时多模态内容 | 固定探测静默执行；超限图片有界缩放；截图写入必须确认；附件不持久化 |
| `src/update` | GitHub Release 发现、版本/ABI 选择、SHA-256 校验与原子替换 | Release 元数据与 Android ABI → 已校验的新可执行文件 | 不执行模型输出；不接受跨 ABI 或无校验资产 |
| `src/agent` | `AgentRunner`、上下文完整交互单元、`Confirmer` | 用户任务 → Tool Loop / 最终文本 | 不得绕过 security 和 confirmer |
| `src/bridge`、`src/tools/runtime` | 固定环境盘点、工具目录、有界 JSON Agent 调用和直接工具调用 | `bridge` CLI → JSON / 私有会话或工具结果 | 直接调用绕过 LLM 但保留工具安全链；默认 ask 拒绝待确认操作、invoke 等待本地审批，显式 bridge_auto_approve 自动批准；不开放任意 adb 命令 |
| `a2a_gateway` | 主机侧 A2A Agent Card、JSON-RPC、鉴权、Task Store、adb 传输、stdio/HTTP MCP 适配及显式构建部署 | MCP → A2A 消息 → Android bridge 结果 | 不在设备运行；HTTP MCP 需 Bearer 令牌；不直接执行模型输出；部署仅到独立候选路径 |
| `src/tools` | `Tool`、显式 `ToolRegistry`、风险/能力元数据、派生 schema 与 `PreparedToolCall` | 模型调用 → 预备动作 → 审批后有界结果 | 只用本地元数据定风险；修改预览必须在统一确认入口批准后执行 |
| `crates/nl2sh-tool-macros` | 编译期 `#[tool]` 生成适配器与元数据 | 注解函数 → Rust Tool 实现 | 只在构建主机运行；不自动注册或授予执行权限 |
| `src/security` | `shell/{parser,analyzer,expansion,effects}`、`policy/{filesystem,android,privilege,network}`、特殊 regex/自定义规则、`SecurityAssessment` | 原始命令或结构化工具风险 → 风险和确认要求、命令绑定能力 | 不依赖 TUI、LLM 或执行器 |
| `src/shell` | `ExecutionBroker`、`CommandExecutor`、root invocation、process group、pipeline/PTY 边界 | 已批准命令能力 → `ExecutionResult` | 不自行降低风险或批准命令 |
| `src/tui` | terminal guard、session 状态机、独立 output/history 生命周期、事件、输入、`@` 文件候选、中英文文案、ratatui 渲染 | key/mouse event → 用户输入 | 不解析 OpenAI JSON，不直接执行；启动帮助不进入模型上下文 |
| `src/file_references` | 识别用户输入中 `@` 后最长的已存在路径前缀并解析为绝对路径 | 原始用户文本 → 保留原文并附加有界路径提示 | 不读取文件内容、不执行命令；内容仍由结构化文件工具按上限读取 |
| `src/ima` | 腾讯 ima 知识库只读发现、搜索与原文读取 | 独立 Client ID/API Key → 有界知识库结果 | 强制直连且不使用代理；不提供任何写接口，不泄露长期凭据、临时 header 或签名 URL |

公共 trait 允许测试以 mock 替换网络、执行、确认和 root 探测。依赖方向保持 `UI → Agent → abstractions`，security 与 shell 彼此通过调用参数协作，无循环依赖。

模型可见工具由 Registry 显式注册，ima 按本地 `Capability` 条件暴露；APK/JADX 与 Tailcat 组默认关闭，配置的单工具开关覆盖组开关。Agent、桥接直调与目录共用可用性判定，关闭项不进入模型定义且无法直接调用；Web 目录另外列出关闭项供用户设置。一次性桥接入口过滤需当前进程维持的 Tailcat 监听器工具。`tailcat_install` 在准备阶段固定设备 ABI、官方 v0.7.0 URL（ARM64/ARMv7/x86_64 对应 arm64/armv7/amd64 静态包）、预置 SHA-256 与绝对目标路径，作为修改类进入同一确认链；批准后才下载有界归档，核验摘要、ELF 架构及版本，使用同目录临时文件原子替换，失败时保留原程序。`tailcat_check` 保持只读。`tailcat_adb_pair` 的 setup/share 都是 Dangerous：setup 仅通过受限 Settings 导航引导无线调试，未启用开发者选项或不支持界面时请求用户操作；share 在准备阶段固定当前配对码、地址和连接端口，审批后及监听器启动前后重新核对，以一个受管理子进程共享两个 ADB 端口和显式可选 Web 端口。配对码按用户需求返回模型与对话，不进入审批预览或 shell 参数；监听器不自动替换，失败只清理本次新建实例，停止不撤销系统配对。`tailcat_serve` 把隧道连接转发到已有 localhost 服务，参数是目标服务端口，不重复绑定目标端口；共享仍需 Dangerous 强确认。参数类型通过 `schemars::JsonSchema` 派生定义。共用内部参数类型的 `android.*` 工具按各自操作收窄公开字段及必填项，准备阶段也拒绝无关字段，避免外部 Agent 按宽泛 Schema 误填。`Tool::prepare` 解析参数并构造预览与待执行动作；工具元数据声明风险下限，剪贴板、媒体和便签按实际参数升高单次风险。Agent 统一根据本地风险决定确认/强确认，再调用 `PreparedExecution::execute`。补丁和下载在确认前只准备 diff 或数据，截图、HTTP POST、输入注入与设备控制必须先显示预览；输入注入执行前重新校验当前 UI bounds。Shell 命令继续逐次经过原安全分类、编辑重评估、Root 与 PTY 回收路径。音频缺参问答与分析缓存、截图附件以及 Web 会话审批沿用既有行为。`define_tool!` 和构建期 `#[tool(...)]` 均只生成适配器，注册和权限仍需显式决定。

Web 前端源码和 npm lockfile 保存在 `web/`；Cargo 的 `build.rs` 先把源码复制到 `OUT_DIR`，在副本中执行 `npm ci` 和 `npm run build`，再由 `rust-embed` 把产物编入单一可执行文件。构建需要 Node.js/npm，生成目录不进入版本控制或发布源码包；Android 运行时不依赖 Node.js。TUR 构建使用 Termux 提供的主机 Node 工具。

Web 配置页通过 `/api/config/validate` 使用 Rust `Config` 解析及运行校验 TOML，通过 `/api/config/render` 将分组字段草稿写回 TOML；两种模式保存时都复用 `/api/config` 的校验与原子写入。分组模式覆盖普通字段，高级 `security_rules` 在文件模式编辑；新的 Web 任务在启动时重新加载配置，运行中的任务持有其启动时快照。

Web 会话侧栏支持直接删除单个会话或确认后删除全部会话：删除同时移除内存条目与私有会话快照，只针对合法的会话文件名；运行中、等待审批或终端仍连接的会话拒绝删除。会话列表、恢复、启动任务和删除共享注册表同步，防止删除后立即被迟到的恢复或任务重新写回。

`src/tools/configuration` 注册 `nl2sh_config`（Configuration 类）：list/get 只读展示默认、磁盘、解析及当前任务快照；set/reset 使用强类型动作与原生 JSON 值，仅操作 `Config.source`。loader 的 TOML 解析、预算预设及环境覆盖由内置加载与工具共享。工具保存通过 toml_edit 保留其他字段/注释，不序列化运行快照；审批后复核有界原始内容，稳定侧文件锁串行化工具写入，私有临时文件原子替换并同步。凭据仅展示配置状态且拒绝模型写入，解析诊断不回显原文；安全/Root/桥接/工具/网络/审计字段升至 Dangerous。写入不改变当前任务、客户端、工具目录或审批器；新 Web 任务/bridge 进程加载，TUI 重启应用，不涉及 shell 或 PTY。

## Agent 执行流程

只读操作在 balanced/risk_only 下自动执行；普通修改必须确认；Dangerous/Critical 需要二次确认。root 只是执行属性，不改变分类。审批界面提供固定编号与快捷键，可仅允许本次、拒绝、编辑或选择执行模式；对非 Root、非强确认且最高为 Mutating 的命令，还可在当前 Agent 任务内记住完整命令的精确许可。该许可不持久化、不按前缀匹配，Runner 会在每次复用前重新检查当前评估仍满足条件。拒绝、失败或超时都会生成明确的失败 Tool Result。

审批面板按命令或 diff 的 Unicode 显示宽度和实际换行高度动态调整，最大范围受终端与输入区约束。超高内容在独立正文区通过滚轮或 PageUp/PageDown 浏览，编号选择、强确认和编辑输入固定在底部；布局与滚动不改变风险等级或确认语义。

Agent 文件操作优先使用 `read_file`、`list_dir`、`search_text` 和 `apply_patch`，不依赖设备端 `sed` 或 shell 重定向。路径不设工作区沙箱：允许绝对路径、父目录组件并跟随符号链接；读取、遍历、匹配和文件大小仍有硬上限，最终 Tool Result 继续使用配置的模型输出上限。`apply_patch` 在内存中验证唯一替换并生成 diff，确认前不打开目标进行写入，每次调用均单独确认，批准后才原子替换。

只读工具返回结构化 `needs_input` 时，Runner 可通过独立于安全审批的用户问答接口请求缺失事实。TUI 在同一 frame 内显示支持候选选择和自定义输入的多字段窗口，非 TUI 模式使用终端文本回退；等待用户回答的时间不计入活跃任务时长。当前 Raw PCM 分析会用该接口收集采样率、声道数和采样格式，并将答案直接合并到原工具参数后本地重试，不经过模型改写或猜测。取消只保留 `needs_input` 结果，不批准命令、文件写入或 root 操作，也不改变既有 `Security → Confirmation → Execution` 边界。

同一 Agent 任务内完成的音频分析按原始路径保存结构化结果；质量判断优先按路径引用该结果，避免模型复制、删减或改写 DSP 字段。只有完整的 `status=ok` 分析能够进入缓存，缓存不跨任务持久化。

TUI 输入中的 `@路径` 提供本地文件/目录候选，支持相对路径、绝对路径、`~/`、`./` 与 `../`，Up/Down 选择并以 Enter 或 Tab 补全；Right 保持普通光标右移。提交时按“最长已存在路径前缀”解析，因此 `@test.txt写的是什么内容` 不要求路径后有空格；解析结果只向 Agent 附加绝对路径，实际内容仍由有界结构化文件工具读取。路径解析不会执行文件内容，也不会改变 shell 安全分类、确认或 root 策略。

`inspect_android_environment` 通过固定的静默只读探测返回 Android 版本、设备支持的 ABI、命令可用性、内存与 `/data` 容量；设备 ABI 以 `ro.product.cpu.abi` 为准，进程架构不能替代安装目标 ABI。`android_connectivity` 仅回传有界 ICMP/路由证据与默认网络摘要；ICMP 成功不等于 HTTPS 下载成功。两者只提供证据，不批准安装或降低 shell 命令风险。

Web 输入框通过有界只读接口复用同一文件候选逻辑，按光标位置补全当前 `@路径`；发送时沿用同一引用解析提示。左侧工具列表由当前配置和显式工具注册表生成，仅返回名称与简介并在浏览器本地搜索，不授予额外工具能力。左侧文件列表使用独立只读目录接口，显示类型图标、大小和修改时间，最多返回 1000 项；图片、视频、音频和常见文本/代码格式可在前端弹窗预览。预览接口只提供白名单格式，文本限 2 MiB、图片限 32 MiB、普通音频限 256 MiB、Raw PCM 限 32 MiB、视频限 2 GiB，媒体按 HTTP Range 流式读取；返回固定 MIME 与 `nosniff`，HTML/代码作为转义文本并按已知语言高亮显示，可切换自动换行。WAV 直接使用浏览器播放器，并可从 WAV 头部读取采样率、声道和采样格式作为参数播放初值；无头 PCM 需用户选择这些参数。参数播放在浏览器内解码，单文件限 32 MiB，不猜测原始 PCM 元数据。文件和目录仍可通过右侧箭头将路径填入对话输入框；预览与插入均不执行目标，也不进入 Agent 安全决策。应用列表复用固定参数、普通用户和非 PTY 的 Android 只读应用枚举。工具列表、WebSocket 安全终端与配置编辑器显示在同一个可最小化侧栏中，终端命令仍经过原安全分类和审批。各页面使用独立的初始和已保存宽度，拖动分隔条时为桌面对话保留最小宽度；窄屏改为覆盖式面板。

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
LLM → Typed Tool → local Tool Policy ─────────────────────────────┐
  └→ Shell Tool → brush-parser → Shell AST → Semantic Effects ───┤
                   └→ special-danger/custom regex policy ────────┤
                                                                  ↓
                                                        Unified risk policy
                                                                  ↓
                                                       SecurityAssessment
                                                                  ↓
                                                       Confirmation Policy
                                                                  ↓
                                                       Execution Broker
                                                                  ↓
                                                        normal/root Android
```

`ShellAstAnalyzer` 使用 `brush-parser` 解析命令列表、管道、复合命令、重定向、替换及静态 `sh -c`/`su -c`，递归收集效果；文件、Android、特权和网络策略按命令与参数分类。解析失败、动态命令名、动态 shell 代码、未知脚本内容或未覆盖的复杂结构提升至 Dangerous。旧启发式风险 parser 已移除；内置 regex 仅保留跨命令的 fork bomb 特殊签名，配置中的自定义 regex 仍可提高风险。结构化工具由本地元数据与预备动作确定风险下限。两条路径共用 `SecurityAssessment::from_policy` 转换确认要求；root 执行计划也要求确认。

Shell 审批后，`PrivilegeBroker` 重新评估完整命令并核对批准记录的精确文本，签发只包含该命令和 root 计划的 `ApprovedShellCommand`；普通用户模式还拒绝 AST 识别出的显式 `su`。Agent、TUI 本地命令、Web 安全终端与单次 CLI 均通过 `ExecutionBroker` 将它送到既有执行器。它是进程内能力边界，不是独立特权进程；固定内部探测继续使用原执行接口。AST 分析不执行命令，也不能证明运行时变量、外部脚本和解释器代码的实际效果。

AST 以重定向节点判断真实写入，文字参数中的 `>` 不作为重定向。代码解释器、eval、管道进入 shell 等再次解释输入的场景保守升险；`grep -E/-e/-c` 和 `sed -e` 的普通诊断参数不会单凭 `>` 升险。

## 扩展

- 新 Provider：实现 `LlmClient` 和协议 adapter。
- 新 Tool：增加内部参数类型和 tool policy，所有有副作用 tool 必须进入 security/confirmation。
- 新安全规则：优先在对应领域策略增加 AST 语义与测试；仅跨命令特殊危险签名使用内置 regex，配置仍可增加自定义 regex。
- 新执行环境：实现 `CommandExecutor`，保持结果和取消语义。
- 新配置来源：在 loader 合并并记录优先级，再统一 validate。
- 新 UI：仅依赖 Agent/trait API，不访问 provider JSON。
- 新 shell 语义：扩展 AST 遍历与回归语料，保持 `assess` 和 `SecurityAssessment` 公共接口。

## 更新与设置

启动更新检查是只读后台任务，失败不阻塞 TUI；仅在发现更高版本且匹配本机 ABI 时提示。立即更新会先恢复终端，再下载裸二进制及 SHA-256，校验通过后在当前目录原子替换。跳过版本写入配置，普通暂不更新不持久化。

Termux TUR/APT 构建不启用 `self-update` Cargo feature：不执行启动更新检查，也不替换包管理器拥有的 `$PREFIX/bin/nl2sh`，手工更新入口只提示 `pkg upgrade nl2sh`。TUR 配方从固定 tag 和 SHA-256 源码归档通过 `termux_setup_rust` 构建。直接 Android 发布仍默认启用校验后自更新。Termux 默认配置遵循 XDG config 目录，日志与会话遵循 XDG state 目录；非 Termux直接部署和显式配置路径继续保持配置相邻状态。

`/config` 与别名 `/setting` 使用单一 TUI 设置面板承载服务、模型与 Agent、执行与安全、界面、网络、知识库和工具分类；其他分散配置命令不再暴露。两者属于严格本地命令，打开面板后不得进入模型上下文。服务分类与旧向导共享内置 Provider 预设，选择预设只联动 Endpoint，保留 API Key、模型和协议，自定义 Endpoint 显示为 Custom。Tab/Shift+Tab 只切分类，Up/Down 只移动字段，Left/Right 只调整当前值；保存后主循环重新加载配置和客户端。“工具”分类按组或单项编辑 APK/JADX、Tailcat 可用性，单项显示有效状态与覆盖来源；切换组清除组内单项覆盖，与 Web 语义一致。长列表围绕选中字段显示有界窗口。界面分类独立控制佛像与小火车 ASCII Art，并提供显式的日志清除操作；日志清除仅截断当前 JSONL 文件并恢复后续记录能力，不清理当前会话或改变安全链。

Agent TUI 在输入分发边界保留 `/` 前缀命名空间：所有去除前导空白后以 `/` 开头的输入均为本地命令，已知命令执行本地动作，未知命令只产生本地提示。任何斜杠命令都不得写入模型用户历史或调用 LLM。

`/new` 清空当前内存对话、模型上下文和输入历史并分配新的默认会话名，但不删除已保存快照或审计日志。未知单词型斜杠命令可提示编辑距离接近的已知命令，提示不得自动执行候选。

每个已完成 Agent turn 自动保存到状态目录的 `sessions/` 私有目录，文件以 `0600` 原子替换。`/sessions` 打开按更新时间倒序排列的最近会话列表，可输入序号或用 Up/Down 与 Enter 选择恢复；兼容的命名恢复、重命名和删除子命令仍保留。恢复只装载完整 turn，并重新应用上下文轮数和 Tool Result 上限。会话文档仅包含 provider-neutral 对话项，不包含 `Config`，因此 API Key、代理密码、余额和仅当前任务有效的审批许可不会落盘。

设置编辑器在单次打开期间分别持有 Ollama 与 Custom 的 Endpoint 草稿；离开对应 Provider 前保存当前值，切回时恢复。其他内置 Provider 仍使用固定预设地址，编辑其地址会转入 Custom。

`/shell` 是显式的用户直控边界：它暂停 alternate-screen TUI，直接 Android shell 使用 `/system/bin/sh -i`，Termux 使用 `$PREFIX/bin/sh -i`，开发主机条件使用 `/bin/sh -i`。其中输入直接属于用户而非 LLM 输出，不进入模型、安全分类或审计内容；键入 `exit` 或发送 EOF 后必须 wait 子 shell、恢复 raw mode、鼠标捕获和 alternate screen，并显式清除 ratatui 差分缓存后完整重绘原会话。

## 文档与发布架构

正式用户手册以 `docs/zh/` / `docs/en/` 为唯一事实源，MkDocs Material 与静态 i18n 生成中文根路径和英文 `/en/`，导航按照用户成长路径组织。根目录与模块 README 只保留简介和入口；架构/计划/状态/视觉规范保留贡献者内部上下文。CLI/config/tool 参考从编译后的公开接口导出，中文说明与 Schema 分离，CI 比较生成区域并检查双语页面、链接、路由和编辑地址。

Release 先构建签名 Termux APT 快照并作为正式资产发布，不再直接覆盖 Pages。文档 workflow 在 PR 验证、master 或成功 tag Release 完成后构建并合并最新正式快照；旧 Release 没有快照资产时验证已有线上仓库。公钥指纹、Release/InRelease 签名、索引和包 SHA-256 全部通过后才合并 `dists/`、`pool/`、`nl2sh-repo.gpg`，失败停止部署。唯一 Pages 发布任务使用共享并发组，保留 APT 根地址，不把 HTML 写回 master。此流程只处理文档/分发资产，不进入 Android 安全、执行或 PTY 边界。

## 运行时能力与服务信息

`runtime::RuntimeCapabilities` 为每个任务及只读 Web 信息请求建立独立快照；Agent、bridge tools 和
直接调用使用 `ToolRegistry::for_runtime`。配置目录保留全部工具以供设置，并分别呈现 enabled 与
available。能力不是安全许可，仍需原有 Security → Confirmation → Execution。
Bridge 原生端先只读发现 protocol 2 与服务/限额，使用 invoke/base64url JSON、唯一请求 ID 和有界
回复；只在尚未发送动作且旧伴侣不提供 capabilities 时选择旧调用方式，v2 写入后绝不重放。
JADX installed_info 只验证并探测已有 DEX JAR 的协议，不下载；provisionable 单独表示批准后可获取。
`/healthz` 不依赖配置/会话/模型；`/api/info` 的协议 1 提供真实端口、PID、单调运行秒数、ABI 和扩展
快照，不暴露凭据。ShellExecutor 的固定 discovery probe 独立使用 normal、无 PTY、有界输出与五秒
超时，沿用 pipeline 的进程组终止和 wait。描述元数据统一和 service 生命周期随后实施。

### Native service lifecycle

`service` binds lifecycle ownership to the canonical configuration parent and a private adjacent runtime directory. An operation flock serializes start/stop/restart; a child-held runtime flock prevents duplicate daemons. Private atomic state records PID, start ticks, executable device/inode, UID, version, actual bound port and a random shutdown token. Public status omits the token and independently verifies process identity plus HTTP PID/version/port. A token-authenticated loopback TCP control channel authorizes graceful shutdown; controllers never kill processes by name or signal stale PIDs. The child detaches with setsid, logs privately, cancels task watches and rejects pending approvals during shutdown. Android uses Bionic-compatible flock, /proc, loopback TCP and signals, with no systemd dependency. Config/session layout and the security/confirmation/execution chain remain unchanged. Foreground --web-only remains available without implicit service registration.

Helper deployments record an adjacent version/checksum/source ownership marker after successful readiness validation. Native update check/install inspect this marker with bounded no-follow reads and verify the installed file digest; Helper-owned or invalid ownership records cannot self-update through CLI/TUI. Package-manager compile policy remains authoritative. /api/info exposes the non-secret update ownership and supported entry point.

### Signed runtime release policy

The pinned public key authenticates binary SHA-256 OpenPGP signatures before JSON parsing or executable replacement. `runtime_dependencies::manifest` validates schema/protocol, exact tagged asset URLs, ABI, API 26, digests and size limits. Signed extension policy is embedded during native builds; the final policy adds all three native asset identities. JADX caches by artifact digest, verifies a detached signature before publication, and validates protocol/features/version through --info before decompilation. Native self-update verifies manifest, asset digest/size/signature and ELF ABI while retaining installation ownership gates. Unsigned development builds have no default extension download. Helper packages the same public key and checks minimum installer/service compatibility. Private signing material stays in the existing Actions signing environment.
