# 更新记录

## [Unreleased]

- Web 模型回答的围栏代码块新增无文字的复制图标按钮；一键复制完整代码，成功后图标短暂变为对勾，并为辅助技术保留状态名称。Clipboard API 不可用时使用浏览器兼容回退。

- Agent 记忆重构为内嵌 SQLite 事务存储；Android 原生部署保存在可执行文件旁的 `memory/`，Termux 使用状态目录，不依赖系统 `sqlite3`，也不迁移旧 JSON。Web 左侧新增记忆面板，可搜索并增删改查。

- 改进 Agent 记忆检索提示：回答用户姓名、个人身份、偏好、长期约定或已保存事实前先查记忆，并区分持久记忆与 Android 系统用户、设备 Owner 及应用账户；无答案且语义含糊时先澄清，无关设备任务不自动加载记忆。

- `tailcat_send_file` 省略 `mode` 时默认使用不依赖 scp 的原始流；`copy` 保留为面向接收箱的显式兼容模式，并明确 Android 原生 shell 通常不提供其外部 scp 依赖。

- 新增强确认的 `tailcat_adb_pair`：引导 Android 11+ 无线调试，复核当前配对信息后共享配对/连接及可选 Web 端口，返回配对码和对端命令；拒绝过期审批和已有监听器替换。

- 补齐 Tailcat 全部八个工具的参数示例、远端 `forward` 访问 9999 与端口映射，以及双向原始流和接收箱传输步骤。

- 补充 Tailcat 在 Android 8/9 的 DNS 协议不兼容说明、典型诊断与恢复方法；记录同一 v0.7.0 二进制在 Android 8.1/API 27 失败、Android 15/API 35 成功转发 HTTP 请求的竖屏对照，并明确验证范围。

- 新增模型可调用的 `nl2sh_config`，支持 list/get/set/reset、点分工具开关、磁盘与当前任务快照查询；凭据脱敏且由用户管理，修改经确认、敏感策略强确认，保留其他字段与注释并拒绝过期审批覆盖。配置只落盘，新任务或重启后加载。

- 修复 TUI 等待模型时 Ctrl+C/Ctrl+Q 仅显示取消却无法结束任务的问题，接入任务级取消信号并保留执行清理与终端恢复。

- 修复 Tailcat 自动安装漏掉 Android x86_64，使用固定官方 amd64 静态包并保持完整校验；明确端口共享转发到已有服务，不重复绑定目标端口，缺少程序时提示确认安装后重试。

- TUI `/config` 新增工具组与单项启用开关，保存后自动重载；组切换清除单项覆盖，长列表跟随选中项显示。

- 快速开始新增 nl2sh-helper 作为第二条安装路径，说明 Android 控制端、目标设备连接、Web 配置和架构限制。

- 修复 Linux/Windows 构建与运行启动器无法识别含空格的无线 ADB mDNS 序列号、错误要求输入设备 IP 的问题。

- 补齐运行与一键安装脚本的三 ABI 文案，复用缺少 x86_64 的旧包时提示备份配置并安装到新目录。

- 在 Android 8.1/API 27 x86_64 模拟器验证原生程序、TUI/PTY、修改与危险确认、超时、终端恢复、窗口缩放及 Web-only 页面/接口。

- 新增 Android API 26+ x86_64 原生构建、部署、自更新、统一发布包及自建 Termux deb/APT；启动器优先选择原生 x86_64，签名 APT 合并兼容旧双架构快照。

- 完善 A2A/MCP 协议格式、任务/结果语义、限制、审批与排障；更新客户端接入并说明独立 Android 项目边界。

### 新增

- 建立默认中文、完整英文的 MkDocs Material 文档站；统一用户手册与逐项翻译的历史更新记录，加入双语/链接/代码派生参考 CI，并在验证签名和摘要后合并 Termux APT 到唯一 Pages 产物。

- Web 快速开始可用未保存地址和密钥查询模型，并选择或手工输入。
- 新增可选 tailcat_install，确认后按 ABI 下载固定官方版本、验证摘要和程序并原子安装。
- 新增默认关闭的 APK/JADX、Tailcat 组与单项持久开关，关闭项不进入模型和直接调用目录。
- 新增确认后的 Tailcat 文件传输、端口共享与主机/设备测试脚本。

### 变更

- Android Bridge 与 JADX helper 源码拆分到 `nl2sh/android-bridge`、`nl2sh/jadx-helper` 独立仓库，各自维护 Android 构建、CI、标签发布和双语文档；主仓库不再构建或重发 helper，运行时仍固定历史 `v1.0.4` URL 与摘要。

