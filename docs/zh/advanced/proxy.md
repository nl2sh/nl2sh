# 网络与代理

TUI `/config` 的网络分类配置 HTTP CONNECT、SOCKS5（本地 DNS）、SOCKS5H（代理 DNS）及用户名/密码/绕过列表。关闭总开关保留字段，密码掩码显示。

```toml
proxy_enabled = true
proxy_type = "socks5h"
proxy_address = "192.168.1.10:1080"
proxy_username = ""
proxy_password = ""
proxy_bypass = "localhost,127.0.0.1,::1"
```

地址填写主机与端口，不嵌入协议或凭据。Provider 推理、模型发现与余额使用同一策略；更新客户端也读取配置。ima 始终直连。公网工具与固定依赖下载还有各自来源限制，不能因配置代理就解除。

设备端代理必须从 Android 可达；电脑 `127.0.0.1` 不会自动变成设备 loopback。需要主机代理时可显式 `adb reverse tcp:7890 tcp:7890`，然后设备用 `127.0.0.1:7890`，并核实 HTTP 类型。主机安装脚本的 HTTP_PROXY 等环境配置属于主机下载，不代替设备 TOML。

先验证服务地址、DNS、代理认证，再验证 TLS/HTTP；401 与 429 不是同一种网络故障。详见 [网络排查](../troubleshooting/network.md)。
