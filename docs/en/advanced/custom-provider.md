# Custom providers and protocols

Providers must implement supported Responses or Chat Completions tool-calling structures. Base URLs commonly include `/v1`; do not use a full `/chat/completions` request URL as the base.

```toml
endpoint = "https://provider.example.com/v1"
model = "exact-model-id"
api_key = "your-api-key"
api_type = "auto"
```

Non-OpenAI local services may use an empty key, omitting Authorization rather than sending an empty header. Official OpenAI requires a key. Verify exact model IDs, context windows, and output limits with the provider; override `model_context_window` / `model_max_output_tokens` if needed. Metadata failure still permits manual input.

Automatic negotiation prefers Responses and falls back/caches Chat only for incompatible endpoints/structures before output. Explicit or already negotiated protocols retry transient 405 responses within `llm_retry_count`; a first automatic 405 probe can fall back immediately. 401 is not retried, and streams with output are never automatically replayed.

Use temporary `--endpoint`, `--model`, and `--api-type chat_completions` overrides, validated together. Chat-only services without tool calling cannot reliably power the Agent. Test a short model request, then a small read-only task.

See [provider troubleshooting](../troubleshooting/provider.md) and [Ollama](ollama.md).