- Web 模型发现记录无凭据诊断：请求编号、域名、耗时和上游状态。
- Web 高风险从手输 CONFIRM 改为两次点击；服务端按待决请求检查，按钮用警告色。
- 按网络与文件副作用分类 Tailcat shell 命令，上传与端口共享强确认。

### 修复

- 已选或已协商模型协议的偶发 HTTP405 有限重试，包含工具回传后的流；初次自动探测仍立即回退。
- Web 模型选择器显示全部结果，不受预填名称过滤。

## [1.0.6] - 2026-10-01

### 新增

- Web logo 下显示程序版本。
- 新增有界目录浏览、类型/大小/时间、图片/视频/文本/代码/音频预览及 WAV/PCM 参数播放。

### 变更

- Web 会话、文件、应用、工具、终端和配置移入固定左侧栏，面板可调整宽度。

## [1.0.5] - 2026-10-01

- 发布 v1.0.4 后新增的 Android Web-only 启动支持，Release 程序接受 --web-only。

### 新增

- 新增 --web-only，无 TUI 后台运行 Web，重定向后可在 ADB 断开后继续服务。
- 源码、打包和安装启动器贯通 Web-only，nohup 启动、不申请 PTY、输出 nl2sh-web.log。
- companion 新增 nl2sh Keyboard：可编辑字段用 ACTION_SET_TEXT，can_write 字段用剪贴板，其他字段用 InputConnection，后端在确认前固定。IME 绑定包名/field ID、拒绝密码、无法回读报失败，仅 IME 支持 replace。用户手动选择键盘，无障碍与输入法独立。DUMP 限制的运行时 IME_TEXT/IME_TEXT_B64/IME_CLEAR 广播等同直接 adb shell，不经过 nl2sh 确认。
- 新增默认关闭 bridge_auto_approve，显式自动批准桥接 ask/invoke 的全部风险等级。
- 网关现有端口新增 Bearer 鉴权的 Streamable HTTP MCP /mcp，复用 stdio 工具与设备确认。
- A2A 支持 Compose、无线 ADB 设备 IP、跨主机 Hermes；远程明文 HTTP 需显式选择。
- 新增 Accessibility companion，提供 Unicode、实时节点、语义点击与有界手势，Binder 仅 shell/root，保留确认。
- A2A/MCP 新增具名 Tool Runtime 直接调用与 android.* 语义工具，默认等待一次性设备终端确认；网关不能批准，截图可返回有界图像块。

### 变更

- Web 左侧活动栏整合可最小化、独立宽度面板，路径箭头填入输入；文件/应用只读，终端安全链不变。
- Compose 可用 NL2SH_GATEWAY_BASE_IMAGE 或构建参数选择镜像，默认 python:3.12-slim-bookworm，不绑定厂商。
- android.* 工具有独立必填 Schema，准备前拒绝无关参数。
- Agent Card/MCP 明确为 Device Runtime，内置 Agent 咨询可选；直接调用不需设备模型。
- brush-parser AST 成为主要 shell 分类器，拆分领域策略，仅保留 fork-bomb regex；能力 broker 复核精确批准与 root 计划，动态/未知代码强确认。

### 修复

- 字段隐藏 isEditable 时通过身份匹配节点或可编辑祖先粘贴 Unicode，focused_target 只读定位；包名/身份与 shell/root 边界保留。
- companion extra 值中的百分号/冒号编码后解码，避免 content call 拒参却退出0造成 invalid reply；明确报告参数拒绝。
- android.launch_app 解析并验证 MAIN/LAUNCHER 组件后显式启动，修复厂商 am start 包过滤失败。
- 语义点击和 Unicode 输入在确认前后及 companion 最终执行绑定包名，缺失/改变拒绝。
- 限定包名 MAIN/LAUNCHER 启动代替 monkey 随机事件；退出0但打印错误仍判失败。
- 清理 bridge 异常退出留下的一次性请求，跨进程串行化待决上限检查与发布。
- 密集树达到 Binder 预算时返回部分树，机器协议通过静默管道捕获，不受普通 PTY 影响。
- 节点完整文字/描述摘要绑定语义点击，长文字可精确匹配，显示前缀相同但全文改变会拒绝。
- 有界捕获 adb 与 MCP HTTP 流，超限立即终止并回收设备子进程。
- 解码 uiautomator XML 字符引用；wait_text 读取失败立即报告，不当成节点缺失。
- UI 树读取失败明确返回失败工具调用，不伪装空白屏成功。
- 批准后和 companion 内再次核对节点身份，拒绝同 bounds 替换控件；截断树不能证明唯一目标。
- Unicode 写入绑定确认时焦点可编辑控件，改变后拒绝。
- 滚动坐标由显示计算；shell Unicode 明确拒绝，保留字面 %s 不误转为空格。
- 每个 Web 审批/问答使用独立展示 ID，连续提示不继承旧禁用状态。
- 将脚本执行选项限定到已知解释器，带 >>> 的 grep 等诊断保持只读，真实写入仍确认。
- Web 实时更新轮次/步骤/工具计数，失败、取消和重启恢复统计。
- 服务返回的 reasoning 单独展示，保留工具中间说明，输出思考后禁止协议重放。
- Web 历史、快照、导出保留任务总/模型/工具/人工等待耗时。
- Web 事件写共享审计，带会话 ID，脱敏并共享文件限额。
- Android 日志优先有界定向查询，超时后缩小范围。

