# Project Status

Last Updated: 2026-09-26

## Recent Changes

- Web 安全终端解析 WebSocket 返回的命令结果，将 `stdout`、`stderr` 和退出状态分段显示，JSON 中的转义换行恢复为实际换行；窄屏长文本可折行，普通错误消息仍按原文显示。只改变浏览器呈现，不改变命令安全分类、审批、执行、Android 或 PTY 路径。
- Web 安全终端换行验证：Web `npm test`、`npm run build`，Linux 目标 `cargo fmt --all -- --check`、`cargo check`、`cargo test` 与 `git diff --check` 通过；Playwright 在 320px 视口用模拟 WebSocket 响应验证实际换行和无横向溢出。
- Web 响应式布局经 Playwright 在 1920×1080、1280×800、768×900、390×844 和 320×568 视口检查：中等宽度顶栏可完整换行，窄屏会话列表改为顶部横向滚动，对话输入、配置分类与字段使用可用宽度；快速开始缩短说明，高级快捷设置改为横向滚动，短屏快速开始与审批的操作按钮保持可见。仅改变浏览器静态页面，不影响 Web 默认 IPv4 监听、免登录、安全确认、Android 或 PTY 路径。
- Web 布局验证：对已连接 Android Web 服务使用 Playwright 检查对话、快速开始和配置的五种视口，高级功能、工具目录、终端弹窗及模拟待审批状态的三种视口；无页面级溢出或脚本异常，模拟审批未向设备提交命令。`npm run build`、`npm test`、`cargo fmt --all -- --check`、`cargo check`、`cargo test` 与 `git diff --check` 在 Linux 目标通过。
- Web 快速开始首选 DeepSeek，预填 `deepseek-flash`；补齐 OpenRouter、OpenAI、Moonshot/Kimi、SiliconFlow、Ollama 与自定义服务。OpenAI 使用固定官方地址并要求 API Key；Ollama 和自定义服务可在下一步编辑 Base URL；切换服务时清空上一服务的 API Key。仅调整前端引导与文案，Web 仍默认监听所有 IPv4 接口且无需登录。
- 快速开始预设验证：Web `npm run build`、`npm test`，Linux 目标 `cargo fmt --all -- --check`、`cargo check` 和 `cargo test` 通过；预设测试覆盖服务列表、DeepSeek 首选与默认模型、现有配置保留、OpenAI 固定地址及其密钥要求，以及自定义服务的地址编辑能力。Playwright 检查 OpenAI 下一步只显示模型和 API Key，自定义服务仍显示 Base URL。
- 新手体验分阶段接入：Web 首次缺少 Provider 凭据时打开快速开始，支持服务选择、配置原子保存与只读模型列表检查；空白对话提供只读示例，首次发送自动创建会话。普通视图突出对话和任务状态，高级功能可展开；常见错误附下一步提示与配置入口。Web 审批显示本地规则依据、中文风险、完整待执行内容和强确认提示；编辑后仍经过原审批链。TUI 欢迎内容补充自然语言与拒绝说明，Agent 最终回答要求说明证据和未验证部分；新增跨 Termux/电脑连接路径的新手指南。Web 默认监听所有 IPv4 接口且无需登录的行为保持不变。
- 新手体验验证：Linux 目标 `cargo fmt --all -- --check`、`cargo check`、`cargo test`，Web `npm run build`、`npm test`，以及配置 NDK clang/llvm-ar 后的 Android API 26 AArch64 `cargo check --target aarch64-linux-android` 通过；新增本地安全规则说明回归。
- 新增 `inspect_android_environment` 固定只读工具，返回 Android 版本、设备 ABI、常见命令可用性、内存与数据分区容量；运行摘要区分设备 ABI 与进程架构，Agent 在建议设备端程序时以设备 ABI 为准。`android_connectivity` 改为有界 ICMP、路由及默认网络摘要，并明确不代表 HTTPS 已验证。
- 安全分类修正引号内输出箭头与只读 `mount` 列表的误报；真实重定向、命令替换中的写入与挂载修改仍需确认。Linux 目标 `cargo fmt --all -- --check`、`cargo check`、`cargo test` 通过；Android API 26 ARMv7 `cargo check --target armv7-linux-androideabi` 通过。
- Web 会话侧栏左上角显示项目 logo，顶栏右上角增加可跳转仓库首页的 GitHub Star 链接；窄屏布局保留入口。仅调整浏览器静态资源和样式，不改变安全确认、Android 或 PTY 路径。
- Web 页头验证：`npm run build`、`npm test`、`cargo fmt --all -- --check`、`cargo check` 与 `cargo test` 在 Linux 目标通过。
- TUI 启动欢迎内容末尾突出显示 Web 访问地址，使用独立入口、主题语义色和窄屏换行；仅改变本地显示，不影响模型上下文、审计、安全确认、Android 或 PTY 路径。
- Web 欢迎入口验证：Linux 目标 `cargo fmt --all -- --check`、`cargo check` 与 `cargo test` 通过；新增测试覆盖 ANSI 256 强调样式与窄屏换行。
- Web 会话侧栏新增单条删除和删除全部按钮；单条直接删除，删除全部前确认，同时清理内存条目与私有快照；运行中、等待审批或终端仍连接时拒绝删除。会话列表、恢复与任务启动同删除路径同步，避免迟到写回。删除范围不含其他状态文件，安全审批、Android 和 PTY 执行路径不变。
- 单条删除交互调整后，Linux 目标 `cargo fmt --all -- --check`、`cargo check`、`cargo test`、`npm run build` 与 `npm test` 通过。
- Web 会话删除验证：Linux 目标 `cargo fmt --all -- --check`、`cargo check`、`cargo test`、`npm test` 与 `npm run build` 通过；HTTP 回归覆盖单条和全部删除、内存与磁盘同步、其他状态文件保留及运行中拒绝删除。
- Web 配置页增加与 TUI 对齐的功能分组字段编辑和 TOML 文件编辑切换；浏览器输入后调用同一 Rust 配置解析及运行校验，显示错误并禁止保存无效配置。保存仍原子写入，后续 Web 任务自动读取新配置；当前 TUI 会话重启后读取。
- Web 配置编辑验证：`cargo fmt --all -- --check`、`cargo check`、`cargo test`、`npm run build` 与 `npm test` 在 Linux 目标通过；HTTP 回归覆盖无效 TOML、默认配置解析、字段回写与保存后重新加载。
- 准备发布 `v1.0.2`：版本号和 changelog 已从 Unreleased 收敛到 2026-09-24 的补丁版本；tag 推送继续通过既有工作流构建双 ABI Android、自更新裸二进制、Termux `.deb`、签名 APT 仓库及 GitHub Release。
- Web 工具调用改为逐项实时显示：Agent display sink 在每个工具开始与完成时携带 `call_id` 发布事件，浏览器立即显示调用卡片并按 ID 回填成功或失败结果；任务结束仍以完整 transcript 持久化会话，但当前消息列表不再重复追加同一工具轮。此变更不改变工具串行执行、安全确认、Root、Android 或 PTY 边界。
- Web 会话输入框下方显示空闲、模型思考、工具执行和等待审批/补充信息状态，并每秒更新当前状态时长；轮次、步骤、工具、Token 与 Root 统计也移至输入框下方。状态由现有 Agent 执行事件驱动，不改变安全确认、Android 执行或 PTY 路径。
- 工具公共路径统一为 `src/tools/<domain>/`：内部引用已迁移，旧顶层兼容导出已移除。此项改变 Rust 库模块路径，不改变工具注册、安全确认、Android 执行或 PTY 行为。
- Web 输入框新增与 TUI 共用的 `@` 文件/目录候选和路径引用提示；右上角工具目录展示当前配置下注册的工具名称、简介及搜索过滤。候选读取有界，目录接口只读；Agent 安全分类、确认、Root 与 PTY 边界不变。
- 工具架构重构：文件、音频、Android、网络、UI 和便签实现迁入 `src/tools/<domain>/`；显式 Registry 分发全部内置工具，参数类型派生 JSON Schema。风险元数据及动态单次风险决定统一审批，预备动作获批后执行；音频缺参问答与缓存、输入二次校验、图片附件、Web 审批、Shell 动态分类/Root/PTY 保持既有边界。`define_tool!` 与编译期 `#[tool(...)]` 均不自动授予权限。
- Web 对话支持 F2 同步展开或收起所有工具卡片，同时保留每张卡片的独立点击控制；审批和终端弹窗优先接收键盘事件。
- Web 工具结果及实时输出在展示时将字面量 `\\n` 还原为换行，保留已有真实换行与成对反斜杠；仅影响浏览器显示，不改写原始 Tool Result、模型上下文或审计数据。
- Web 对话不再把连续工具事件合入同一折叠项：每次调用与按 ID 匹配的结果独立显示，已保存会话恢复沿用相同顺序；原始实时输出仍单独呈现，不改变执行和审批边界。
- Web 页面配色统一到 TUI TrueColor 语义色板：背景、文字、导航、焦点、Markdown 与状态色由 CSS token 集中管理；不调整 Agent 审批、安全或执行路径。
- 修复 TUI 消息历史滚动后的旧字符残留与整屏闪烁：滚动位置变化时补画消息区域的全部可见单元格，包括宽字符后与短行右侧的空格，不再整屏清除；普通 TUI 与 Agent 会话共用此绘制路径，不改变输入、安全确认或 PTY 行为。
- Web 顶栏新增当前会话日志导出：下载 ZIP 包含导出时可见对话的 `conversation.json`、当前共用审计日志 `nl2sh.log` 及范围说明；空日志也可导出。导出只读取数据，不改变 Agent 安全审批或执行路径。
- Web 资源改为 Cargo 编译前从锁定的 npm 依赖自动生成；发布 CI 固定 Node 版本，TUR 源码含构建脚本时使用主机 Node 工具，`web/dist/` 不再纳入版本控制，Android 仍交付单一可执行文件。
- 内置 Web 服务默认优先监听 9999，端口占用时再选择可用端口；侧栏选中已保存会话时先恢复快照，再加载并显示对话历史，切换时忽略旧会话的迟到响应。
- Web 模型回答与流式文本改用完整 Markdown 解析，支持表格、编号列表、嵌套内容和代码块；原始 HTML 保持转义，危险链接不生成可点击地址，并补充对应样式与回归测试。
- Web 技术栈重构为 Tokio + Axum 0.8 + serde JSON：Agent 更新使用 SSE，安全终端使用 WebSocket；前端迁移为 Preact + TypeScript + Vite + 纯 CSS，生产资源由 rust-embed 编入单一 Android ELF。
- Web 对话增加安全 Markdown 渲染和默认折叠的工具输出，流式增量保持在同一段；顶部快捷栏支持切换 Provider、模型和审批策略，并显示轮次、步骤、工具调用、Token/上下文和 Root 状态。
- Web 界面升级为类似 dsh 的多会话工作区：会话拥有独立运行、输出、审批和问答状态，可在多个后台 Agent 之间快速切换；首轮完成后由当前 LLM 自动生成短标题并随会话持久化。
- 新增内置 Web 页面与 HTTP 服务：欢迎消息显示设备 IPv4 URL；浏览器无需登录即可编辑、校验并原子保存 TOML 配置。Web 提供独立 Agent 对话、流式文本和命令输出、审批与命令编辑、结构化问答、新会话及历史会话恢复；所有命令仍经过原安全分类、确认和执行链。
- 修复 Agent 流式输出期间主输入框无法编辑：运行中的普通按键继续进入输入框，Enter 在任务结束前保留草稿；审批与用户问答弹窗仍优先接收按键，安全与 PTY 路径不变。
- 修复 Provider 在工具轮之后超时导致“继续”丢失上下文：Agent 错误携带已完成的部分 transcript，TUI 将包含工具证据的失败 turn 纳入当前模型历史并自动保存；初次请求即失败仍不产生空会话，安全确认、Root 与 PTY 边界不变。
- 修复长期超时的 TUI 保活回归：用例不再从 ratatui 原始差分 ANSI 字节流匹配连续中文，也不再混测 `/help` 与 `/clear`；测试关闭启动装饰、复用统一 PTY 启动器，并以合法 Responses SSE 增量及完成事件验证 Agent 回答显示、TUI 保活、Ctrl+Q 退出和审计记录。
- 权限确认新增“本次运行全部允许”选项，并新增 `/permission [status|allow|ask]` 本地命令；运行期许可只存在于当前进程内存，仅自动批准非 Root、无需强确认且最高为 `Mutating` 的操作，Dangerous、Critical、Root 与强确认仍必须逐次审批。
- 修复 Android 视觉任务闭环：截图查看接受 PNG/JPEG/WebP，超出 2 MiB 时在进程内有界缩放；UI 检查默认仅返回可操作、有标签或聚焦节点，输入成功后直接附带最新界面状态。固定结构化探测改用静默执行，不再把内部 UI XML 或 dumpsys 原文重复写入实时输出与审计 sink。
- 修复 ContentProvider/MediaStore 假成功：投影支持结构化列数组并规范化为 Android `content` 语法，识别退出码为零但正文含 provider 异常的结果；媒体列改为 `datetaken`、`date_added` 与 `_size`，支持按创建时间过滤、倒序和兼容投影回退。最终回答约束禁止从拍摄元数据推断画面内容，视觉检查失败时必须明确报告部分完成。
- 新增 Android 交互闭环与结构化运维工具：UI bounds 双次校验输入、PNG 多模态查看、确认后 JSON POST、通知/Crash/ANR/功耗/流量/存储/网络/Doze/权限聚合、剪贴板、媒体、连接性和持久便签。所有写入、远程 POST、输入注入及媒体控制均保持独立确认；图片附件不进入持久会话。
- 新增 `inspect_tls` 公网 TLS 诊断：直连握手并验证 SNI 主机名、证书有效期和 Mozilla 信任链，返回各级证书主题、颁发者、起止时间与 SHA-256；拒绝本机/私网目标，不发送 HTTP 请求。
- TUI 为 Provider 401、429、流提前结束、网络超时及 ima 连接失败附加本地诊断和 `/config` 操作建议；原始错误仍保留，401 继续立即失败且不重试，流式残片不冒充完整回答。
- 新增 `inspect_android_ui`，通过自动清理的内部临时 XML 返回有界 UIAutomator 控件节点、焦点窗口及显示尺寸/密度；新增 `capture_android_screen`，仅在用户确认后把当前画面写为指定 PNG，不提供自动点击或输入。
- 新增受控 `http_request` 与 `download_url`：只允许公网 HTTP(S) GET/HEAD，禁用重定向、URL 凭据和私网目标并限制响应大小；下载先获取有界内容并展示 URL、字节数和目标，确认后才同目录原子写入。
- 新增 `inspect_android_app`、`android_dumpsys`、`android_logcat`、`android_settings` 和 `android_content_query` 结构化只读工具；参数拒绝 shell 元字符，写设置、service call 与 ContentProvider 写操作不在工具接口中，结果逐项标记完整、部分、失败或超时。
- Agent 音频质量判断现在优先引用当前任务内按路径缓存的完整 `analyze_audio` 结果，避免模型复制特征时遗漏 `status` 或改写数值；缓存不跨任务持久化。
- Shell Tool Result 新增 `complete`、`partial`、`failed`、`timed_out` 状态，非零退出但已有 stdout 的复合只读查询会保留为部分证据。
- TUI 新增 `/new` 空白会话命令；未知单词型斜杠命令会提示最接近的本地命令但绝不自动执行，也不会提交给模型。

