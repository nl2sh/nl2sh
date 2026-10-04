# Project Plan

状态以 2026-10-01 的代码、验证记录和公开发布状态为准；当前发布版本为 1.0.6。未完成项不会机械勾选。

## 双语 Docs-as-Code 文档站 — 已实现

- [x] MkDocs Material 与静态 i18n；默认中文根路径，完整英文 `/en/`，按用户成长路径组织 46 对页面。
- [x] 用户手册集中 `docs/zh/` / `docs/en/`，双语 README 与模块入口指向正式文档，历史更新记录逐项翻译。
- [x] CLI 帮助、配置默认值、工具 Schema 从代码导出，中文说明、页面对齐、链接、语言切换和编辑目标由 CI 检查。
- [x] PR 严格构建，master 或成功 Release 后统一 Pages 发布；签名 APT 快照验签/验摘要并合并，旧发行版支持验证已有站点仓库。
- [x] 发布 ZIP/TAR 携带完整双语 Markdown 和素材，Termux deb 提供双语站点入口；AGENTS 与 PR 模板要求用户行为变化同步双语。

## 可选工具组与 Tailcat — 已实现

- [x] APK/JADX 与 Tailcat 工具组默认关闭，允许按组及按单工具设置并保存配置；关闭工具不进入模型定义或直接调用注册表。
- [x] Tailcat 结构化检查、原始流接收与发送、文件接收箱与复制、单端口服务及状态/停止；操作按本地风险审批，监听器绑定当前进程。
- [x] Tailcat 安装工具按 Android ABI 选取固定官方发行包，在修改确认后下载、校验并原子安装；检查工具继续保持只读。
- [x] 通用 shell 路径对 Tailcat 网络与文件操作升险；一次性 bridge 入口过滤无法保持的监听器工具。
- [x] 宿主工具级 live 测试及已连接 Android 设备双向传输脚本。

## Hermes 设备 Tool Runtime — 进行中

- [x] A2A 网关支持 Docker Compose 部署、无线 ADB `设备IP:端口` 连接与跨主机 Hermes MCP 接入；明文私网 HTTP 需显式开启，ADB 密钥和任务库由命名卷持久化。
- [x] 创建 `feature/hermes` 分支，保留内置 Agent，并增加直接工具调用的 bridge/A2A/MCP 路径。
- [x] 注册通用 `android.*` UI 工具，复用设备端工具准备、安全评估和确认；文字/节点点击执行前重读 UI 树。
- [x] 将当前节点所属包名纳入语义点击与 Unicode 输入的审批前后和 companion 最终执行校验。
- [x] 为每个 `android.*` 工具公开对应字段与必填参数的 Schema，并在准备阶段拒绝无关参数。
- [x] 建立设备交互终端的一次性审批通道；直接工具调用等待本地决定，修改类需确认、危险类需二次确认，网关不暴露批准入口。
- [x] 增加默认关闭的 `bridge_auto_approve` 设备配置，使 A2A/MCP 的 `ask` 与 `invoke` 可显式自动批准所有风险等级；其他入口继续使用各自审批器。
- [x] 直接工具结果保留 `view_screenshot` 的有界图片附件，A2A/MCP 传输限额覆盖其最大尺寸。
- [x] 提供无持久截图文件的直接屏幕捕获与有界图片回传，MCP 将图片转为视觉模型可读取的图像内容块。
- [x] 完成 Android shell 与可选 Accessibility companion 的通用 UI 后端：APK 提供 Unicode 输入、实时节点树、按文本或 bounds 点击、单笔 swipe/scroll；无 companion 时保留 uiautomator、input、am 和 screencap 路径，模拟器验证两种点击与手势后端。
- [x] companion 增加可选输入方法 `nl2sh Keyboard`：`android.input_text` 在字段不上报任何文本动作时经 `InputConnection` 提交中文，后端在确认前固定为 `ACTION_SET_TEXT`、剪贴板或输入法之一，`mode: "replace"` 只由输入法通道清空后写入；shell/root 另可用 ADBKeyboard 风格广播直接输入。
- [x] 在 Android API 26 模拟器验证 companion 中文输入、语义点击、手势、直接截图及 A2A/MCP 图像链路。
- [x] Hermes Tool Runtime 通过 Android API 26 ARMv7 release 交叉编译，产物为 32 位 PIE ELF；设备运行验证仍单列。
- [ ] 验证 companion 在 Android API 26+ 真机上的节点点击、手势、Unicode 输入、审批和失败恢复。
- [x] 核对普通 Termux UID 的 Android 权限边界：系统拒绝 `content` CLI 外部 provider 访问、`input` 事件注入和 `screencap`；当前完整 UI 自动化要求 shell/root UID。
- [ ] 若要让普通 Termux UID 使用 companion，设计显式本地授权与 Binder 客户端，不放宽现有 shell/root provider 的安全边界。
- [ ] 在 Android API 26+ 真机验证 A2A/MCP 直接调用、审批、UI 操作及失败恢复。