## [1.0.4] - 2026-09-28

### 新增

- 新增 APK/条目/DEX 类索引，单类 JADX 强确认，Android app_process DEX helper，本地构建与固定摘要下载。
- 新增 Windows CMD 一键入口，与 Linux 长参数一致，复用 PowerShell 核心并保留交互终端。
- Linux/PowerShell/CMD 显式选择 Gitee，同源 ZIP 校验，失败不隐式换来源。

### 变更

- helper 固定 Android 兼容 JADX1.5.1、单类工作、私有 ART 临时目录，兼容 API28 ARMv7，拒绝意外 XML。
- 补齐安装后重复启动、设备选择、配置保留/显式部署、安全退出教程。

### 修复

- curl 管道安装读取结束后重连 controlling terminal，保留 ADB PTY 交互。
- CMD 进入括号前确定 PowerShell URL，避免早期展开为空。
- 参数 shift 前保留 BAT 目录，临时路径在括号外赋值，避免 endpoint 污染路径。
- Windows 完整安装目录幂等复用，合并显式 Provider 字段，保留其他设置/无参数配置，拒绝不完整目录。
- Linux 同样幂等复用、保留配置字段与终端重连。

## [1.0.3] - 2026-09-27

### 变更

- Linux/Windows 启动器比较主机和设备 SHA-256，相同跳过推送，推送后复核。
- agent_memory 用 Schema 枚举动作并先拒绝未知值；失败 Web 任务可重提原输入，不重放结果/审批/不可恢复脱敏文本。
- 损坏 Tool JSON 不执行，反馈诊断并最多两次重新生成；拒绝占预算，修复后走完整安全链。
- Web/TUI 围栏代码用语义色高亮，未知语言保持纯文本，Web 转义 HTML。
- TUI 首轮生成短标题；Web 创建时间首日相对显示，其后日期时刻。
- Web 重连清理过期运行状态，保存脱敏进行中检查点；回答先保存，标题后台生成。
- Web 保存设置后测试真实推理；中文分类工具目录展示用途/确认，完成轮汇总完整/部分/失败。
- Web 存储只读示例要求总/已用/剩余空间图表。
- 工具结果回填后清理 Web 临时 OUT/ERR，仅保留折叠卡片。
- Web 安全终端分段显示 stdout/stderr/退出码，窄屏折行。
- 优化桌面/平板/手机布局，平板顶栏换行，手机横向会话，短屏保留引导/审批操作。
- Web 引导首选 DeepSeek/deepseek-flash，含所有 Provider/Custom；OpenAI 官方地址，Custom 可编辑。
- 默认 Web 聚焦对话/进度，高级区放工具/终端/参数/统计；审批展示风险/完整动作，编辑重分类。
- 要求 Agent 报告完成内容、证据、失败/未验证步骤和有证据的设备改变。
- 引号输出和只读 mount 不误判，真实重定向/mount 改变仍确认。
- 连接性返回摘要，区分 ICMP/HTTPS，设备 ABI 与进程架构分别报告。
- TUI 欢迎末尾突出 Web URL，窄屏折行。

### 新增