- 新增与安全审批独立的结构化用户问答窗口：支持多字段候选选择和自定义输入；Raw PCM 缺少采样率、声道数或采样格式时在 TUI 内收集答案并直接本地重试，非 TUI 模式提供文本回退，取消不猜测也不改变安全、root 或 PTY 边界。
- 修复音频工具集成测试文件末尾误写的字面量 `\\n`，恢复测试 target 编译；同时应用 rustfmt 标准格式，不改变音频分析、LLM 判断、安全确认、Android 或 PTY 行为。
- TUR PR 已以 #2804 重新提交，并按 review 将 maintainer 改为 `Name <email>` 格式、移除不必要的显式 license file 与 API level；四架构 CI 曾在实际编译前受上游重复 `bazel` 配方影响，PR 分支已 rebase 到包含上游修复的最新 `master` 以重新触发构建。
- GitHub 主仓库、开发分支、版本标签与 Release 历史迁移到 `nl2sh/nl2sh` 组织；自更新 API、TUR 源码、Debian 包主页、APT Pages、README/TUI 支持链接及 Cargo 包元数据已统一指向新地址，并更新项目 logo。
- TUI 支持 Codex 风格的 `!command`：不请求 Provider，直接经过既有安全分类、确认、Root 与 PTY 执行链，并在当前界面显示有界实时输出和退出状态；命令及结果不进入模型上下文，未配置 Provider 时也可使用。
- 发布 `v1.0.1`，并将 TUR 配方更新为该 tag 的固定 GitHub 源码归档及 SHA-256；GitHub Release 工作流状态由 Actions 最终结果确认。
- 新增可提交到 Termux User Repository 的 `tur/nl2sh/build.sh`：固定 release 源码与 SHA-256，使用 TUR/Termux 构建系统的 Rust toolchain 和目标架构构建，并关闭包内自更新。
- 集中识别直接 Android shell 与 Termux：Android shell 保持 `/system/bin/sh`/toybox 一等基线，Termux 兼容模式动态使用 `$PREFIX/bin/sh`、XDG 与包管理提示；Agent、Command、运行摘要和 `/shell` 保持一致，安全分类、确认、root 与 PTY 边界不变。
- 新增 `android-build-tmux-run.sh`：自动选择安装 Termux 的 ADB 设备并识别 `aarch64`/`arm`，只构建匹配架构的包管理版 `.deb`，推送到 Android 临时目录后通过 ADB 转发 SSH 进入 tmux；已有同名会话时新建部署窗口，确保仍会安装新包并运行 `nl2sh`。
- 新增 `pack-termux-release.sh`：本地构建 `aarch64`/`arm` 包管理版本并直接输出两个独立 `.deb`，不额外封装 ZIP、不接触仓库签名私钥；独立 Termux 安装说明保留在仓库中。
- 新增 `pack-termux-release.ps1`：使用 Windows NDK 原生构建双 ABI Android 二进制，通过安全的 `wslpath` 参数转换仅调用 WSL `dpkg-deb` 封包，支持项目路径包含空格。
- 新增自建 Termux APT 仓库打包与 GitHub Pages 签名发布，只覆盖已支持的 `aarch64`/`arm`；包管理构建关闭程序内自更新并引导使用 `pkg upgrade nl2sh`，Termux 配置/日志/会话分别遵循 XDG config/state 路径，直接 Android 部署兼容路径不变。
- `/sessions` 改为弹出按更新时间倒序排列的最近会话列表，保存时间以设备本地日期和分钟显示；可直接输入序号或使用 Up/Down 与 Enter 选择恢复，原有命名恢复、重命名和删除子命令继续可用，会话数据边界不变。
- 新增 OpenRouter 内置 Provider 与 OpenAI-compatible 模型发现；新配置默认使用 `https://openrouter.ai/api/v1` 和 `openrouter/free`，远程调用仍要求 API Key，现有安全确认、Android 与 PTY 边界不变。
- 启动小火车改为主题 palette 驱动的彩色 ASCII Art：烟雾、车顶、车身、`NL2SH` 字样与轮组分层着色，继续支持 TrueColor/ANSI 256、窄屏裁剪，且不改变动画生命周期或安全边界。
- 准备 1.0.0 正式版：Cargo 包版本提升到 1.0.0，汇总 0.2.0 之后的功能、稳定性与安全边界变更，并沿用双 ABI Android 标签发布流程。
- ima 设置页明确将默认知识库 ID 标注为可选；留空时继续有界发现可访问知识库，不改变搜索逻辑。
- 新增可选的腾讯 ima 只读知识库连接器：独立 Client ID/API Key、始终无代理直连，仅提供知识库发现、搜索与有界原文读取 Tool；支持笔记正文和受控临时 URL，禁止写操作、重定向、非 HTTPS/非白名单来源及凭据进入模型、日志或会话。
- TUI 输入新增 `@` 文件/目录引用候选：支持相对路径、绝对路径、`@~`、`@/`、`@.` 和父目录路径，Up/Down 选择、Enter/Tab 补全，目录可继续下钻；Right 保持普通光标右移。提交时按最长已存在路径前缀解析，`@test.txt写的是什么内容` 无需额外空格；只附加绝对路径提示，内容仍由有界结构化文件工具读取，安全与确认链不变。
- 命令批准弹窗改为内容驱动的动态宽高：长命令和结构化 diff 超过终端可用高度时可用滚轮或 PageUp/PageDown 浏览，上方正文独立滚动，底部审批选项、强确认或编辑输入保持可见。
- 新增结构化 `read_file`、`list_dir`、`search_text` 与 `apply_patch` Agent 工具：路径不设工作区边界并支持绝对路径、父目录和符号链接，读取/遍历/匹配/文件大小仍有界；补丁先展示 diff 并确认，再原子写入。
- 新增私有会话快照与 `/sessions` 管理：已完成 turn 自动保存，支持列表、恢复、重命名和删除；恢复重新应用上下文与 Tool Result 上限，Provider 凭据、代理密码、余额和临时审批许可不进入会话文件。
- Windows ADB 启动路径新增 alternate-scroll 兼容模式：不请求远端鼠标捕获，让 Windows Terminal 将滚轮转换为 Up/Down 事件，再由 TUI 滚动历史；Linux 路径保留原生鼠标捕获，命令候选菜单仍优先使用方向键导航。
- Agent Runtime 新增独立 Step、Tool Call、活跃任务时长、连续停滞、重复动作和系统硬 Step 上限；默认 Normal 为 50 Step、100 Tool、30 分钟，另有 Fast/Deep 预设。确认等待不计时，所有命令仍完整经过安全分类和确认链。
- 相同规范化命令连续产生相同结果三次后会在下一次执行前阻止；连续无进展会先强制重新规划再终止，80%/90% Step 水位提示模型优先收敛。任务结束状态和审计事件新增步骤、工具调用、活跃时长、停滞、重规划及限制原因摘要。
- LLM 协议默认改为可省略配置的 `auto`：首次请求优先 Responses，仅在尚未输出内容的协议结构不匹配时回退 Chat Completions 并缓存；401、429、5xx、超时和部分流式输出错误不会误触发切换，显式协议仍可强制覆盖。
- 设置面板“界面”分类新增清除审计日志操作，以及默认开启、互相独立的佛像与小火车 ASCII Art 开关；清除后当前进程可继续写入新日志。
- 设置面板文本字段现在维护独立 UTF-8 光标，支持 Left/Right/Home/End 定位编辑；切换字段或分类时同步到新字段末尾，密码掩码光标仍与原始字符位置一致。
- 统一设置面板的“模型与智能体”Tab 新增在线模型列表操作，后台复用 Provider 元数据客户端，成功后在面板内选择并回填模型、上下文窗口和最大输出 Token，失败不覆盖当前手工配置。
- 输入边界现在同时过滤完整 `[<b;x;yM/m` 和 adb 丢失 CSI 后的 `<b;x;yM/m` SGR 鼠标报告，并覆盖主输入框与设置文本字段。
- 本地命令边界统一为“所有 `/` 开头输入均不进入 LLM”；修复 `/update` 执行后继续落入 Agent 的问题，未知斜杠命令现在仅显示本地提示。
- 修复 `/config` 本地命令打开面板后继续落入 Agent 提交流程的问题；设置面板打开时主输入框失焦，当前文本字段显示独立输入边界、背景和闪烁光标。
- 配置入口收敛为 `/config` 和别名 `/setting`，移除 `/provider`、`/model`、`/models`、`/proxy` 的候选与 Agent TUI 路由。
- 新增 `nl2sh update`、`/update` 与启动后台检查：按 Android ABI 获取 GitHub Release 裸二进制，经独立 SHA-256 校验后原子替换；提示支持立即更新、暂不更新和跳过此版本。
- 配置命令统一进入分类 TUI 设置面板；Tab/Shift+Tab 切分类，Up/Down 切字段，Left/Right 调整当前值，最大步骤与轮次显示推荐值 24/16。

