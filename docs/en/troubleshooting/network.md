# Network troubleshooting

1. Identify whether the failure comes from host installation downloads, device provider requests, Web access, or the device protocol service; they use different environments.
2. Check URLs and actual listener ports. Device loopback means the device. ADB forward reaches the device from the host; ADB reverse reaches the host from the device.
3. Check DNS, TCP, TLS, and HTTP separately. Ping proves ICMP only, not HTTPS/model access.
4. Inspect device TOML proxy type/address/authentication/bypass and narrow diagnostic scope.

Web binds all IPv4 and selects another port if occupied. Read startup output, then set `adb forward tcp:PORT tcp:PORT` for the actual port. Check firewalls and device connectivity; successful forwarding alone does not prove the page started.

MCP/A2A prefers the configured port (default `8765`), selecting an available port if occupied; zero requests a system-assigned port directly. Automatic ports may change on each start; read the actual address from the TUI/Web connections view or `service status --json`. If an older version reports `cannot bind protocol listener` and `Address already in use`, upgrade or temporarily set `protocol_service_port` in the device configuration to `0`; standalone `protocol serve` accepts `--port 0`. `service stop` stops only the managed service owned by this configuration; `stopped` does not mean another application released the port. Do not terminate unrelated services to free the port. Authentication and device-local approval requirements remain unchanged.

For provider timeout, check Base URL and routes before increasing all timeouts. Helper/Tailcat downloads require matching origins/digests; retain verification when diagnosing failures. ima bypasses proxies and needs independent direct-connect checks.

Issues should include redacted errors, stage, non-sensitive provider domain, and version, excluding keys/passwords/tokens/private device data.