- 新增互链英文 README 与当前 TUI/Web 截图。
- Linux/Windows 一键安装校验 ZIP、生成可选 Provider 配置再启动，配置0600。
- 新增 stdio MCP，发现/鉴权 A2A，提供盘点、目录、咨询与任务查询。
- 新增主机 A2A1.0、Agent Card、鉴权 Task、持久上下文与受限 bridge；构建部署显式检查点，默认拒绝无人审批修改。
- Web 新增停止任务、无模型只读概览、工具示例与弹窗焦点管理。
- 新增有界只读图表，Web 柱/线/饼和数值表，可恢复；TUI 文字数值。
- Web 新增配置/模型列表引导、只读例子、首发新建会话、可操作错误与入门文档；全部 IPv4 无登录。
- Web 侧栏 logo 与顶栏 Star。
- 新增有界只读环境工具，查询系统/ABI/命令/内存/存储。
- Web 单会话/全部删除，全部删除确认，活动会话保护。
- Web 分组字段与原始 TOML 编辑、校验，新任务使用保存值。

## [1.0.2] - 2026-09-24

### 变更

- Web 工具开始立即展示，按调用 ID 回填有界结果，最终转录唯一持久来源，无重复卡片。
- 移除旧 Rust 顶层工具重导出，使用 nl2sh::tools::<domain>。
- 工具按域与显式注册表重组，派生 Schema/本地风险/准备确认执行，原行为保留；宏仅编译期。
- Web F2 全部展开/折叠，保留单卡控制。
- Web 结果转义换行按真实换行显示，持久结果不变。
- 每个 Web 调用与对应结果独立卡片，含恢复会话。
- Web 深色导航/Markdown/焦点/状态采用 TUI 语义色。
- 滚动只重绘 TUI 对话区域并清理短行尾，减少全屏闪烁。
- Cargo 按 npm lock 生成嵌入资源，停止跟踪 web/dist，Android 仍单文件。
- Axum0.8/rust-embed/SSE/安全 WebSocket 与 Preact/TS/Vite 重构 Web，保留单文件。
- TUI Agent 回归用正确 SSE fixture、禁用装饰，避免原始 ANSI 连续文本断言。
- 新增运行期普通修改许可与 /permission，Root/强确认/高危仍单独确认。
- 有界视觉流程：紧凑节点、输入后状态、图片缩放、Provider 错误与 MediaStore 投影/排序修正。
- 源码/发布/包/支持/更新迁至 nl2sh 组织，更新 logo/元数据。

### 新增

- Web @ 路径补全与共享解析，可搜索工具目录。
- Web 导出当前对话与共享日志 ZIP。
- Web Markdown、折叠工具、流式拼接、快速 Provider/模型/审批、会话统计。
- Web 并发独立会话、持久侧栏、独立审批、状态与模型标题。
- 内嵌浏览器配置/对话/输出/审批/问答/会话恢复。
- 新增确认后 bounds 输入、图片附件、JSON POST、通知/Crash/ANR/功耗/网络/存储/Doze/权限/剪贴板/媒体/便签；写入仍确认，图片不持久化。
- TUI 新增 !command，不请求模型，保留安全/确认/Root/PTY/超时/恢复，输出不进模型上下文。
- 新增确定性 WAV/PCM analyze_audio 与 judge_audio_quality，缺元数据 needs_input，Jev 配置后优先，否则通用模型。
- 新增多字段问答，Raw PCM 收集真实元数据后本地重试，不由模型猜测。

## [1.0.1] - 2026-08-31

### 新增

- 新增 TUR 配方，固定源码/摘要与 Termux Rust 工具链。
- 集中识别 Android shell/Termux，用于提示、摘要、路径和 shell。
- 新增按 ABI 的 Termux 构建、ADB 与 SSH/tmux 部署启动脚本。
- 新增 aarch64/arm 签名 APT，包管理更新、XDG config/state。
- 新增 Linux 双架构 deb 打包与独立 Termux 指南。
- 新增 Windows NDK 构建、WSL 仅 dpkg-deb 封包。
- 新增 OpenRouter，默认 openrouter/free 与 https://openrouter.ai/api/v1。

### 变更

- 明确 Android shell 一等、Termux 兼容，动态 PREFIX/XDG/包管理，安全不变。

## [1.0.0] - 2026-08-25

### 变更

