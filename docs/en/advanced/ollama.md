# Ollama

Ollama is an optional model service and is not installed automatically. Prepare a tool-capable model on the service host and verify its exact name.

```toml
endpoint = "http://127.0.0.1:11434/v1"
model = "installed-model-name"
api_key = ""
api_type = "chat_completions"
```

Here `127.0.0.1` means the Android device running nl2sh, not the computer. For Ollama on the computer, explicitly establish ADB reverse:

```bash
adb reverse tcp:11434 tcp:11434
```

Alternatively use a trusted service address reachable from the device, checking Ollama listening settings and model capabilities. Discovery uses native `/api/tags` and `/api/show`; inference uses the OpenAI-compatible endpoint. Successful chat does not prove vision/tool-calling/context-metadata support. Verify a small task first.

Include local service hosts in proxy bypass lists. See [proxies](proxy.md) and [network troubleshooting](../troubleshooting/network.md).