- 支持余额接口时，Agent TUI 会在启动后及每 60 秒静默刷新，最近一次成功余额常驻顶栏，失败保留旧值；手工 `/balance` 立即刷新，余额仍只存在内存且不进入对话、配置或审计历史。
- 新增 `/proxy` Agent TUI 配置弹窗，支持 HTTP CONNECT、SOCKS5/SOCKS5H、认证和绕过列表；总开关关闭时保留配置。LLM、模型发现、Ollama 元数据和余额查询统一使用同一代理策略，密码仅掩码显示且不进入日志或模型上下文。
- `/proxy` 弹窗复用方向键碎片序列过滤：CSI/SS3 左右键不会再因先到达的 Esc 字节而关闭弹窗，独立 Esc 在短暂组合窗口后仍可取消。
- Agent 与单命令 prompt 在直接 Android shell 中以 `/system/bin/sh` 和 toybox 为一等基线；Termux 兼容模式改用 `$PREFIX/bin/sh`、XDG 和包管理基线，两者都必须先只读探测可选程序。
- 普通输入路径现在也统一过滤碎片终端序列，将部分 PTY 的 `Esc O Q` 还原为 F2，避免循环展开/收起工具结果时把 `OQ` 写入输入框。
- Agent 根据已知 Context Window、最大输出预留和 Provider 实际输入 Token 动态淘汰最旧完整历史轮次；system instruction、当前交互和 Tool Calling round 不拆分，配置的轮次与步骤上限仍是硬边界。
- 新增不记入审计的 `/balance`：使用现有 API Token 查询 DeepSeek 与 SiliconFlow 的公开只读余额接口；其他未提供稳定 Bearer Token 余额接口的国内外 Provider 明确显示不支持，不调用控制台私有接口。
- 新增独立 `ProviderMetadataClient`，分别适配 OpenRouter、OpenAI、DeepSeek、SiliconFlow 的模型列表与 Ollama 原生模型详情；配置支持上下文窗口/最大输出 Token 覆盖，已知窗口用于在状态栏估算最后一次请求的上下文占用率。
- Agent 任务会累计所有模型步骤返回的输入/输出 Token，并在 TUI 状态栏展示本次任务合计；新增 `/models` 在线模型选择，网络或协议失败时回退手工输入，凭据和原始 Provider 响应不写入审计日志。
- 启动小火车由约 10 FPS 提升到约 30 FPS，并将每帧位移由两列改为一列，以减少跳格卡顿；TUI 事件轮询与异步刷新周期同步缩短，动画约 4.1 秒后结束。
- 修复启动小火车车头与向右行驶方向相反的问题；车体和烟雾现朝向右侧，`NL2SH` 字样保持正向，奇偶宽度下仍会让车头贴到内容区右边缘后再驶出。
- 修复 ADB TTY 在 `/` 命令菜单首尾继续按方向键时可能把拆分的 CSI/SS3 尾字符写入输入框、导致菜单消失的问题；菜单打开时会将 `Esc [ A/B/C/D` 与 `Esc O ...` 重新组合为方向键，首尾循环选择保持不变。
- 修复部分 ADB 宿主终端在 LLM 流式临时文本切换为最终 Markdown 时留下旧渐变字符的问题；流结束后由 TUI 主线程执行一次完整重绘，不改变终端模式、PTY 或安全流程。
- Chat Completions 与 Responses 的模型文本现在通过 SSE 增量显示到 Agent TUI；生成中的尾部使用 TrueColor/ANSI 256 语义渐变动画，完成后立即恢复普通 Markdown 样式。流式工具参数仍完整聚合后才进入安全与确认链。
- 每个 Android Agent 任务会向 system prompt 动态附加一次低敏感环境摘要（API level、ABI、shell、UID、root/su 能力）；失败时安全降级，不采集设备标识或网络信息，也不影响安全与确认链。
- 新增本地 `/exit` 命令，可从命令候选菜单或直接输入安全退出 TUI，行为与 Ctrl+Q 一致且不会进入模型上下文。
- 修复启动小火车以每帧两列移动时可能跨过右边缘贴边帧的问题；奇数和偶数宽度下车头都会抵达内容区最右列后再完整驶出。
- 首次启动欢迎页会在佛祖图下方播放一次带动态蒸汽和 `NL2SH` 车身字样的纯 ASCII 小火车；`android-build-run.sh` 会先把宿主终端行列数同步到 Android PTY，动画再按真实内容宽度移动；窄屏按视口裁剪，且不进入会话、审计或模型历史。
- 佛祖终端图的 `\\`、`/`、`|`、`=`、`^` 光芒与轮廓字符使用独立的装饰金色 token 加粗显示，文字和面部细节保持正文色；该 token 不复用安全警告色，历史内容仍为无 ANSI 的纯文本。
- README 在已知限制之后新增“支持项目”区块，包含 Star/Issue 话术、赞赏说明、可点击的在线微信赞赏码及备用文字链接；TUI 仍保持纯文本方案。
- 修正终端佛祖祝福图的 Unicode 显示宽度：含中文的祝福行与 ASCII 外框统一为 65 列，避免右侧突出。
- 缺失配置时不再自动运行启动向导，而是直接进入不可执行模型任务的 TUI；新增 `/provider` API 配置和 `/model` 模型配置入口，`/config` 保留完整配置能力。
- README 顶部居中展示仓库内的 `assets/logo.png`；启动欢迎页与 `/help` 显示项目 Star/Issue 支持链接、在线微信赞赏链接和纯文本终端祝福图，TUI 仍不内嵌图片或渲染二维码点阵。
- 新增本地 `/help` 帮助与 `/clear` 会话清理命令；清理可见对话、模型上下文和输入历史，但保留审计日志。
- 修复 Android ADB TUI 退出后宿主终端仍处于 SGR 鼠标追踪模式的问题：Rust 正常/异常恢复路径统一关闭鼠标捕获，`android-build-run.sh` 在 ADB 正常、失败或中断退出后额外执行宿主侧兜底恢复。
- 默认 `max_agent_steps` 由 8 提升到 24、`max_context_turns` 由 10 提升到 16，以支持安装后验证、多阶段诊断等更长任务；`config.toml.example` 与默认值测试同步更新。

