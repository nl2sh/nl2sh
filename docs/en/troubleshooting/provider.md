# Provider troubleshooting

| Symptom | Check |
| --- | --- |
| 401 / 403 | Provider, complete key, validity, permissions; repeated requests do not fix an invalid key |
| 429 | Quotas/rate limits; allow backoff or reduce request frequency |
| 404 / 405 | Base URL/protocol; initial auto can fall back, selected-protocol 405 has bounded retry |
| 5xx | Temporary upstream failure; retain error evidence |
| Premature stream end | Incomplete result; output streams are not automatically replayed |
| Broken tool JSON | Bounded repair results; broken arguments never execute |
| Empty/failed model list | Enter the exact ID manually; discovery and inference are separate endpoints |

Web model-list failures show correlation IDs for JSONL `web_model_list_started` / `web_model_list_finished`, recording domain/timing/errors without upstream bodies or keys. The post-save connection test performs real model inference without device commands.

For special providers try explicit `chat_completions` / `responses`, first a short request then a read-only Agent task. Override known context limits when metadata is missing; unknown token usage is not zero.

See [custom providers](../advanced/custom-provider.md) and [proxies](../advanced/proxy.md).
