# Networks and proxies

The TUI `/config` network category supports HTTP CONNECT, SOCKS5 (local DNS), SOCKS5H (proxy DNS), credentials, and bypass lists. Disabling the switch preserves fields; passwords display masked.

```toml
proxy_enabled = true
proxy_type = "socks5h"
proxy_address = "192.168.1.10:1080"
proxy_username = ""
proxy_password = ""
proxy_bypass = "localhost,127.0.0.1,::1"
```

Use host and port without a scheme or embedded credentials. Provider inference, discovery, and balances share the policy; update clients also read configuration. ima always connects directly. Public tools and fixed-dependency downloads retain separate origin restrictions.

The proxy must be reachable from Android. A computer's loopback is not the device loopback. For a host proxy explicitly use `adb reverse tcp:7890 tcp:7890`, then device `127.0.0.1:7890`, verifying the HTTP type. Host installer HTTP_PROXY settings affect host downloads rather than replacing device TOML.

Check service URL, DNS, proxy authentication, then TLS/HTTP. 401 and 429 have different causes; see [network troubleshooting](../troubleshooting/network.md).
