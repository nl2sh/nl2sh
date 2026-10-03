# Controlled automation

There are three paths: built-in Agent planning, one-shot CLI, and an external agent directly invoking Device Tool Runtime. Direct calls reduce device model requests while retaining argument validation, risk assessment, and approvals.

1. Inspect read-only environment facts and the tool catalog.
2. Read actual target state: file windows, app UI trees, or service state.
3. Request one specific action and inspect complete targets/arguments in approval.
4. Reread state and check results; retain failures rather than blindly replaying writes.

Non-interactive CLI rejects unavailable approvals. A2A/MCP writes normally wait for a device terminal; `ask` rejects them by default. Explicit bridge auto-approval gives token holders unattended execution at the process privileges; see [A2A/MCP](a2a-mcp.md).

nl2sh has no general scheduler. Host scripts can schedule reads, but scheduling, supervision, permissions, and approval policy must be configured explicitly. `/shell` and companion debug broadcasts are not Agent safety-gated automation interfaces.
