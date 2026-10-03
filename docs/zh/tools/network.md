# 网络工具

`http_request` 对公网 HTTP(S) 执行有界 GET/HEAD；`http_post` 确认后可发送有界 JSON POST。禁止重定向、URL 凭据、私网目标与任意自定义 header。请求外部服务本身可能暴露你选择发送的数据，批准 POST 前查看正文。

`download_url` 先获取有界内容并展示 URL、字节数和目标，批准后才原子写入；批准前不创建/替换目标文件。受控下载不等于安装或执行下载内容。

`inspect_tls` 对公网主机直连 TLS 握手，检查主机名、证书有效期及 Mozilla 信任链，返回各级主题/颁发者/时间/SHA-256，不发送 HTTP 请求，也拒绝本机/私网。

这些工具的来源约束与 Provider 客户端不同；Provider 的内网 Ollama 地址由配置决定。连接诊断应分别检查设备网络、DNS、TCP、TLS、HTTP，而非仅看 ping。更多见 [代理](../advanced/proxy.md)。
