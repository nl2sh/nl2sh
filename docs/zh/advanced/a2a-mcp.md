# 设备端 MCP / A2A

升级时自动忽略已移除的顶层 `bridge_auto_approve`，旧值不会启用 `protocol_auto_approve`，后者仍默认关闭。加载不改写原文件；通过配置编辑器保存时移除废弃字段。配置向导和 Web 校验/保存采用同一规则，其他未知字段与 TOML 语法错误仍拒绝。

MCP 与 A2A 已内置在 nl2sh 的 Rust 可执行文件中。外部 Agent 直接连接设备，不需要 Python、Docker、主机网关或运行时 ADB。MCP 直接调用 Tool Runtime，A2A 向内置 Agent 委派任务。直接工具调用不需要模型；Agent 委派需要配置设备模型。协议和限制见 [协议参考](../reference/a2a-mcp.md)。

## 查看连接方式

TUI 启动页、`nl2sh --config <配置路径> service status`（含 `--json` 的 `connections` 字段）、Web 左侧垂直菜单“MCP / A2A”均提供连接方式。HTTP 运行时显示实际公告 origin，即使端口或 HTTPS 地址不同于默认值；stdio 运行只显示本地进程，不宣称 HTTP 可用。未启动或未知时，默认 loopback 地址仅作为明确标记的示例。Web 窗口可以刷新并复制客户端配置、启动命令与 A2A 请求体。

查询使用私有 `protocol/connection.json`、进程启动身份和独占锁，只有 TUI/Web 所有者界面读取已验证运行进程的令牌，不向公告地址发出请求。停止后的遗留记录不会报告运行中。它只确认同 UID、同配置进程，不保证远端网络可达；TUI 为启动时快照，Web 与 CLI 查询刷新当前状态。状态查询不启动服务；`protocol_start_with_service=true` 时 TUI/Web 启动路径也一并启动协议。

## 随后台服务启动

在配置中设置：

```toml
protocol_start_with_service = true
protocol_service_port = 8765
```

默认 `false`，也可在 Web 配置页“服务”分组切换。执行 `nl2sh service start` 或 `service restart` 时，后台进程一并启动 Web 和 MCP/A2A；`service stop` 一并取消任务并关闭两个监听器。开关在启动时读取，修改后需 `service restart`，重复 `start` 保持当前进程。同一开关也作用于直接 TUI 和 `--web-only` 启动；UI 退出时关闭自己启动的协议。已有同配置协议时 UI 复用连接信息，不停止独立进程。单独 `protocol serve/stdio` 不启动 UI。

协议监听 `0.0.0.0`，使用自动设备 IP 和独立 Bearer 令牌。`protocol_service_port` 默认 `8765`；可改用其他端口避免占用，或设为 `0` 自动分配端口。自动令牌和完整连接信息写入配置相邻私有 `config.service/service.log`；日志含凭据，仅给可信调用者读取。TUI/Web 显示运行版本、实际地址和令牌，并可复制带鉴权的 MCP 配置；普通 `service status` 不输出令牌。设置 `NL2SH_PROTOCOL_TOKEN` 可在启动时提供固定令牌；未设置时服务重启会更换令牌。此开关不启用 `protocol_auto_approve`。

协议端口占用或同一配置已有独立协议进程时，后台启动报错并清理本次启动的资源，不接管或停止独立进程；Web 启动失败也会关闭本次新建的协议监听器。需自定义公告地址/监听参数时，关闭此开关并单独运行 `protocol serve`。

## 一条命令启动设备 HTTP 服务

在设备 shell/root 终端运行：

```sh
nl2sh protocol serve
```

使用默认配置路径；需要其他配置时加 `--config /data/local/tmp/config.toml`。默认监听 `0.0.0.0:8765` 并允许 HTTP，不启动 TUI 或 Web。未设置 `NL2SH_PROTOCOL_TOKEN` 时，从系统安全随机源生成 64 字符令牌，并在启动终端打印 MCP、A2A、公开 Agent Card 地址、令牌和 MCP 客户端配置。将整段连接信息交给外部 Agent，或把令牌填入客户端的 Bearer 鉴权/`NL2SH_PROTOCOL_TOKEN`。自动令牌每次启动变化，不写入配置；运行期间保存在私有 `protocol/connection.json`（0600），供 TUI/Web 显示连接凭据。停止或身份/权限无法确认时不展示旧令牌，普通状态查询和公开 Agent Card 不输出令牌。启动输出包含凭据，应只交给可信调用者。

默认自动选取 IPv4 路由的源地址，不发送探测数据包、不查询 DNS；失败时枚举活动网卡的非回环 IPv4，再无可用地址时公告 `127.0.0.1`，此时公告地址仅本机可用。使用绑定后的实际端口，包括 `--port 0`。IP 在启动时获取；网络/IP 改变后重启协议服务并更新客户端。

`--host` 决定绑定范围，`0.0.0.0` 表示所有 IPv4 接口，不能作为客户端目标。`--advertised-url` 决定向客户端公告的 HTTP(S) origin：Agent Card 据此生成 A2A 接口地址，MCP 与连接说明使用同一地址，Host/Origin 校验也据此限制调用。正常设备网络无需填写；多网卡、VPN、NAT 或 HTTPS 代理需要另一个可达地址时可覆盖：

```sh
nl2sh protocol serve --advertised-url https://agent.example.com
```