## 独立 A2A 网关 — 已实现

- [x] 主机侧独立 A2A 1.0 网关，支持 Agent Card、Bearer 认证、JSON-RPC、持久 Task 与同上下文续接。
- [x] 独立 stdio MCP 适配层通过 A2A 协议调用网关，供 Codex 发现设备盘点、工具目录、咨询与任务查询能力。
- [x] A2A 网关在同一监听端口提供需 Bearer 令牌的 Streamable HTTP MCP `/mcp`，复用现有工具与设备安全链。
- [x] Android 单文件程序增加最小 `bridge` 适配，复用既有 Agent、工具、安全和私有会话；无人值守写入由确认器拒绝。
- [x] 主机侧显式格式检查、测试、按 ABI 交叉编译、摘要校验和独立候选部署工作流。
- [x] 连接设备验证 Agent Card、鉴权、环境盘点、工具发现、跨轮续接、拒绝修改及构建部署后的继续测试。

## Web 易用性与任务反馈 — 已实现

- [x] Web 实时展示会话轮次及本轮模型步骤、工具调用；完成、失败和取消统计随显示历史恢复。
- [x] 每轮总耗时拆分为模型请求、工具处理和人工等待，摘要保留在对话、快照与导出中。
- [x] 解析并单独显示 Provider 返回的思考增量；Web 审计事件带会话 ID 并执行脱敏和日志限额。

- [x] 断线重连时核对会话是否仍存在，清除过期运行状态并恢复可用会话。
- [x] Web 进行中任务保存私有脱敏检查点；重启后保留诊断证据并标记中断，未完成内容不进入模型历史。
- [x] Web 失败检查点支持显式重新提交原始用户输入，不回放工具输出、审批或脱敏后不可恢复的内容。
- [x] 完整回答先保存并结束运行状态，短标题在后台生成和更新。
- [x] 运行中任务提供会话级停止入口，模型等待可取消，捕获式命令完成进程组清理后停止。
- [x] 快速开始使用短模型请求验证真实对话接口；设备概览无需模型即可读取固定只读信息。
- [x] 工具目录按中文用途、分类和确认要求展示，并提供可填入输入框的示例提问。
- [x] Web 对完成任务归纳工具完整、部分和失败结果，弹窗维持键盘焦点并在关闭后恢复。

## Android 预编译部署优化 — 已实现

- [x] Linux 与 Windows 启动器以主机和设备端实际 SHA-256 判断是否需要推送，推送后再次校验。
- [x] 提供 Linux Bash、Windows PowerShell 与 CMD Bootstrap，可显式从 GitHub 或 Gitee Release 下载并校验 ZIP，生成最小 Provider 配置后启动；Linux 管道安装在启动 TUI 前恢复 controlling terminal 输入。
- [x] 配置只通过显式 `NL2SH_CONFIG_SOURCE` 部署，设备端保持 `0600`，普通启动不覆盖已有配置。

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
- [x] Responses、Chat Completions 与 SSE 的残缺 Tool Calling JSON 以不可执行失败结果反馈模型，有限重试后终止。
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
- [x] 引入 `ShellAstAnalyzer` 和独立 parser、expansion、effects 模块；解析失败和动态执行强确认。
- [x] AST 成为 shell 风险主要来源，旧启发式风险 parser 移除；文件、Android、特权、网络策略分模块，内置 regex 仅保留特殊 fork bomb 签名。
- [x] 进程内 `PrivilegeBroker` 绑定审批文本、重新评估命令与 root 计划，再通过 `ExecutionBroker` 分派所有用户 shell 入口。
- [ ] 扩展 AST 对参数、算术及特殊 shell 结构的精细效果识别，增加 fuzz/对照语料，并评估独立特权进程是否必要。

