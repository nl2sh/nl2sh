# Configure a model provider

Prepare a provider API key, exact model name, and Base URL reachable from the device. Direct tool calls do not require a device model; ordinary Agent conversations do.

## Web Quick Start

Open the URL shown at startup. Missing provider configuration opens Quick Start: choose a service, enter key/model and editable Base URL, then save and test a real model conversation. “Get model list” queries the unsaved URL and key without saving configuration; failures retain manual input and show a log correlation ID.

Web Quick Start initially selects DeepSeek / `deepseek-flash`; Rust configuration defaults use OpenRouter / `openrouter/free`. These are distinct entry-point defaults, and Quick Start preserves existing configuration.

## TUI settings

Enter `/config` (alias `/setting`). Tab/Shift+Tab selects categories, Up/Down fields, Left/Right choices, and Ctrl+S saves. Provider choices include OpenRouter, OpenAI, DeepSeek, Moonshot/Kimi, SiliconFlow, Ollama, and Custom. Switching presets fills the URL while preserving key/model/protocol. Ollama and Custom retain separate URL drafts during the settings session. The model/Agent category can discover models and fill context metadata.

## Minimal configuration

```toml
endpoint = "https://openrouter.ai/api/v1"
model = "openrouter/free"
api_key = "your-api-key"
ui_language = "en"
```

Configuration files use private `0600` permissions. Direct deployment defaults to the resolved executable's directory; Termux uses XDG paths. `NL2SH_API_KEY` overrides the file key; CLI `--endpoint` / `--model` / `--api-type` override file values.

The default `api_type` is `auto`: try Responses, then fall back to Chat Completions for incompatible protocols before output. Authentication, rate limits, timeouts, and errors after output do not switch protocols. Available models, prices, and limits depend on the provider.

Next: [first task](first-task.md), [custom providers](../advanced/custom-provider.md), [Ollama](../advanced/ollama.md), or [configuration reference](../reference/configuration.md).
