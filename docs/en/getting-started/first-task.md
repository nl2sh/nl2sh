# Run your first task

1. Start nl2sh and configure a model.
2. Enter “Show how much storage this device has left” in the TUI, or select the storage example in Web and send it.
3. Observe model requests, tool calls, exit states, and the final summary. Web may show a storage chart; TUI shows numeric text.
4. Check whether evidence is complete. `partial`, `failed`, `timed_out`, and truncation cannot support a claim that everything succeeded.

This task should read information only. Balanced policy can automatically run ordinary reads; Strict or Root conditions may require confirmation. For changes, read the complete action and risk first. Reject unclear actions and ask for an explanation.

TUI uses F2 to expand results, Ctrl+C to cancel a task, and Ctrl+Q or `/exit` to quit. Web can stop the current session's task; switching sessions does not stop background work.

Try “Inspect recent ANRs without changing the device” or “List files in `@/data/local/tmp`.” `@` provides a path rather than automatically reading all content. [Safety approvals](../guide/security-confirmation.md) explains approval and rejection scopes.