风险：AST 不能证明运行时变量、脚本文件或 Android shell 扩展的实际效果；不确定结构保守升险。进程内 broker 不提供操作系统级权限隔离。

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
- [x] OpenRouter、OpenAI、DeepSeek、SiliconFlow 与 Ollama 模型发现适配。
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

## Phase 21 1.0.0 正式版发布 — 完成

- [x] 将 Cargo 包版本提升到 1.0.0，并整理 0.2.0 之后的用户可见变更。
- [x] 完成 stable Rust 格式、检查、Clippy 与 release 构建；默认测试除已记录的启动动画 ANSI 差分伪终端用例外均通过。
- [x] 合并 `dev` 到 `master`，以 `v1.0.0` 标签触发双 ABI Android GitHub Release 工作流。

## Phase 22 Termux APT 自建仓库 — 完成

- [x] 为 Termux 默认配置与运行状态引入 XDG 路径，并保留直接 Android 部署和显式配置路径兼容。
- [x] 增加可关闭的程序内自更新 feature；APT 构建只提示通过 `pkg upgrade nl2sh` 更新。
- [x] 仅为已支持的 `aarch64`/`arm` 生成独立 `.deb`，构建签名 APT 索引并通过 GitHub Pages 发布。
- [x] 增加包结构、无自更新构建、仓库索引与 ARM64 开发主机验证流程。
- [x] 增加本地双架构 Termux `.deb` 打包脚本与独立用户安装说明，不额外封装 ZIP。
- [x] 增加 Windows PowerShell 打包入口：Windows NDK 原生编译，WSL 仅负责 `dpkg-deb` 封包。

## Phase 23 TUR 与双运行环境兼容 — 完成

- [x] 增加可复制到 Termux User Repository 的 `tur/nl2sh/build.sh`，使用固定 tag、SHA-256、`termux_setup_rust` 和包管理版 feature 构建。
- [x] 集中探测直接 Android shell 与 Termux，动态切换 Agent/Command prompt、执行 shell、`/shell` 和运行环境摘要。
- [x] 保持 Android shell 为一等运行路径，Termux 为兼容路径；环境提示不参与安全分类、确认或 root 决策。

## Phase 24 1.0.1 兼容版发布与 TUR 验证 — 完成

- [x] 将 Cargo 包版本提升到 1.0.1，并整理 Android shell/Termux 动态运行环境与部署脚本变更。
- [x] 发布 `v1.0.1` 并以 tag 源码归档的 SHA-256 更新 TUR 配方。
- [x] 在 TUR 完整环境构建 `aarch64`、`arm`、`i686` 与 `x86_64`，并通过官方配方 linter、ELF 清理和符号检查。
- [x] 在 AArch64 Termux 真机验证包升级与 TUI，在支持 32 位 ABI 的真机验证 ARMv7 ELF，并在 API 27 x86/i686、x86_64 模拟器验证官方 Termux、APT 安装、版本启动、TUI 退出和终端恢复。
- [x] 向 TUR 上游提交 `tur/nl2sh` 配方，最终 PR #2804 于 2026-09-15 合并。
- [x] 完成 TUR 四架构构建、真机/模拟器运行验证及上游 Review。

## Phase 25 TUI `!` 本地命令 — 完成

- [x] 识别 TUI 中以 `!` 开头的输入并绕过 Provider，直接运行其后的命令。
- [x] 复用安全分类、确认、Root、PTY、超时、取消和终端恢复边界，编辑后重新分类。
- [x] 在当前 TUI 显示有界实时输出与退出状态，不把命令结果加入模型会话。

## Phase 26 结构化用户问答窗口 — 完成

- [x] 增加与安全审批独立的 Agent 用户问答接口，支持一次请求多个字段、候选值与自定义输入。
- [x] TUI 使用同一 ratatui frame 显示问答窗口，支持方向键、Tab、直接输入、提交和取消；非 TUI 模式提供文本回退。
- [x] Raw PCM 缺少元数据时收集采样率、声道数和采样格式，并直接合并参数后本地重试，不让模型猜测。
- [x] 问答等待不计入活跃任务时长，不改变 shell 安全分类、确认、root 或 PTY 路径。

