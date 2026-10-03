# Network tools

`http_request` performs bounded public HTTP(S) GET/HEAD. `http_post` performs confirmed bounded JSON POST. Redirects, URL credentials, private-network targets, and arbitrary custom headers are rejected. External requests can expose selected data; inspect POST bodies before approval.

`download_url` fetches bounded content and displays URL, byte count, and target before approval. It creates/replaces the destination atomically only after approval. Downloading does not install or execute content.

`inspect_tls` connects directly to a public host for TLS, validating hostname, certificate validity, and Mozilla trust roots. It returns chain subjects/issuers/dates/SHA-256, sends no HTTP request, and rejects local/private hosts.

These source restrictions differ from provider clients, whose local Ollama URLs come from configuration. Diagnose device network, DNS, TCP, TLS, and HTTP separately rather than relying on ping. See [proxy configuration](../advanced/proxy.md).