## Current Phase

Android 交互闭环与结构化运维工具已实现并进入验证；1.0.1 TUR 上游配方仍在等待 CI/Review。

## Overall Status

- Product positioning: 以 Android 原生 shell 为一等环境、Termux 为兼容环境的类 Hermes AI Agent；核心程序以单个可执行文件交付，提供多轮 Tool Calling 和丰富 TUI，不声称与 Hermes API 或插件兼容。
- Build status: 1.0.1 的 stable Rust 检查、Clippy 与 AArch64 API 26 包管理版 release 交叉编译通过。
- Test status: 全量 `cargo test --all-targets` 通过；显式凭据 ima live smoke 按设计忽略。
- Android cross-compile status: GitHub Actions 使用 NDK r28c、API 26 构建 `aarch64-linux-android` 与 `armv7-linux-androideabi` release 产物。
- Android device validation: 已完成真机 root/非 root、修改确认、命令超时和全屏交互程序验证矩阵。
- CI release workflow: 已添加 `.github/workflows/release.yml`，在推送 `v*` tag 时用 GitHub Actions 并行交叉编译 `aarch64-linux-android` 与 `armv7-linux-androideabi`，将两个程序放入统一包的 ABI 子目录，并与自动选择设备/ABI 的 Linux/Windows BAT 启动脚本、`config.toml.example`、`使用说明.md` 打包为单一 `.tar.gz`/`.zip`，附带 SHA256 校验和发布到 GitHub Release；`workflow_dispatch` 可手动触发草稿发布。
- Known blockers: 无；标签发布产物状态由 GitHub Actions 最终结果确认。

## Completed