## Phase 27 设备诊断与工具可靠性 — 完成

- [x] 任务内按路径缓存完整音频分析，质量判断优先引用缓存而非模型复制的特征对象。
- [x] Shell Tool Result 区分 `complete`、`partial`、`failed` 与 `timed_out`，保留非零退出时已有的 stdout 证据。
- [x] 增加 `/new` 空白会话和未知斜杠命令的仅提示纠错。
- [x] 增加结构化 Android 应用诊断，以及 dumpsys、logcat、settings、ContentProvider 有界只读取证工具。
- [x] 增加受控 HTTP GET/HEAD 与确认后原子下载工具。
- [x] 增加 UIAutomator 结构化状态读取和确认后截图工具。
- [x] 增加公网 TLS 证书、主机名、有效期、信任链与 SHA-256 指纹诊断工具。
- [x] 增加固定只读的 Android 环境盘点工具，以设备 ABI、命令可用性及有界资源摘要支持工具选择。
- [x] 改进 Provider/连接器错误诊断，为鉴权、限流、流提前结束、超时与 ima 失败提供本地可行动提示。

## Phase 28 Android 交互闭环与结构化运维 — 完成

- [x] 增加基于当前 UIAutomator bounds 双次校验并确认的 tap、swipe、long-press 与文本输入。
- [x] 增加 PNG 截图多模态回传；图片有硬上限且不写入会话快照。
- [x] 增加确认后的有界公网 JSON POST，保持无重定向、无 URL 凭据和私网拒绝。
- [x] 增加通知、Crash/ANR、温控功耗、流量、存储、Wi-Fi/以太网、Doze 和权限审计聚合工具。
- [x] 增加剪贴板、媒体控制/查询、连接性诊断和私有有界 Agent 便签；所有写入与控制操作继续确认。

## Phase 29 内置 Web 界面 — 完成

- [x] 交互式运行时启动内置 IPv4 HTTP 服务，欢迎消息展示设备实际地址和端口；默认端口占用时选择空闲端口。
- [x] 提供 `--web-only` 无终端入口，服务生命周期不依赖 TUI，可在重定向标准输入输出后后台运行。
- [x] 构建运行、预编译启动和一键安装脚本提供显式 Web-only 参数，并以设备端日志报告后台服务地址或错误。
- [x] 浏览器无需登录即可查看、校验并原子保存 TOML 配置。
- [x] Web 独立 Agent 会话支持对话历史、输入、流式输出、审批、编辑重评估、强确认、结构化问答及已保存会话恢复。
- [x] Web 支持多个 Agent 会话并发运行和侧栏快速切换；首轮完成后由当前 LLM 生成并持久化会话标题。
- [x] Web 合并流式增量并渲染安全 Markdown，折叠工具输出；支持快捷切换 Provider、模型和审批策略并显示会话预算与 Token 使用。
- [x] Web 后端迁移至 Axum 0.8，使用 SSE 推送 Agent 状态、WebSocket 承载安全终端；Preact + TypeScript + Vite 资源经 rust-embed 保持单 Android ELF 分发。
- [x] Cargo 编译前由构建脚本生成 Web 静态资源，发布和 TUR 构建提供 Node 工具；生成的 `web/dist/` 不纳入版本控制。
- [x] Web 执行采用捕获式路径，并继续使用 Agent 的安全分类、确认和执行边界。
- [x] Web 输入框复用 TUI 的 `@` 路径候选和引用解析；工具目录按当前配置展示注册工具及简介，支持搜索。
- [x] Web 文件列表显示类型、大小和修改时间，常见图片、视频、文本及代码格式支持有界前端预览。
- [x] Web 文本预览支持代码高亮与自动换行开关；常见音频可播放，WAV/Raw PCM 可按采样率、声道和采样格式在前端解码播放。

## Phase 30 工具架构重构 — 完成

