# 安全确认

执行边界始终是 `LLM → Security → Confirmation → Execution`。模型说明不构成批准；Root、工具参数和用户编辑都不能跳过分类。默认 balanced/risk-only 下：

| 风险 | 默认决定 |
| --- | --- |
| ReadOnly | 普通只读查询可自动运行，Root/strict 条件另需确认 |
| Mutating | 展示动作并确认 |
| Dangerous / Critical | 强确认 |
| 无法可靠解析、动态执行 | 保守评估，要求强确认 |

Android 输入注入命令 `input` 和 `cmd input`（包括方向键、确认键、点击和文字）属于 Dangerous，必须强确认；它们可能激活破坏性控件，不能视为只读查询。

<a id="full-access"></a>

## 开发设备 Full access

!!! danger "所有入口自动批准，包括危险 Root 指令"

    在 Web 配置页“执行与安全 → 确认策略”、Web 高级快捷栏的审批菜单，或 TUI `/config` 的“执行与安全 → 确认策略”选择 `full_access`。界面显示“Full access（危险，含 Root）”。保存后所有风险、Root 和强确认操作都自动批准，覆盖 CLI、TUI、Web、安全终端、MCP/A2A 及微信/QQ 渠道。Web 无登录，接入者能借用进程权限执行操作，仅用于完全信任的开发设备和网络。

选择会持久保存为：

```toml
execute_confirm_policy = "full_access"
```

Web 完整配置页需要点击“保存配置”；快捷审批菜单选定即保存。保存后新 Web、协议和渠道任务使用新策略，进行中任务保留自己的快照；TUI 内保存设置会重载配置，外部修改后需重启 TUI。改选 `risk_only` 等普通档并保存，可关闭配置授予的 Full access；重启继续保留已保存的选择。模型可以读取该档位，但不能通过 `nl2sh_config` 自行开启。

也可用全局 `--full-access`（别名 `--dangerously-full-access`）临时强制开启：

```sh
nl2sh --full-access --config /data/local/tmp/config.toml service restart --json
```

该参数同样适用于直接 TUI、`--web-only`、`protocol serve` 和 `protocol stdio`，不写入 TOML。它优先于已保存的确认策略；启动参数强制开启时，仅改选普通档不能关闭，需不带参数重启，并确保配置不再是 `full_access`。对已运行且未开启的服务使用 `--full-access service start` 会要求 restart；普通 `service start` 只复用服务，不改变其启动参数。

风险分类、参数校验、命令/root 计划绑定、目标复核、取消、资源限制和审计继续运行。审计记录 `approval="approved_by_full_access"`；`service status --json`、`/api/info`、`/api/quick-settings` 与 `nl2sh_inspect` 的 `full_access` 报告有效策略，`full_access_cli` 区分启动参数是否强制开启。Web 常驻危险提示，TUI 启动、`/status` 和 `/permission` 可查看。`/permission ask` 不改写已保存的策略。

Full access 不凭空取得 root、不更改 SELinux/Android 系统权限，也不启用关闭的可选工具。提权仍需 `execute_user_mode="auto"` 或 `"root"` 和可用的 root/su；`normal` 继续拒绝显式 su。它优先于 strict 和协议/渠道确认器。下方逐次确认规则描述未开启 Full access 的通常模式。

## TUI / CLI

先看完整命令或 diff、本地风险和规则依据。TUI 支持允许一次、任务内允许完全相同普通命令、运行期允许普通修改、拒绝、编辑、交互/捕获执行。任务许可不匹配前缀；运行期许可只保存在内存。`/permission allow` 开启运行期普通修改许可，`/permission ask` 关闭。Root、高风险和强确认不获得这些许可。非交互 CLI 可由匹配的有效授权凭据批准合格操作；未命中且无法取得必要批准时拒绝，见下方“授权凭据”。

强确认的文字按当前提示输入；不要把自然语言“可以”当作所有后续动作的授权。编辑后整个分类/确认链重新运行。

## 审批弹框快捷切换

Web 的执行审批弹框提供“确认策略快捷设置”，可选择 `always`、`risk_only`、`never`、`full_access`，点击“保存并立即应用于本会话”。TUI 审批弹框按 **P** 打开同样的四档选择，用方向键选择，Enter 保存并立即应用，Esc 返回审批；编辑命令时 P 仍作为普通文字输入。

快捷选择保存到原配置的 `execute_confirm_policy`，同时更新当前会话的实时审批层，**正在等待的这条操作也会立即重新决定是否需要人工确认**，无需停止任务或重新提问。Full access 会自动批准当前操作及本会话后续危险/Critical/Root 操作；普通档位只免去符合条件的只读确认，修改、Root、strict 或强确认要求仍按原规则执行，仍需审批时重新显示完整操作。

已经运行的其他会话不继承此实时覆盖；它们保留当前任务的审批策略。新任务加载保存的配置，当前 Web 会话的快捷选择也作用于后续调用及安全终端。临时会话覆盖不写入历史或检查点，重启/恢复不重放旧批准；保存的配置档位正常保留。既有精确授权和普通运行期许可仍按原范围匹配，CLI `--full-access` 继续优先。

服务器核对会话、当前请求编号和取消状态；旧弹框、补充信息窗口和已结束请求不能修改档位。普通档切换后仍需审批时生成新编号，危险强确认重新开始；保存失败时保留原策略与待决请求。风险分类、参数/目标复核、精确命令与 Root 绑定、取消和执行限额不改变。策略变更记入 `confirmation_policy_changed`；实时普通档释放只读操作使用 `approved_by_session_policy` 审计。