- 动态宽高、长内容可滚动且操作区固定的审批弹窗；Up/Down 仍只负责选项导航。
- 结构化文件工具的无工作区路径限制、资源大小、唯一替换、确认前不写入和原子替换边界。
- 会话自动保存与 `/sessions` 列表、恢复、重命名、删除；私有文件权限及敏感运行态排除。
- `/shell` 退出后显式清除 ratatui 差分缓存，确保恢复 alternate screen 后完整重绘 TUI，而非只绘制差异导致界面缺失。
- Provider 设置在面板会话内分别保留 Ollama 与 Custom 的 Endpoint 草稿，切换到其他内置服务商再返回时恢复此前输入。
- 统一 TUI 设置面板的“服务”分类恢复内置 Provider 选择，复用向导中的 OpenRouter、OpenAI、DeepSeek、Moonshot/Kimi、SiliconFlow、Ollama 与 Custom 预设；切换只回填 Endpoint，不覆盖 API Key、模型或协议。
- 本地 `/shell` 可暂停 TUI 并进入系统交互 shell，使用 `exit` 或 Ctrl+D 后回收子进程、恢复终端并重绘原会话；shell 内容不进入模型上下文或审计日志。
- Android shell 版类 Hermes AI Agent 的产品定位，以单文件 Android 可执行程序和丰富 TUI 为主要交付形态。
- 模块化 Cargo 工程、CLI、配置加载/校验/向导。
- 两种 OpenAI API adapter 与统一 LLM trait。
- Agent loop、shell tool、真实结果回传和最大轮数。
- Agent 任务级 Step/Tool/时间/停滞/重复动作组合预算、Fast/Normal/Deep 预设和不可由普通配置绕过的硬 Step 上限。
- 四级安全评估、内置/自定义规则和确认接口。
- root 模式解析、su 参数化执行、pipeline timeout 和进程组清理。
- 可持续多轮输入、完整历史回放、滚动和 terminal guard 的 TUI。
- openpty 主执行器、pipeline fallback、交互双向桥接、resize、ANSI 过滤和实时 output sink。
- Ctrl+C 对 LLM 请求/退避及命令进程组的取消路径；编辑命令重新分类。
- endpoint/model/api-type CLI 覆盖，覆盖后统一配置校验。
- 隔离子进程 SIGINT 回归测试，验证 Agent 退出及 PTY 进程组回收。
- 单 frame Agent TUI、内嵌确认弹窗、实时状态/输出，以及真实伪终端生命周期回归测试。
- 缺失配置时直接进入 TUI，普通任务在 Provider 配置完成前被本地拒绝；`/config` 可完成全部设置，`/provider` 配置 API Endpoint、API Key 和协议，`/model` 单独配置模型，并在保存后热重载客户端。
- TUI 底部输入框独占一行，运行状态、轮数和剩余上下文在下一行显示。
- 对话历史按用户、Tool、Agent、命令、成功和错误等语义类型使用不同颜色。
- 对话历史逐条持久化到默认配置目录下的 `0600` JSON Lines 日志，供异常排查。
- 只读应用版本查询及其命令替换循环不再误判为修改操作；替换内副作用仍需确认。
- TUI 捕获滚轮以浏览历史；按住 Shift 拖选时由宿主终端原生高亮选区并通过右键菜单复制，PageUp/PageDown 仍可浏览历史。
- 输入框使用统一主题的青蓝色闪烁光标和聚焦边框，支持 Left/Right/Home/End/Delete 定位编辑，并可用 Up/Down 调取当前会话已提交的输入历史。
- 输入以 `/` 开头时显示垂直命令候选菜单；Up/Down 选择，Enter 补全，列出 `/help`、`/clear`、`/config`、`/provider` 和 `/model`。
- TUI、初始化向导与安全确认支持中文/英文，默认中文；启动历史区预置常用 Android 任务和操作说明，且不进入模型上下文。
- 输入行和低权重状态行使用统一主题的 `background_alt`，快捷键分隔线使用 `border`/`border_focus`，其余 frame 使用非纯黑 `background`。
- `android-build-run.sh` 会选择或连接 ADB 设备、查询 ABI 并自动选择对应 Rust target，再执行 root adbd、交叉编译、推送和启动；不支持 root adbd 时才回退 `su -c`，私有配置不可读则提前失败。
- `android-build-run.ps1` 使用 NDK `windows-x86_64` LLVM 工具链在 Windows PowerShell 原生完成同等的设备选择、ABI 自动编译、root/su 回退和启动流程，无需 Bash 或 WSL。
- `android-run-linux.sh` 与可双击的 `android-run-windows.bat` 会在无设备时提示连接网络 ADB、单设备自动选择、多设备按编号选择，再查询 ABI 并从 `bin/arm64-v8a` 或 `bin/armeabi-v7a` 推送对应程序；同时保留 root adbd 优先、`su` 回退和私有配置保护。
- 部署文档说明了文件存在却由 ABI/ELF interpreter 不匹配引发 `No such file or directory` 的情况，并给出 AArch64/ARMv7 识别、重建和验证步骤。
- adb TTY 将鼠标 SGR 序列拆成按键字符时，输入边界会过滤 `[<数字;数字;数字M/m`，且空闲 Esc 不再误清已有输入；确认弹窗 Esc 行为不变。
- `/dev/null` 重定向与 fd 复制不再把只读诊断误判为修改或连带要求 root；真实文件写入和命令副作用仍受确认保护，strict 仍按定义确认全部命令。
- 工具执行期间保留有界实时输出，完成后结果默认折叠并可用 F2 展开/收起；模型接收带显式截断标记的有界结果，最终答复被要求使用用户语言及可读表格或文本总结。
- Agent Markdown 原生映射为 ratatui 行与样式，支持标题、行内样式、列表、引用、代码块、链接和分隔线；表格按 Unicode 宽度对齐、换行，并在窄屏降级为键值列表。
- F2 展开工具结果后按 ratatui 实际换行高度定位和滚动，不再用逻辑历史条目数限制大结果，长命令与输出可完整浏览。
- 交互命令退出后恢复备用屏幕与鼠标捕获，并使 ratatui 强制完整重绘，避免第二轮对话只显示结果而框架消失。
- 配置、安全、root、HTTP mock 和 Agent loop 测试源码。
- GitHub Actions release workflow：tag 推送自动构建 AArch64/ARMv7 Android release、打包快速启动脚本并发布 Release。
- `pack-release.sh` 与 `pack-release.ps1` 可在 Linux/Windows 本地构建双 ABI，并输出与 GitHub Release ZIP 相同的 `nl2sh-android/` 目录结构和 SHA256 校验文件。
- 面向普通用户的中文使用说明，覆盖 ADB 连接、Linux/Windows 启动、自动设备/32/64 位选择和常见故障，并纳入统一 release 压缩包。
- `screenshots/nl2sh.gif` 动态操作演示嵌入中文使用说明与 README，release 压缩包同步包含 `screenshots/`，保证打包后的说明动图完整。
- 已建立 `UI_DESIGN.md`，统一定义深色 TUI palette、语义颜色、各界面区域样式、ANSI 256 fallback、实现边界和验收标准；规范明确颜色不得改变或替代安全分类与确认流程。
- 已在 `src/tui/theme.rs` 实现集中式 Theme/Palette 与 TrueColor/ANSI 256 能力选择，并迁移标题栏、对话、Markdown、工具结果、表格、快捷键、输入区、状态栏、命令菜单和确认弹窗；长正文与 stdout 不再继承成功绿色。
- 命令审批改为固定 `1-6` 列表，支持方向键/Enter 与 `y/n/a/e/i/t` 别名；可在当前 Agent 任务内记住完全相同的普通命令，但 Root、Dangerous、Critical 和强确认命令始终禁用该选项，且许可不持久化、不做前缀匹配。
- 审批区域使用完整风险色边框和统一 `background_alt` 面板背景；阶段切换保持稳定最小高度并清空整个面板，避免列表字符残留到强确认或编辑画面。
- 审批面板锚定在输入区正上方的左下角；初始审批忽略孤立 Esc 和大写 CSI 尾字符，避免 adb 将方向键拆分后误触拒绝或 always 导致弹窗消失。
- MIT `LICENSE` 已纳入仓库；Cargo 版本为 1.0.1。
- 实时 TUI、捕获式工具结果、发给模型的 Tool Result、JSONL 单事件和单文件均有可配置上限；截断会插入明确标记。
- TUI 输出与历史生命周期已从 session 控制器拆为独立模块，同时保留新的审批菜单和任务级精确命令许可。
- 真机 root/非 root、修改确认、命令超时和全屏交互程序验证矩阵已完成，覆盖提权与确认链、超时回收，以及全屏程序退出后的终端恢复和 TUI 重绘。

## In Progress

- TUR PR #2804 已提交并完成首轮 review 修改；分支已 rebase 到包含重复 `bazel` 配方修复的上游 `master`，新一轮 Actions 等待 TUR maintainer 批准运行，批准后继续跟进四架构 CI 与 review。

## Pending / Known Issues

- 根包若通过 crates.io `cargo package` 发布，需先发布版本匹配的 `nl2sh-tool-macros` 编译期依赖；GitHub/TUR 的完整源码归档构建及单 ELF 运行交付不受影响。
- 真机矩阵已覆盖 root/非 root、超时和全屏交互程序；未覆盖的设备、su 或终端实现仍可能存在兼容差异。
- 源码编译启动脚本仅自动映射 `arm64-v8a` 与 `armeabi-v7a`；其他设备 ABI 会明确拒绝，显式 `RUST_TARGET` 与设备不匹配时也会停止。
- Agent TUI 在 LLM 和捕获式命令执行期间保持同一 ratatui frame；全屏交互命令会临时挂起 TUI，退出后恢复并完整重绘。
- 新主题已完成渲染与样式测试，仍需在不同 adb shell 宿主的 TrueColor/ANSI 256、窄屏和实际电视显示效果下做真机可读性验证。

## Technical Decisions

- reqwest 关闭默认 feature，只使用 rustls、JSON 和 stream feature。
- Provider JSON 与 Agent 通过统一类型隔离。
- su 命令作为独立 argv 传递，避免 nl2sh 自己做不安全 shell quoting。
- 安全规则只允许自定义规则提高风险，内置规则不可被清空。
- 直接 Android shell 使用 `/system/bin/sh`，Termux 使用 `$PREFIX/bin/sh`，非 Android 开发主机条件使用 `/bin/sh`。

## Verification Performed

