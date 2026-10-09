# Web 界面

交互启动默认同时监听 `0.0.0.0:9999`；端口占用时选择其他可用端口，以启动日志中的实际地址为准。**页面无需登录，能访问页面的人可查看数据、编辑配置并提交任务。只在受信任网络使用。** `adb forward` 不会关闭局域网监听。

## MCP / A2A 连接

左侧垂直菜单“MCP / A2A”打开连接窗口，可刷新协议进程状态，复制实际 MCP Streamable HTTP、A2A JSON-RPC 与公开 Agent Card 地址，以及 HTTP/stdio 客户端配置、设备启动命令与 A2A 请求示例。查询无需模型，不启动服务，也不展示令牌或提供协议审批按钮。修改仍需设备同 UID、同配置的交互终端批准。

Web 与协议服务分别启动；Web 的 `ready` 不代表 MCP/A2A 已启动。`service status` 文本及 JSON 的 `connections` 字段也提供同一连接信息，`/api/connections` 提供轻量只读查询，`/api/info` 包含 `connections`。状态为 `running/stopped/unknown`；运行中只确认本地协议进程身份与独占锁，不保证客户端网络可达。未启动或无法确认 HTTP 时显示明确标记的默认本机示例。127.0.0.1 仅同设备可用；默认开放 HTTP 并自动获取设备 IPv4；多网卡/VPN/代理可覆盖公告地址，远程推荐 HTTPS。未设 `NL2SH_PROTOCOL_TOKEN` 时启动自动生成并打印令牌，窗口仅显示变量名称。详见 [设备 MCP/A2A](../advanced/a2a-mcp.md)。

## 无终端后台启动

```bash
adb shell '/data/local/tmp/nl2sh --config /data/local/tmp/config.toml service start --json'
adb shell '/data/local/tmp/nl2sh --config /data/local/tmp/config.toml service status --json'
```

`state=ready` 表示已验证进程身份及 `/api/info` 返回的 PID、版本和实际端口。按输出中的 `port` 设置 `adb forward tcp:9999 tcp:<port>`，然后打开 `http://127.0.0.1:9999/`。重复 `start` 返回健康的现有服务；显式重启或停止使用 `service restart --json`、`service stop --json`。`--port 9999` 指定首选端口，`--port-strict` 拒绝占用而非切换；`--port 0` 请求系统分配端口。

服务按配置路径管理相邻的私有 `config.service/` 目录，包含运行锁、操作锁、`state.json`、`service.log`。启动返回前核验健康；记录进程启动标识、可执行文件设备/inode、UID、版本与实际端口。停止仅向核验过的服务发送私有关闭令牌，不按进程名批量终止进程；服务会取消任务、拒绝待决审批并退出。状态 JSON 不包含令牌。更新二进制后，`status` 仍显示实际运行版本；采用 `restart` 才启动新版本。配置和会话文件保持原位。

后台模式不初始化 TUI/PTY，断开 ADB 后继续运行。启动器的 `--web-only`（PowerShell 安装器 `-WebOnly`）调用原生服务接口。启动器只停止本配置拥有的受管服务，不再按进程名结束设备上的其他 nl2sh 进程；`--web-only` 复用已健康的服务，推送新二进制后需显式 `service restart --json` 才会运行新版本。`nl2sh --web-only` 仍可用于由其他进程管理器托管的前台进程；它不自动注册为受管服务。无响应但身份匹配的服务不会被 `start` 自动替换，应查看日志并显式 `restart`。旧启动器或 Helper 的 `nohup` 服务未注册，须先通过其原管理入口停止，不能用新接口接管。

## 界面与会话

左侧图标栏打开会话、文件、应用、工具、安全终端、记忆和配置；相邻区域可最小化、拖动分隔条并记住宽度。窄屏使用覆盖式内容区。侧栏底部可新建会话，左侧 logo 下显示程序版本。

多个 Agent 会话可同时运行，切换不停止后台任务；等待审批各自独立。首轮结束后后台生成短标题，列表显示创建时间。不能删除正在运行、等待审批或终端连接中的会话。Web 与 TUI 各自维护对话状态。

配置保存后每个新 Web 任务读取最新值；当前 TUI 需要重启才能加载浏览器修改。快速开始见 [模型配置](../getting-started/configure-provider.md)。顶栏只读设备概览无需模型；高级功能显示预算、Token、Root 与阶段耗时。思考内容仅在服务返回时显示，不加入模型历史。

记忆面板可搜索、新增、编辑和删除持久便签，也可在二次确认后清空全部。这里的操作是用户直接发起的本地 Web 管理操作；模型通过 `agent_memory` 修改时仍必须经过安全分类与确认。Android 原生部署将 SQLite 数据库保存在 nl2sh 可执行文件同目录的 `memory/agent-memory.sqlite3`；Termux 使用 nl2sh 状态目录下的 `memory/`。旧 `.nl2sh-agent-memory.json` 不会自动迁移。