- 启动小火车按烟/屋顶/车身/品牌/轮子使用主题，保持裁剪与 ANSI256。
- @ 候选用 Enter/Tab 插入，Right 保持光标移动。
- 新增只读 ima、无代理直连、库/搜索/原文、来源限制和凭据脱敏，无写操作。
- TUI @ 支持相对/绝对/波浪路径与紧接中文问题，内容仍有界工具读取。
- 确认面板按内容调整大小，长命令/diff 滚动，操作固定。
- 新增无工作区沙箱的有界文件读/列/搜索/补丁，diff 确认后原子写入。
- 私有会话自动保存与列表/恢复/重命名/删除，不保存凭据/余额/许可。
- Windows ADB 使用 alternate-scroll，不捕获远端鼠标，滚轮映射方向键。
- 完成 Android Root/非Root、修改、超时、全屏与恢复验证矩阵。
- /shell 返回清空 ratatui 缓存，备用屏完整重绘。
- 设置面板分别保留 Ollama/Custom 地址草稿。
- 统一设置恢复内置 Provider，联动地址且保留 Key/模型/协议。
- 新增 /shell 直控系统 shell，exit/Ctrl+D 恢复 TUI，不进模型或审计。
- 独立步骤/工具/活跃时间/停滞/重复/硬上限预算与 Fast/Normal/Deep，确认等待不计时，安全不变。
- 相同命令结果三次后阻止第四次，停滞重规划/终止，80%/90%提醒；摘要记录预算和终止原因。
- 设置文本 UTF-8 光标移动/插入/删除，密码掩码。
- 模型与智能体恢复后台模型发现与元数据回填，失败保留手工值。
- 过滤丢失 CSI 的 SGR 鼠标报告，避免进入输入字段。
- 所有 / 输入限定本地，未知提示，不发模型；修复 /update 落入 Agent。
- /config 接管焦点，不发模型，独立编辑边界与光标。
- 移除 /provider /model /models /proxy，统一 /config /setting。
- 新增 update /update 与后台检查，按 ABI/SHA-256 原子更新，可延后/跳过。
- 多 Tab 整合 Provider/模型/Agent/安全/界面/代理，历史推荐24/16。

### 新增

- 默认 auto 优先 Responses、仅安全协议不匹配回退 Chat 并缓存，不误判鉴权/限流/超时/部分流。
- 统一设置支持清日志与独立佛像/小火车默认开启装饰。

- 任务累计各工具步骤的模型输入/输出 Token 并显示。
- 新增历史 /models 查询与加载状态，失败手填，不记录凭据/账户正文。
- 元数据独立客户端适配 OpenAI/DeepSeek/SiliconFlow/Ollama，已知或覆盖窗口估计占用。
- /balance 公开接口查询 DeepSeek/SiliconFlow，不支持则明确失败，不用私有控制台。
- 余额每60秒刷新常驻，失败保留成功值，不进模型/配置/日志。
- 上下文达到水位后淘汰最旧完整轮次，保留 system/当前轮/工具 round。
- 新增历史 /proxy 支持 HTTP/SOCKS/DNS/认证/绕过及保留配置的开关，统一 Provider 策略。
- 重组碎片 CSI/SS3 左右键，避免误判 Esc 关闭代理弹窗。
- Agent/Command 以 Android sh/toybox 为基线，可选桌面工具先探测。
- 输入路径过滤碎片 F2 ESC O Q，避免 OQ 残留。
- Chat/Responses SSE 文本渐变流式显示，完成恢复 Markdown，参数完整后审批。
- 统一 Release 包含双 ABI，Linux/BAT 自动选设备与 ABI。
- 源码启动器更名 android-build-run.sh/ps1，按设备自动构建匹配目标。
- 本地 pack-release.sh/ps1 构建双 ABI，统一 ZIP 与 SHA256SUMS。
- README/用户说明/发布包使用动态 TUI 演示。

## [0.2.0] - 2026-08-22

### 新增