- `v1.0.2` 发布门禁：`cargo fmt --all -- --check`、默认及 `--no-default-features` 的 `cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --all-targets`、`cargo build --release`、前端测试/生产构建、Release workflow 静态检查和发布相关 Bash 语法检查通过；NDK r28c/API 26 的 AArch64 与 ARMv7 release 交叉编译通过，分别验证为使用 `/system/bin/linker64` 的 64 位 PIE 和 `/system/bin/linker` 的 32 位 PIE。
- Web 工具调用实时显示：`cargo fmt --all -- --check`、默认及 `--no-default-features` 的 `cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings` 和 `cargo test --all-targets` 通过；回归覆盖工具开始即进入消息列表、完成结果按 `call_id` 配对、乱序完成映射及 Runner 开始/完成事件顺序。全量测试为 129 项有效库测试、3 项主程序测试、20 项 Agent loop 测试及其余集成/PTY/TUI 测试通过，1 项显式凭据 ima live smoke 按设计忽略。
- Web 输入区状态与统计布局：主机目标 `cargo fmt --all -- --check`、`cargo check`、`cargo test`，以及前端 `npm test --prefix web`、`npm run build --prefix web`、`git diff --check` 通过。
- 工具公共路径清理：主机目标 `cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo check --all-targets --no-default-features`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 与 `git diff --check` 通过；工具注册、参数 schema、确认、安全分类和 PTY 回归保持通过。
- Web `@` 补全与工具目录：主机目标 `cargo fmt --all -- --check`、`cargo check`、`cargo test`，以及前端 `npm test`、`npm run build`、`git diff --check` 通过；回归覆盖光标所在引用的替换、工具目录 HTTP 响应及文件候选 HTTP 响应。Android 运行时不增加 Node 依赖。
- 工具架构重构：主机目标 `cargo fmt --all -- --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --all-targets` 与 `cargo build --release` 通过；覆盖全部内置工具注册、参数 schema、混合读写工具的动态风险、补丁确认、音频问答、Android/网络/会话现有回归。显式凭据 ima live smoke 按设计忽略。
- Web F2 工具折叠：前端 `npm test`、`npm run build`，以及主机目标 `cargo fmt --all -- --check`、`cargo check`、`cargo test` 通过；回归验证全部展开、全部收起、混合状态及空列表。
- Web 逐工具结果展示：前端 `npm test`、`npm run build`，以及主机目标 `cargo fmt --all -- --check`、`cargo check`、`cargo test` 通过；回归覆盖多工具结果按调用 ID 配对、错误结果、无结果调用及历史会话重建。
- TUI 历史滚动重绘：`cargo fmt --all -- --check`、`cargo check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 通过；配置 NDK API 26 编译器后的 `cargo check --target aarch64-linux-android --no-default-features` 通过。回归验证滚动帧不发送清屏控制序列，消息区域补画宽字符和空白单元格。
- Web 构建顺序调整：`cargo package --allow-dirty` 在不包含 `web/dist/` 的源码包上通过验证；`npm test`、`cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings`、`cargo test`、TUR Bash 语法检查及 AArch64 Android API 26 `RUST_TARGET=aarch64-linux-android NL2SH_PACKAGE_MANAGER_BUILD=1 ./cross-compile.sh` 通过。
- Axum/Preact Web 栈：`npm ci && npm run build`、`cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings` 和全量 `cargo test` 通过；HTTP 集成回归覆盖 rust-embed 首页、CSP、JSON 状态、SSE 首事件与 WebSocket 握手/响应。AArch64 Android API 26 `cargo build --release --target aarch64-linux-android --no-default-features` 通过，产物为使用 `/system/bin/linker64` 的 64 位 PIE，前端资源包含在 5,423,832-byte 单一 ELF 中。

- Web Markdown、工具折叠与快捷控制：`cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings`、全量 `cargo test`、内嵌 JavaScript `node --check` 及 AArch64 Android API 26 `cargo check --target aarch64-linux-android --no-default-features` 通过。新增回归覆盖流式增量同段拼接、工具结果类型转换及 Provider/模型/审批策略保存。

- Web 并发多会话与自动标题：`cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings`、全量 `cargo test` 及 AArch64 Android API 26 `cargo check --target aarch64-linux-android --no-default-features` 通过。新增回归覆盖会话状态隔离、列表并发状态、标题清理与长度限制，以及标题持久化。

- 内置 Web 页面：`cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo test`、AArch64 Android API 26 `cargo check --target aarch64-linux-android --no-default-features` 通过。新增测试覆盖 HTTP 页面/状态返回、配置保存与无效配置拒绝、危险操作强确认及待审批状态保持。

- TUI 保活回归稳定化：目标用例连续运行 5 次通过；`cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings` 和全量 `cargo test --all-targets` 通过，其中 114 项有效库测试、3 项主程序测试、16 项 Agent loop 测试及全部 7 项 TUI 伪终端测试通过，1 项显式凭据 ima live smoke 按设计忽略。
- 运行期权限许可：`cargo fmt --all -- --check`、`cargo check --all-targets` 与 `cargo clippy --all-targets -- -D warnings` 通过；114 项有效库测试、3 项主程序测试、16 项 Agent loop 测试及其余非 TUI 测试通过，AArch64 Android API 26 release 交叉编译通过。新增回归覆盖确认框运行期许可及高风险禁用；全量测试仍只有既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 因启动动画 ANSI 差分文本匹配超时。
- Android 视觉闭环可靠性修复：`cargo fmt --all -- --check`、默认及 `--no-default-features` 的 `cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings` 通过；113 项有效库测试、3 项主程序测试、16 项 Agent loop 测试及其余非 TUI 测试通过，AArch64 Android API 26 release 交叉编译通过。MediaStore 首选投影在真机以 `_size`、`datetaken`、`date_added` 验证不再返回 provider 列异常。全量测试仍只有既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 因启动动画 ANSI 差分文本匹配超时。
- Android 交互闭环与结构化运维：`cargo fmt --all -- --check`、默认及 `--no-default-features` 的 `cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings`、111 项有效库测试及其余非 TUI 测试通过；AArch64 Android API 26 release 交叉编译通过。全量测试仍只有既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 因启动动画 ANSI 差分文本匹配超时。
- 设备诊断与工具可靠性：`cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo check --all-targets --no-default-features`、`cargo clippy --all-targets -- -D warnings`、106 项有效库测试、3 项主程序测试、16 项 Agent loop 测试及其余非 TUI 测试通过；新增 `/new`/斜杠纠错伪终端回归单独通过。API 26 AArch64 release 交叉编译通过，产物为使用 `/system/bin/linker64` 的 PIE。真机只读探测验证 toybox Top 字段、包列表和 UIAutomator 临时控件树命令；全量测试仍只有既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 因启动动画 ANSI 差分文本匹配超时。
- 结构化用户问答窗口：`cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo check --all-targets --no-default-features` 与 `cargo clippy --all-targets -- -D warnings` 通过；95 项有效库测试、3 项主程序测试、15 项 Agent loop 测试及其余非 TUI 测试通过，1 项显式凭据 ima smoke 按设计忽略。新增回归覆盖候选答案、自定义输入、Esc 取消，以及 Raw PCM 三项缺失元数据收集后不经过模型直接重试。全量测试仍只有既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 因启动动画 ANSI 差分文本匹配超时。
- 测试 target 编译修复：`cargo fmt --all -- --check`、`cargo check --all-targets` 与 `cargo check --all-targets --no-default-features` 通过；`cargo test` 的 93 项有效库测试、3 项主程序测试、14 项 Agent loop 测试及其余非 TUI 测试通过，1 项显式凭据 ima smoke 按设计忽略。全量测试仍只有既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 因启动动画 ANSI 差分文本匹配超时。
- TUR PR #2804 build 修复：确认失败的四个架构均在 nl2sh 编译前因 TUR 构建环境报告 `Duplicated package: bazel` 停止；上游随后移除重复配方且其他 PR build 恢复通过。PR 分支 rebase 到修复后的 `master`，保持相对上游仅新增 `tur/nl2sh/build.sh`；新一轮 Actions 已创建，等待 maintainer 批准运行。
- TUI `!` 本地命令：`cargo fmt --all -- --check`、`cargo check`、前缀解析单元测试及未配置 Provider 的伪终端直跑回归通过；回归确认实时输出、退出状态、TUI 保活和审计事件。全量 `cargo test` 的其余测试通过，既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 仍因启动动画 ANSI 差分文本匹配超时。
- TUR 上游提交：PR #2776 仅包含提交 `addpkg(nl2sh): v1.0.1` 和 `tur/nl2sh/build.sh` 一个新增文件；GitHub 判定分支可合并。初始 `Packages-TUR` 与 `Package updates TUR` workflow 为 `action_required`，等待上游 maintainer 批准首次外部贡献运行。
- v1.0.1 签名 APT 发布：补齐仓库签名 Secret 和 `github-pages` 的 `v*` tag deployment policy 后，release workflow 全部 job 通过，GPG 导入、仓库组装签名、Pages artifact 上传及部署成功。线上 `InRelease` 通过仓库公钥验签，指纹为 `5230 D3A7 CCBE ED46 16D3 9C51 FC6A D1BC 63F7 D4D8`；aarch64/arm Packages 中的 1.0.1 元数据和 SHA-256 与实际下载 `.deb` 一致。
- TUR 1.0.1 四架构：上游 `scripts/lint-packages.sh tur/nl2sh/build.sh` 全部通过；`TERMUX_INSTALL_DEPS=true ./build-package.sh -a <arch> nl2sh` 对 `aarch64`、`arm`、`i686`、`x86_64` 均完成 release 编译、打包、ELF 清理与未定义符号检查。四个包均为 Android API 26、NDK r29 产物，架构分别核验为 ELF64 AArch64、ELF32 ARM EABI5、ELF32 i386 与 ELF64 x86-64。
- Termux 四架构运行矩阵：AArch64 API 36 真机通过 `.deb` 升级、版本检查、tmux/TUI 启停和终端恢复；同一支持 `armeabi-v7a` 的真机执行 TUR ARMv7 ELF 返回 `nl2sh 1.0.1`；API 27 x86 与 x86_64 模拟器分别安装对应官方 Termux APK，通过 `apt install` 安装 i686/x86_64 TUR 包，`nl2sh --version`、全屏 TUI 启动、Ctrl+Q 退出及 alternate-screen 恢复均通过。
- 1.0.1 AArch64 Termux 真机：API 36 `aarch64` 设备通过本地 `.deb` 从 1.0.0 升级到 1.0.1，`dpkg-query`、`nl2sh --version`、Android 26 PIE/linker64、`TERMUX_VERSION`/`PREFIX`、`$PREFIX/bin/sh`、tmux 内 TUI 启动、Ctrl+Q 退出和宿主终端恢复均通过；XDG state 目录按 Termux 路径创建。
- TUR 与双运行环境兼容：`cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings`、TUR/部署脚本 Bash 语法和 AArch64 API 26 `--no-default-features` release 交叉编译通过；新增测试覆盖 Termux 标记识别及两套 prompt 约束。全量测试其余项目通过，既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 仍因启动动画 ANSI 差分文本匹配超时。
- Termux 本地发布打包：`bash -n pack-termux-release.sh packaging/termux/build-deb.sh cross-compile.sh`、`cargo fmt --all -- --check`、`cargo check --no-default-features` 与 77 项默认库测试通过；1 项显式凭据 ima smoke 按设计忽略。
- Termux APT 自建仓库：Linux 目标的 `cargo fmt --all -- --check`、默认/`--no-default-features` `cargo check`、77 项默认库测试与包管理 release 构建通过；`nl2sh update` 在包管理构建中正确提示 `pkg upgrade nl2sh`，生成的 `aarch64` `.deb` 包含 `$PREFIX/bin/nl2sh`、配置示例、README 与 LICENSE，包根目录权限为 `0755`。APT `Packages`/`Release` 的架构过滤、相对资源路径与 SHA-256 通过，临时测试密钥生成的 `InRelease` 和 `Release.gpg` 均通过 `gpgv` 验签。ARM64 GNU/Linux 目标的格式、两种 feature 检查及相同 77 项库测试通过。全量测试仍只有既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 因启动动画 ANSI 差分文本匹配超时。
- `/sessions` 最近会话选择：Linux 目标下 `cargo fmt --all -- --check`、`cargo check`、会话存储、设备本地日期格式及序号/方向键选择测试通过；全量 `cargo test` 的 76 项库测试、其余 CLI/Agent/配置/日志/LLM/PTY/root/安全及 4 项 TUI 测试通过，既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 仍因启动动画 ANSI 差分文本匹配超时。
- OpenRouter Provider 与默认模型：Linux 目标下 `cargo fmt --all -- --check`、`cargo check`、9 项配置测试、Provider 识别及设置面板预设回归通过；`cargo test` 的其余测试通过，唯一失败为既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 启动动画 ANSI 差分文本匹配超时。
- 1.0.0 发布门禁：`cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings` 与 `cargo build --release` 通过。`cargo test --all-targets` 共 133 项，132 项通过；唯一失败为已记录的伪终端原始文本匹配用例 `agent_reply_remains_in_live_tui_until_ctrl_q`，单独重跑仍因启动动画的 ratatui 差分 ANSI 输出无法形成连续“审计日志保留”文本而超时。
- ima 只读连接器：`cargo fmt --all -- --check`、`cargo check`、`cargo clippy --all-targets -- -D warnings` 与 74 项默认库测试通过，1 项显式凭据 live smoke 默认忽略；使用 `NL2SH_IMA_CLIENT_ID`/`NL2SH_IMA_API_KEY` 单独运行该 smoke 后，真实知识库发现和库内搜索通过且未输出账户响应；在 `HTTP_PROXY`、`HTTPS_PROXY`、`ALL_PROXY` 均指向不可用地址时仍通过，验证 ima 强制直连。mock 回归覆盖搜索、媒体信息、笔记正文、认证 header 与凭据不进入 Tool Result，来源策略覆盖非 HTTPS/非白名单拒绝。凭据和真实响应未写入仓库；全量测试仍只有既有启动动画 ANSI 差分文本匹配用例超时。
- `@` 文件/目录引用：`cargo fmt --all -- --check`、`cargo check`、`cargo clippy --all-targets -- -D warnings` 与 71 项库测试通过；新增回归覆盖句中光标补全、相对 `@.`、绝对路径、`@~`、目录标记，以及 `@test.txt写的是什么内容` 的最长已存在路径解析。全量 `cargo test` 的其余测试通过，既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 仍因启动动画 ANSI 差分文本匹配超时。
- 结构化文件工具、会话恢复与长内容审批布局：`cargo fmt --all -- --check`、`cargo check` 和 66 项库测试通过；全量 `cargo test` 的 CLI、Agent、取消、配置、日志、LLM mock、PTY、root、安全及 4 项 TUI 测试通过，既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 仍因启动动画 ANSI 差分文本匹配超时。新增回归覆盖 Tool schema、绝对/父目录/符号链接路径、确认前不写入、原子替换、会话保存/恢复/重命名/删除、凭据脱敏，以及审批正文滚动与固定操作区。
- Windows ADB 滚轮诊断确认：默认鼠标捕获模式下设备端只收到退出按键，滚轮事件未到达进程；禁用捕获后 Windows Terminal 稳定发送 Up/Down 事件。兼容实现的 `cargo fmt --all -- --check` 与 Linux 目标 `cargo check` 通过；新增单元测试覆盖 SGR 降级输入。`cargo test` 的 61 项库测试及其他非 TUI 测试通过，全量测试仅既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 因启动动画 ANSI 差分文本匹配超时而失败。
- `/shell` 返回完整重绘：`cargo fmt --all -- --check`、`cargo check` 和 `/shell` 伪终端回归通过；回归在 `exit` 后要求重新出现完整框架的 `Ctrl+Q` 提示，并继续验证安全退出与 shell 内容不写入日志。
- Ollama/Custom Endpoint 草稿保留：`cargo fmt --all -- --check`、`cargo check` 与 8 项设置面板测试通过；新增回归覆盖自定义 Ollama 地址和 Custom 地址在切换其他 Provider 后分别恢复。
- TUI 内置 Provider 恢复：`cargo fmt --all -- --check`、`cargo check`、59 项库测试及其余非 TUI 集成测试通过；新增回归覆盖预设识别、Endpoint 联动、Custom 编辑，以及 API Key、模型和协议不被覆盖。全量 `cargo test` 仅既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 因启动动画 ANSI 差分文本匹配超时。
- `/shell` 直控终端：`cargo fmt --all -- --check`、`cargo check` 与新增伪终端回归通过；回归覆盖普通命令执行、`exit` 返回、TUI 子进程继续存活、安全退出，以及 shell 内容不写入审计日志。全量 `cargo test` 的其余测试通过，既有启动动画原始 ANSI 连续文本匹配用例 `agent_reply_remains_in_live_tui_until_ctrl_q` 仍超时。
- Agent 任务运行预算：stable Rust 1.98.0 下 `cargo fmt --all -- --check` 与 `cargo check` 通过；58 项库测试、3 项主程序测试、13 项 Agent loop、取消、配置、日志、10 项 LLM mock、PTY、root、安全以及 3 项其他 TUI 测试通过，共 108 项。全量 `cargo test` 仅既有 `agent_reply_remains_in_live_tui_until_ctrl_q` 失败，单独重跑仍因启动动画 ratatui 差分 ANSI 输出无法形成连续“审计日志保留”原始文本而超时。
- LLM 自动协议协商：`cargo fmt --all -- --check`、`cargo check`、`cargo clippy --all-targets -- -D warnings` 与 NDK r28/API 26 AArch64 release 构建通过；58 项库测试、CLI、Agent loop、取消、配置、日志、10 项 LLM mock、PTY、root 与安全测试通过。回归覆盖 Responses 成功、结构不匹配回退并缓存 Chat Completions、SSE 回退、部分文本后禁止重放，以及 503 不误判；全量 `cargo test` 的 4 项 TUI 伪终端测试中 3 项通过，既有启动动画原始 ANSI 文本匹配用例 `agent_reply_remains_in_live_tui_until_ctrl_q` 仍超时。
- 设置面板日志与 ASCII Art 开关：`cargo fmt --all -- --check`、`cargo check`、`cargo clippy --all-targets -- -D warnings` 通过；58 项库测试、CLI、Agent、配置、日志、LLM mock、PTY、root 与安全测试通过。全量 `cargo test` 的 4 项 TUI 伪终端测试中 3 项通过，既有启动动画原始 ANSI 文本匹配用例 `agent_reply_remains_in_live_tui_until_ctrl_q` 仍超时。
- 设置入口与焦点修复：`cargo fmt --all -- --check`、`cargo check`、`cargo clippy --all-targets -- -D warnings`、53 项库测试和 `/config` 伪终端回归通过；回归确认 `/config` 只记录为本地命令、不作为用户消息提交，并覆盖设置文本字段边界/光标及命令候选收敛。
- 自更新与统一设置：`cargo fmt --all -- --check`、`cargo check`、`cargo clippy --all-targets -- -D warnings`、53 项库测试及非 TUI 集成测试通过；三个配置伪终端用例已迁移到设置面板并通过，一个既有启动动画原始 ANSI 连续匹配用例仍超时。

- 代理弹窗方向键修复：WSL2 `cargo fmt --all -- --check`、`cargo check` 与 49 项库测试通过，新增独立 Esc 延迟释放及碎片 CSI/SS3 方向键回归覆盖。
- `/proxy` 代理配置：WSL2 `cargo check`、48 项库测试及除 2 个既有动画时序用例外的全部 target 测试通过；Android API 34 ARMv7 release 构建与真机 PTY 验证弹窗打开、类型切换、Esc 取消、保存后热重载及终端恢复正常，配置文件保持 `0600`。启用 SOCKS 后 strip 二进制为 2,530,264 bytes，比此前增加 23,480 bytes（约 0.94%）。
- 余额常驻与动态上下文：WSL2 `cargo fmt --all -- --check`、`cargo check`、45 项库测试及 Agent/config/provider 等测试通过；全量测试仅有 2 个既有 TUI 动画时序用例超时。Android API 34 ARMv7 release 真机确认会话启动后余额自动出现在 80 列顶栏、退出时终端正常恢复，并确认完整余额显示文本未进入 JSONL 日志。
- TUI 内余额弹窗：WSL2 `cargo check` 与 44 项库测试通过；Android ARMv7 真机确认查询期间保留 TUI frame、状态栏显示网络活动、结果弹窗可见并可关闭，余额完整显示文本未进入 JSONL 日志。
- Provider 余额第三阶段：WSL2 `cargo check`、42 项库测试及除两个已知动画匹配用例外的全部 target 测试通过；Android ARMv7 release 构建、推送后在真实 ADB PTY 使用 `/balance` 成功查询 DeepSeek CNY 余额、确认返回 TUI，并以完整显示文本检查 JSONL 日志未记录余额。
- Provider 元数据第二阶段：WSL2 `cargo check`、41 项库测试、10 项 Agent loop 测试和 6 项配置测试通过；Android ARMv7 release 构建、推送后在真实 ADB PTY 使用 `/models` 成功拉取 DeepSeek 模型及 1,000,000 Token 上下文元数据，并完成选择和 TUI 恢复。
- Token 统计与 `/models` 第一阶段：WSL2 `cargo check` 通过；`cargo test` 的 40 项库测试、CLI、Agent loop、取消、配置、日志、LLM mock、PTY、root 与安全测试通过，两个已知启动动画伪终端用例仍因 ANSI 差分文本匹配超时；Android API 设备完成 ARMv7 release 构建、推送及 `--version` 启动验证。
- 本地统一 ZIP 打包：`bash -n pack-release.sh` 与 `pack-release.ps1` PowerShell AST 解析通过；Windows 脚本使用 NDK 双 ABI release 实际构建成功，生成的 `dist/nl2sh-android.zip` 包含与 GitHub workflow 一致的 ABI 子目录、启动脚本、配置示例、说明和截图，`dist/SHA256SUMS` 复算一致。
- 源码编译启动脚本：`bash -n android-build-run.sh` 与 `android-build-run.ps1` PowerShell AST 解析通过；在 ARMv7 ADB 设备上，两者均自动选择唯一设备、识别 `armeabi-v7a` 并把 Rust target 映射为 `armv7-linux-androideabi`，显式指定不匹配的 `aarch64-linux-android` 时在编译和推送前明确拒绝。
- 双 ABI 统一发布包：`bash -n android-run-linux.sh`、`cargo fmt --all -- --check` 与 `cargo check` 通过；模拟组装的 `.tar.gz`/`.zip` 均包含 `bin/arm64-v8a/nl2sh`、`bin/armeabi-v7a/nl2sh`、Linux/BAT 脚本、配置示例、用户说明和截图；Windows BAT 在 ARMv7 ADB 设备自动选择唯一设备、识别 `armeabi-v7a`、推送 32 位程序并正常进入及退出 TUI。`cargo test` 的非 TUI 测试通过，两个已知启动动画伪终端用例仍因 ANSI 差分文本匹配超时而失败。
- ADB 命令菜单方向键：在 API 28 ARMv7 设备向 `/` 菜单分别以超过 500ms 的间隔注入 `Esc`、`[`、`A/B`；第一项向上循环到最后一项、最后一项向下循环回第一项，菜单保持显示且输入始终为 `/`，无 CSI 字母残留。
- LLM 流结束重绘：在 API 28 ARMv7 设备部署 release 二进制，通过 Responses 模型生成不同长度的三行中文；生成期间增量渐变正常，完成时执行一次完整重绘，最终 Markdown 无旧字符残留，Ctrl+Q 后终端正常恢复。
- LLM 流式输出：`cargo fmt --all -- --check`、`cargo check --target aarch64-linux-android` 与 `cargo test --target aarch64-linux-android --no-run` 通过；新增流式文本/工具参数聚合和 TUI 渐变样式测试，其 Android 测试二进制编译通过。
- 0.2.0 发布门禁：`cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings`、`cargo build --release`、`RUSTDOCFLAGS='-D missing_docs' cargo doc --no-deps` 与 `actionlint .github/workflows/release.yml` 通过。
- 0.2.0 Android 交叉编译：NDK r28c/API 26 的 AArch64 与 ARMv7 release 均通过，分别验证为使用 `/system/bin/linker64` 的 64 位 PIE 和使用 `/system/bin/linker` 的 32 位 PIE。
- 0.2.0 测试：34 项库测试和 10 项 Agent loop 测试通过；`cargo test --all-targets` 中两个旧伪终端测试因启动动画的 ANSI 差分输出不再形成连续原始文本而超时，其余测试通过。
- `cargo check`：通过。
- `cargo test --all-targets`：通过，共 68 项测试；覆盖配置/CLI、安全、历史日志及限额、root、双 LLM 协议、重试/timeout、Agent 历史/失败/取消与模型 Tool Result 截断、真实 SIGINT、PTY、初始化顺序、TUI 重配置、编号审批与任务级精确许可、审批面板定位/跨帧清理/方向键拆分、双行布局、TrueColor/ANSI 256 palette、Markdown/表格/工具结果/确认界面的语义配色。
- `android-build-run.ps1`：PowerShell AST 语法解析与 `git diff --check` 通过。
- `cargo fmt --all -- --check`：通过。
- Release 用户说明打包：`release.yml` 已通过 `actionlint`，AArch64/ARMv7 模拟打包确认 `.tar.gz` 和 `.zip` 均包含 `使用说明.md`。
- `cargo clippy --all-targets -- -D warnings`：通过。
- `cargo build --release`：通过。
- `RUSTDOCFLAGS='-D missing_docs' cargo doc --no-deps`：通过。
- `./cross-compile.sh`：通过；使用 NDK r28c 构建 Android 26 AArch64 PIE，解释器为 `/system/bin/linker64`。
- ARMv7 真机：`--version`、Responses Agent 两轮请求、`getprop` PTY 执行/结果回传和 `adb shell -t` TUI Ctrl+Q 恢复均通过。

## Next Steps

1. 根据真机结果继续优化窄屏布局和全屏交互程序切换。
2. 跟进 TUR PR #2804 的四架构 CI 与 Review。