顶栏的检查更新图标在打开页面时自动查询 GitHub 最新正式版本，点击可再次检查；发现新版本后显示小红点并弹出“立即更新、暂不更新、跳过本次版本”。跳过只记在当前浏览器中，手动打开图标仍可查看该版本。独立 Android 安装可选择立即更新：服务端重新验证所选版本与签名清单，显示实际下载字节、百分比，以及签名下载、签名/摘要/架构校验和安装阶段。安装失败会保留错误并提供重试；重复点击不会启动多个更新。关闭弹窗或刷新页面不会取消更新，重新打开可继续查看进度。安装完成后需要重启 nl2sh 才使用新版本，受管服务使用原管理入口重启或 `nl2sh --config <配置路径> service restart --json`。Helper 安装在助手中升级，Termux APT 使用 `pkg upgrade nl2sh`；两者及无法验证归属的安装不提供网页直接更新。

## 文件与输出

对话回答中的本地图片 Markdown（如 图片路径 `/sdcard/Pictures/screenshot.png`）以及图片、音视频链接（如 音频路径 `/sdcard/Music/record.wav`）可直接预览，复用文件管理组件及同一有界文件接口。支持绝对路径、相对程序工作目录的路径、`~/` 和 `/api/file-preview?path=...` 链接。音视频需手动播放，格式支持取决于浏览器；WAV/PCM 沿用参数播放能力。远程媒体链接不会转为本地文件预览，代码块中的路径不会自动播放。

文件面板只读列出目录、类型、大小和时间。图片、视频、音频、常见文本/代码可有界预览，右侧箭头插入 `@路径`。文本支持高亮和换行；视频支持 HTTP Range。WAV 头部参数可直接用于播放，Raw PCM 必须选择真实采样率、声道和格式；参数解码播放限 32 MiB。其他音频能否播放取决于浏览器解码器。

模型回答中的围栏代码块右上角提供图标按钮，可一键复制完整代码；复制成功后图标短暂变为对勾。工具调用分别显示折叠卡片，F2 展开/收起当前会话所有结果。图表来自工具结果，来源和数值仍需对照原始证据。安全终端通过同一分类和审批执行命令，分段显示 stdout/stderr/退出状态。

## 停止、恢复与导出

停止模型等待会取消请求，命令会终止并回收进程组，其他工具在当前操作结束后停止。进行中检查点经过脱敏；重启后标记中断，不自动恢复审批或继续执行。可识别原始输入的失败任务可“重试原任务”，重新提交输入并重新审批。

导出 ZIP 包含 `conversation.json`、共享 `nl2sh.log` 和范围说明。审计日志可能包含其他会话事件，分享前检查设备信息。已知凭据会脱敏，输出和日志均有上限。

![Web 存储分析](../../assets/web.png)

更多：[安全确认](security-confirmation.md)、[会话](sessions.md)、[网络排查](../troubleshooting/network.md)。

## 服务健康与运行信息

`GET /healthz` 返回 `{"status":"ok"}`，只检查 HTTP 服务是否响应，不依赖会话、模型或伴侣。
`GET /api/info` 返回协议 `1`、程序版本、PID、实际监听端口、Web 服务运行秒数、进程 ABI 和
`capabilities`。能力快照区分 Android 环境、当前 UID 的 shell/UI 权限、已是 root、Bridge 协议
与无障碍/IME 独立状态、已验证协议的 JADX helper、JADX 可在批准后获取，以及 Tailcat 版本。
这些接口只读，不含配置凭据；不要用 `/api/sessions` 代替健康检查，也不要从 root 标记推断动作已获批准。
能力查询不下载资产、不改变系统设置、不请求 su；主程序的内部探测使用普通用户、管道捕获和五秒超时。

`/api/tools` 的 `enabled` 是配置开关，`available` 表示运行时前置条件；未就绪工具仍可在设置目录中
显示。Agent、MCP `nl2sh_tools` 与直接工具调用按当前能力过滤。开发主机不暴露 Android 控制和 ART
反编译；普通 Termux UID 不暴露需要 shell/root 的 UI 工具。APK 静态读取和 Tailcat 检查仍保留。
服务/安装状态变化后，下一任务或信息请求重新发现；当前任务的注册表不在执行中自动改变。

`/api/info` 还返回 `update_ownership`：独立安装可自行更新，Termux APT 使用 `pkg upgrade nl2sh`，Helper 安装使用助手的检查更新动作。主程序对相邻 Helper 归属记录进行有界读取及程序摘要复核；损坏或不匹配时阻止自更新并给出检查提示，避免同时使用两个更新来源。

工具面板从运行时目录派生可选组，将保存的启用开关与已发现的可用性分开展示。当前环境不可用的工具仍可配置，但禁用示例提问按钮；开启不获取能力，也不批准执行。

## Tailcat 快捷共享

点击右上角 Tailcat 图标和文字打开弹窗。默认选择 Web 实际端口与 ADB 连接端口（自动检测，失败默认 5555，可编辑），点击确定后直接安装并共享，不再弹出安全确认；显示下载及执行状态，完成后保留窗口和可复制对端命令。全程无需 LLM，复用现有工具的校验和执行逻辑。详见 [Tailcat](../tools/tailcat.md#tailcat-quick)。

确定或重试会自动停止旧的托管 Tailcat 监听器并重新共享；完成或失败后窗口都保留，仅手动关闭。