- 每任务低敏感 Android 摘要含 API/ABI/shell/UID/root 能力，失败省略，不影响安全。
- 新增 /exit 本地安全退出，不进模型。
- 欢迎页非阻塞一次性 ASCII 小火车，裁剪并不进入历史/模型。
- 小火车退出前贴边，修复两列移动跨过边缘。
- README 增加支持/贡献与微信赞赏链接。
- 欢迎页/help 增加支持链接与纯文字祝福，无图片/二维码渲染。
- 新增 /help /clear，清对话/模型/输入历史，保留审计。
- 实时输出/工具/模型/日志分别有界，显式截断。
- 新增 MIT LICENSE。
- 全项目 TUI 视觉规范，深色/语义/组件/ANSI256/安全边界/验收。
- 滚轮历史、Shift拖选与宿主右键复制。
- 闪烁焦点光标、Unicode 编辑、方向键输入历史。
- 过滤式垂直 slash 菜单，键盘选择/补全。
- 初始 Rust 模块工程与 Android 交叉编译脚本。
- 配置加载/验证/私有向导/环境 Key 覆盖。
- CLI Provider 参数覆盖后统一验证。
- 持久 ratatui/crossterm TUI、ASCII、滚动与恢复 guard。
- 统一 LLM trait 的 Chat/Responses 适配。
- Agent 顺序调用/结果、多轮完整历史限额、编辑与真实反馈。
- 失败关闭安全规则、非 TTY 拒绝、确认和强双确认。
- openpty、交互桥接/resize、ANSI过滤、管道、取消/超时升级/root 策略。
- 增量输出 sink，控制台流与 TUI 历史回放。
- 配置/安全/root/HTTP mock/Agent/PTY 测试。
- 独立进程 SIGINT 证明取消和 PTY 子进程回收。
- 单 frame Agent TUI 内嵌确认/模式/取消和伪终端生命周期回归。
- cc-rs 依赖的 NDK 构建支持，r28c/API26 AArch64 release 验证。
- 可选 ARMv7 构建与 API34 真机网络/PTY/反馈/恢复 smoke。
- TUI BaseURL 优先配置、0600 保存与 Provider 热重载。
- 输入与状态/上下文使用独立行。
- 用户/工具/Agent/命令/成功/错误语义配色。
- 0600 追加 JSONL 记录输入/命令/输出/结果/错误。
- 中文默认与英文 TUI，向导/确认本地化。
- 欢迎预置 Android 任务、设置、滚动/取消/退出说明，不进模型。
- 完成工具默认折叠 F2 展开，保留实时/诊断/模型反馈。
- 终端 Markdown、行内样式/代码/Unicode表格/换行/窄屏降级。
- 一键构建/部署/运行，目录/目标/序列号可配置。
- Windows 原生 PowerShell/NDK LLVM，无 Bash/WSL。
- Linux/PowerShell 预编译部署脚本，不编译。
- 方向键 Provider 预设/自定义地址与 Key 配置，历史入口包括 /provider/--init。
- GitHub tag 工作流 NDK r28c 构建双 Android ABI、打包启动器/配置、ZIP/TAR/摘要和 Release。
- 新增中文 ADB/Linux/Windows/ABI/配置/排查指南，纳入发布包。
- 内存查询截图嵌入文档/README，随包携带。

### 变更

- 恢复 logo 并居中，终端仍不渲染图片。
- 佛像射线用装饰金色，正文普通色，拷贝无 ANSI 且不复用风险颜色。
- 缺配置直接进入 TUI，不自动向导；配置完成前模型任务拒绝。
- 默认步骤8→24、上下文10→16，用于更长诊断。
- TUI 输出/历史生命周期拆出主控制器。
- 0.1.0 作为发布基线，继续开发。
- 确认改编号/方向键与别名，任务内精确普通命令许可，Root/高危排除。
- 集中 GitHub Dark 风格语义 palette，TrueColor/ANSI256覆盖全部组件。
- 明确 Android shell 类 Hermes 单文件丰富 TUI，非 API/插件兼容承诺。
- 输入行改低对比编辑条，状态/分隔保持终端背景。
- 最终回答优先用户语言、对比表格和可读总结。
- 补充 ABI/interpreter 错误导致文件存在却 No such file，给 ARMv7 验证方法。

### 修复

- slash 菜单重组碎片方向键，首尾循环不消失或残留 A/B。
- 流结束/取消使缓存失效、全屏重绘，清除旧渐变字符。
- 启动器同步主机行列到 Android PTY，修复宽屏动画裁剪。
- 小火车按实际对话视口移动、贴右边后退出。
- 佛像中文祝福宽度统一65列，修复右边突出。
- ADB 正常/异常退出关闭宿主鼠标，panic 同样完整恢复。
- 审批锚定输入上方左下，碎片方向键不误拒绝或任务许可。
- 审批阶段清理稳定完整面板，避免残留，统一背景。
- 只读包版本命令替换不误确认，内部修改仍分类。
- 历史支持滚轮/PageUp/PageDown，不每帧强制底部。
- 部署先 adb root/等待/验证UID0，再 su 回退，私有配置失败不放宽。
- 输入过滤碎片 SGR，不写字符或清空输入。
- /dev/null 与 fd 复制不误判修改/Root，真实写入仍保护。
- 多行 Markdown 按真实行渲染，保留标题/列表/空行/表格。
- 展开结果按实际换行高度定位，不受逻辑行数限制。
- 交互 PTY 返回恢复鼠标并完整重绘 TUI。

## [0.1.0] - 2026-08-04

### 新增

- 初始开发基线。
