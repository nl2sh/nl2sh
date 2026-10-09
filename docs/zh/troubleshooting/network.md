# 网络排查

1. 确认报错来自电脑安装下载、Android Provider 请求、Web 访问还是设备协议服务；它们使用不同网络环境。
2. 核对完整 URL 与实际监听端口。设备 loopback 指设备自身，ADB forward 从主机访问设备，ADB reverse 从设备访问主机。
3. 分别检查 DNS、TCP、TLS、HTTP。ping 成功只证明 ICMP，不能证明 HTTPS 或模型服务可用。
4. 检查设备 TOML 代理类型、地址、认证与绕过列表，必要时缩小诊断范围。

Web 默认绑定所有 IPv4，端口占用会选择其他端口；读取启动输出，再对实际端口设置 `adb forward tcp:PORT tcp:PORT`。确保主机防火墙和设备网络允许连接，不能以转发成功证明页面已启动。

MCP/A2A 优先使用配置端口（默认 `8765`），占用时自动选择空闲端口，`0` 则直接请求系统分配。自动端口每次启动可能不同，请从 TUI/Web 连接窗口或 `service status --json` 读取实际地址。旧版本若报 `cannot bind protocol listener` 和 `Address already in use`，升级程序，或临时把设备配置中的 `protocol_service_port` 改为 `0`；独立 `protocol serve` 使用 `--port 0`。`service stop` 只停止本配置拥有的受管服务，返回 `stopped` 不表示其他应用已释放端口。不要为腾出端口结束无关服务；鉴权与设备本地审批要求不变。

Provider 超时先核对 Base URL 与路由，不直接增大所有 timeout。固定 helper/Tailcat 下载要求来源与摘要匹配，不能关闭校验规避网络失败。ima 不使用代理，需单独直连验证。

提交 Issue 时附脱敏错误、阶段、配置中的非敏感服务域名及版本，排除 API Key、代理密码、令牌与设备私有资料。