- [x] 将文件、音频、Android、网络、UI 与便签工具实现及内部引用统一归入 `src/tools/<domain>/`，移除旧公共路径兼容导出。
- [x] 使用显式 `ToolRegistry`、`Tool`、`ToolContext` 和 `PreparedToolCall` 分离参数准备、审批与执行，移除 Runner 逐工具分发 match。
- [x] `agent_memory` 动作使用 JSON Schema 枚举约束，未知动作在风险分流前拒绝。
- [x] 从 serde 参数类型派生 JSON Schema，以 `ToolMetadata`、`ToolRisk` 和 `Capability` 统一风险下限与条件暴露。
- [x] 保留新增工具的动态写入风险、音频缺参问答/缓存、输入二次校验、截图附件及 Web 审批语义。
- [x] 增加声明式 `define_tool!` 和编译期 `#[tool(...)]`，显式注册且保持每 ABI 单一 Android 可执行文件。

## Phase 31 新手体验 — 完成

- [x] Web 首次配置快速开始：首选 DeepSeek 与 `deepseek-flash`，覆盖现有 Provider；OpenAI 使用固定官方地址，自定义服务可编辑 Base URL。填写密钥与模型后原子保存并检查模型列表，保留完整配置入口。
- [x] 空白对话增加只读示例，首次发送自动创建会话；普通视图突出任务状态，高级功能按需展开。
- [x] Web 审批以本地风险和完整待执行内容解释操作，编辑继续重新评估；常见错误给出下一步提示。
- [x] Agent 最终回答说明完成内容、证据、失败或未验证部分，以及有证据支持的修改情况。
- [x] 增加按 Termux 与电脑连接两条路径组织的新手指南。

## Phase 32 统计图表 — 完成

- [x] 增加有界只读图表工具，校验模型提供的类型、标签、数值与来源，并作为 Tool Result 持久化。
- [x] Web 在对话中绘制柱状图、折线图和饼图，提供原始数据表并支持会话恢复。
- [x] TUI 对相同结果显示标题、来源和逐项数值，保留原安全与 PTY 边界。
- [x] 保持 Web 默认监听所有 IPv4 接口且无需登录，安全分类与确认链不变。

## Phase 33 Web 会话恢复与导航 — 完成

- [x] Web 进行中任务保存私有脱敏检查点；服务重启后展示中断诊断，不自动续跑或批准操作。
- [x] 完整回答先保存，自动标题在后台更新；断线重连时清除不存在会话的过期状态。
- [x] 会话侧栏与上方菜单可独立收起；会话显示创建时间，窄屏保持可用。
- [x] TUI 首轮完整 Agent 回复后自动生成会话标题，`/sessions` 展示标题并保留稳定 ID。

## Phase 34 APK 静态分析工具 — 部分实现

- [x] 结构化只读 APK 概览、ZIP 条目检索和 DEX 类索引，限制条目、解压量和结果数量。
- [x] 单类 JADX 反编译经过 Dangerous 强确认；只接受含 `classes.dex` 的 Android helper，并通过 `CLASSPATH` + `/system/bin/app_process` 启动；离线 JAR 或 HTTPS URL + 固定 SHA-256 可在强确认后使用。
- [x] 独立 `nl2sh/jadx-helper` 仓库提供固定 JADX 1.5.1 的最小 Android helper 源码、Gradle 构建和发布元数据生成脚本；该版本兼容 API 28 的 `Inflater` 接口基线。
- [x] 发布并锁定经真机验证的 Android DEX helper，提供默认 GitHub Release 下载地址和固定摘要；自定义下载源仍强制提供独立摘要。
- [x] 在 Android API 35 模拟器通过 `app_process` 对单 DEX 和双 DEX 测试 APK 完成单类反编译；不使用设备端 Java 或安装 helper APK。
- [x] 在 Android API 28 ARMv7 设备通过完整强确认链反编译单类；运行时为 ART 提供私有临时目录，helper 限定目标类并拒绝 XML 解析。
- [ ] 在 Android API 26 真机及其他 API 版本验证，并覆盖大型多 DEX APK、内存与超时清理。

## Android 项目独立仓库 — 已实现

- [x] Android Bridge 与 JADX helper 提取模块 Git 历史，建立独立 Gradle 根工程与 Git 仓库。
- [x] 独立 PR/main CI、`v*` 标签发布、许可证、忽略规则和双语构建/发布文档。
- [x] 主仓库移除 Android 源码与 helper 构建发布 job；文档更新为独立仓库入口，历史固定 helper 下载保持兼容。
