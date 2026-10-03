# Ollama

Ollama 是可选模型服务，nl2sh 不会自动安装它。先在目标服务机器准备支持工具调用的模型，并核对模型名称。

```toml
endpoint = "http://127.0.0.1:11434/v1"
model = "你已安装的模型名称"
api_key = ""
api_type = "chat_completions"
```

这里的 `127.0.0.1` 是运行 nl2sh 的 Android 设备，不是电脑。电脑运行 Ollama 时，可显式建立 ADB reverse：

```bash
adb reverse tcp:11434 tcp:11434
```

也可使用设备能够访问的受信任服务地址；服务监听设置和模型能力需在 Ollama 侧确认。模型发现使用原生 `/api/tags` 与 `/api/show`，推理使用 OpenAI 兼容入口。对话可用不代表图像、Tool Calling 或上下文元数据全部可用，先运行小型任务验证。

代理绕过列表应包含本地服务主机；更多见 [代理](proxy.md) 与 [网络排查](../troubleshooting/network.md)。