自动获取不保证远端可达，也不会自动配置端口映射、NAT 或 HTTPS。普通 HTTP 明文传输令牌；公网接入使用 HTTPS 反向代理，代理需保留公告 Host。公告 URL 不允许路径前缀、凭据、query 或 fragment，也不能为 `0.0.0.0`/`::`。只需本机访问时用 `--host 127.0.0.1`；`--allow-insecure-http` 默认为 true，`--allow-insecure-http=false` 要求本机监听。

需要固定令牌以便重启后继续连接，可显式覆盖（32–256 个可打印 ASCII 字符）；配置的令牌不会在启动输出中显示，空值或不合法值会拒绝启动：

```sh
export NL2SH_PROTOCOL_TOKEN='替换为32到256字符的固定随机令牌'
nl2sh protocol serve
```

公开发现为 `/.well-known/agent-card.json`；`/mcp` 与 `/a2a` 均需 `Authorization: Bearer <token>`。Web 服务有独立端口和访问策略，协议令牌不改变 Web 的访问行为。每个状态目录仅允许一个协议进程；HTTP 与 stdio 不应同时使用同一目录。

后台启动时让输出日志保持私有，并从日志读取自动令牌和连接信息：

```sh
umask 077
nohup nl2sh protocol serve </dev/null >nl2sh-protocol.log 2>&1 &
```

保护日志和配置目录，不把令牌放入公共脚本或版本控制。设备需维持网络可达和进程存活；启动命令不会自动建立后台保活。SIGINT/SIGTERM 请求取消并等待当前操作安全结束，随后关闭服务；取消不回滚已经发生的动作。

## 连接 MCP 客户端

HTTP 客户端直接使用 `http://设备IP:8765/mcp`，远程优先 HTTPS。使用 Streamable HTTP，路径不是 `/sse`。客户端环境中的 `NL2SH_PROTOCOL_TOKEN` 必须与设备服务令牌相同。

支持相应 MCP 配置的客户端可填写：

```toml
[mcp_servers.nl2sh]
url = "http://设备IP:8765/mcp"
bearer_token_env_var = "NL2SH_PROTOCOL_TOKEN"
tool_timeout_sec = 210
```

先调用 `nl2sh_tools` 获取当前工具名称和 Schema，再调用 `nl2sh_invoke`，例如：

```json
{"tool":"android.screen_dump","arguments":{}}
```

视觉步骤调用 `nl2sh_read_screen`，返回文本和 PNG/JPEG MCP 图像块。检查 `success`、`isError` 和新的设备证据后再宣称完成。`nl2sh_ask` 进入设备 Agent，返回 Task；保存 `contextId`，后续调用用 `context_id` 续问。MCP 工具执行不经过 A2A，也不把直接工具调用加入 Agent 历史。

本地 MCP 客户端与 nl2sh 位于同一设备或主机时，可直接启动 stdio：

```sh
nl2sh --config /path/to/config.toml protocol stdio
```

stdio 不需要 Bearer 令牌，使用启动用户的本地权限。stdout 专用于 MCP，诊断使用 stderr。远程客户端不能通过这个命令启动另一台设备上的进程，应使用 HTTP 接入。

## A2A Agent 委派

Agent Card 公告 A2A 1.0 JSON-RPC。发送普通用户文本给 `/a2a`，不使用旧的斜杠工具命令：

```json
{
  "jsonrpc": "2.0",
  "id": "request-1",
  "method": "SendMessage",
  "params": {
    "message": {
      "messageId": "unique-message-1",
      "role": "ROLE_USER",
      "parts": [{"text": "读取设备环境并总结"}]
    },
    "configuration": {"returnImmediately": true}
  }
}
```

请求带 `Content-Type: application/json`、`Authorization: Bearer <token>` 和 `A2A-Version: 1.0`。`returnImmediately` 返回已提交任务；省略或 false 等待最终状态。用 `GetTask` 查询任务，`ListTasks` 按上下文/状态分页，`CancelTask` 请求取消。同一 `contextId` 的 Agent 任务串行处理；续问发送新的消息与任务，不携带旧 `taskId`。直接工具调用使用 MCP。

## 设备本地审批

默认情况下，工具调用和 Agent 委派遇到修改都等待设备本地一次性审批。另一个**同 UID、同配置路径**的设备交互终端运行：

```sh
nl2sh --config /data/local/tmp/config.toml protocol approvals
nl2sh --config /data/local/tmp/config.toml protocol approve REQUEST_ID
```

审批展示完整动作与本地风险；危险操作还需输入绑定请求 ID 的精确短语。120 秒过期、拒绝或取消都不批准操作。最多同时八个待决审批；MCP/A2A 没有远程批准接口。

显式设置 `protocol_auto_approve = true` 会自动批准协议调用的全部风险等级，包括 Dangerous/Critical，只适用于完全信任的调用者。它默认关闭；配置在新协议任务执行时读取，当前任务保留快照。参数校验、安全评估、命令绑定和 root 能力检查继续执行，TUI/Web/普通 CLI 使用各自审批器。

## 更新与验证

升级设备 nl2sh 后重新启动协议服务。原 `a2a_gateway/`、`nl2sh-a2a`、`nl2sh-a2a-mcp`、`bridge` 命令及 `bridge_auto_approve` 已移除，不提供兼容入口或自动迁移；旧配置需要删除旧字段并按上文重新设置。

内置协议回归：

```sh
cargo test --lib protocol::
cargo test --test protocol_stdio_tests
```

Android 完整 UI 自动化要求 shell/root UID；普通 Termux UID 的能力不同。可选 [Android Bridge](android-bridge.md) 支持实时节点树、Unicode 输入与手势，调用仍使用 nl2sh 的审批和目标复核。