## Web

修改单次点击批准；危险操作先点击进入复核，再第二次点击最终批准，不需要输入 `CONFIRM`。服务端核对待决编号和复核状态，旧窗口不能批准后续请求。拒绝与编辑后重评估仍可用。

## A2A / MCP

工具直调与 Agent 委派默认都等待另一个设备交互终端的一次性审批，120 秒超时拒绝；危险类还需精确短语。使用 `nl2sh protocol approvals` / `nl2sh protocol approve REQUEST_ID`，并保持同 UID 与配置路径。协议客户端没有批准接口。

!!! warning "显式协议自动批准"

    `protocol_auto_approve = true` 会自动批准协议调用的所有风险等级，包括危险/严重动作。它默认关闭，仅协议入口使用；安全分类、参数校验与身份绑定仍运行。只向完全信任的客户端提供服务令牌。

## 授权凭据

`[[approval_grants]]` 在安全评估后按工具、可信包名和风险上限匹配；命中后免人工确认，并原子扣减次数。CLI、TUI、Web（含安全终端）、MCP/A2A 与内置 Agent 共用这一入口。默认列表为空，未命中仍使用原有审批策略。只读操作本来无需确认时不消耗次数。

先设置顶层字段，再把数组表放在文件末尾：

```toml
allow_dangerous_grants = false

[[approval_grants]]
id = "app-launch-20"
tool = "android.launch_app"
package = "com.example.app"
max_risk = "mutating"
expires_at = "2030-01-01T00:00:00Z"
uses = 20
```

- `id` 是唯一的 1–128 位 ASCII 字母、数字、点、下划线或连字符。用量和撤销绑定配置文件身份及 ID；重启、删除后重新添加同一 ID 都不恢复次数。新授权请用新 ID；减少配置次数会收紧剩余预算，增加次数不补充已持久化的预算。
- `tool` 可省略以覆盖所有工具，或为精确名称、单个尾随 `*` 前缀，例如 `android.*`。Shell 包装中的动作只以 `execute_shell_command` 匹配，不推断包名。
- `package` 精确匹配启动/停止的已校验参数或语义点击的已准备节点包名。没有可信包名时不匹配带包名凭据；省略包名也只覆盖无包名作用域，不能覆盖已知包名。坐标点击、滑动、滚动、按键和文本输入不推断前台包名。
- `max_risk` 为 `read_only`、`mutating`、`dangerous` 或 `critical`，只比较最终评估结果，不能降低风险。Critical、Root、要求二次确认的动作始终排除。
- `expires_at` 是可选 RFC 3339 墙钟截止时间，达到即失效；修改设备时间会改变授权窗口。`uses` 是可选正整数，省略表示不限次数；失败、取消或目标复核失败也不返还已消耗次数。

!!! warning "危险授权默认关闭"

    Dangerous 匹配同时需要 `allow_dangerous_grants = true` 和启动参数 `--allow-dangerous-grants`，启动会输出警告；后台服务启动/重启会把显式参数传给子进程。Root、Critical 和二次确认排除规则仍然生效。当前所有 Dangerous 工具要求二次确认，因此此开关不会自动批准 UI 点击、输入等工具。只对不要求二次确认的 Dangerous 评估开放；不要将它当作无人值守 UI 授权。

模板在每次需要确认时重新读取，过期、耗尽及撤销无需重启。用量存放在配置对应状态目录的私有授权目录，目录 0700、文件 0600，使用跨进程文件锁和原子保存；损坏或不安全的状态不能自动放行。TUI 的“执行与安全”分类在打开时显示用量快照；Web“高级规则”可刷新只读列表并立即撤销 ID。撤销不停止已经批准或执行中的动作。Web 管理接口沿用现有所有者界面的信任边界，协议客户端没有新增远程批准接口。

审计使用 `approval = "approved_by_grant"` 和 `grant_id` 区分凭据放行、人工批准与临时记忆许可。凭据批准不会生成任务内或运行期的后续许可。

消息渠道管理见[微信与 QQ 消息渠道](channels.md)。渠道任务可以在原会话完成一次性审批，独立于 MCP/A2A 本地审批与协议自动审批开关。

渠道附件是不可信数据；发送文件必须通过原会话审批，预览包含目的地、文件大小和 SHA-256，实际发送审批前取得的不可变快照。图片字节、媒体密钥和签名 URL 不写入会话历史，缓存路径可能到期。参见[渠道管理](channels.md)。

渠道 `/approve session` 授权原发送者/渠道会话全部普通操作，危险、Root、Critical 与强确认仍逐次询问；`/new` 或重启失效。`/approve` 一次、`/deny` 拒绝；`/approve always` 对普通媒体发送匹配原发送者/会话、目标及媒体类型，工具/风险/规则仍校验；每次发送保留独立文件快照，其他操作按完整预览精确匹配，危险首次强确认。旧记录不自动扩大范围。多个 pending 要求编号。详见[渠道审批](channels.md)。

QQ 审批按钮复用原会话待决请求校验，危险二次确认仍需完整短语；审批语言读取 ui_language，默认中文。通用渠道发送工具从 MCP/A2A 调用时使用协议独立审批，不继承目标渠道授权。参见[渠道管理](channels.md)。

Android 与其他 Unix 配置工具写入使用有界等待的 `flock` 跨进程锁，持锁后重新校验审批前快照，再原子保存；锁忙或快照变化时拒绝写入。渠道授权命令不会修改全局审批策略；`execute_confirm_policy="always"` 只表示每次询问。
