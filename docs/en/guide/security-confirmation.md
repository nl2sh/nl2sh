# Safety approvals

The execution boundary remains `LLM → Security → Confirmation → Execution`. Model explanations are not approval; Root, tool arguments, and edits cannot skip classification. Under balanced/risk-only defaults:

| Risk | Default decision |
| --- | --- |
| ReadOnly | Ordinary reads may run automatically; Root/strict conditions require confirmation |
| Mutating | Show the action and confirm |
| Dangerous / Critical | Strong confirmation |
| Unreliable parsing or dynamic execution | Conservative assessment and strong confirmation |

## TUI / CLI

Read the complete command or diff, local risk, and rule reasons. TUI offers one-time approval, identical ordinary-command approval within a task, process-lifetime ordinary-mutation approval, rejection, editing, and interactive/captured execution. Task grants do not match prefixes; process grants are memory-only. `/permission allow` enables process ordinary-mutation grants; `/permission ask` disables them. Root, high risk, and strong-confirmation actions are excluded. Non-interactive CLI refuses actions requiring unavailable approval.

Enter strong-confirmation text exactly as displayed. A natural-language “yes” does not authorize subsequent actions. Editing restarts the full classification and confirmation chain.

## Web

Mutations use one approval click. Dangerous actions require a first click to enter review and a second to approve, without typing `CONFIRM`. The server checks the pending request ID and review state; old dialogs cannot approve later requests. Rejection and reassessment after editing remain available.

## A2A / MCP

Both direct tools and delegated Agent tasks default to one-time approval in another interactive device terminal, refusing after 120 seconds. Dangerous actions require an exact phrase. Use `nl2sh protocol approvals` / `nl2sh protocol approve REQUEST_ID` with the same UID and configuration path. Protocol clients have no approval endpoint.

!!! warning "Explicit protocol auto-approval"

    `protocol_auto_approve = true` approves protocol calls at every risk level, including Dangerous/Critical. It defaults off and applies only to protocol entry points. Classification, parameter validation, and identity binding still run. Give the service token only to fully trusted clients.

Safety checks cannot guarantee model understanding. Inspect targets and side effects; reject uncertain actions and request read-only diagnosis first. See [the security model](../reference/security-model.md).

## Approval grants

`[[approval_grants]]` matches the final security assessment against a tool, trusted package and risk ceiling. A match releases confirmation and atomically consumes a use. CLI, TUI, Web (including its safe terminal), MCP/A2A and the built-in Agent share this boundary. The default list is empty; unmatched requests retain their existing approval policy. Read-only operations that already need no confirmation consume no uses.

Set top-level fields first, then place the array tables at the end of the file:

```toml
allow_dangerous_grants = false

[[approval_grants]]
id = "app-launch-20"
tool = "android.launch_app"
package = "com.example.app"
max_risk = "mutating"
expires_at = "2030-01-01T00:00:00Z"
uses = 20
```

- `id` is unique and contains 1–128 ASCII letters, digits, dots, underscores or hyphens. Counters and revocations bind to the configuration file identity and ID. Restarting or removing and re-adding an ID does not replenish uses. Use a new ID for a new authorization. Lowering the configured budget tightens the remaining count; raising it does not replenish a persisted budget.
- Omit `tool` to cover any tool, or specify an exact name or a single trailing `*` prefix, such as `android.*`. Actions wrapped in shell use `execute_shell_command`; no package is inferred from shell text.
- `package` exactly matches validated launch/stop arguments or a prepared semantic click node. Requests without a trusted package cannot match package-scoped grants. Omitting the package covers only requests without a package scope, including coordinate taps, gestures, keys and text input; it does not cover known packages. Foreground packages are never inferred.
- `max_risk` is `read_only`, `mutating`, `dangerous` or `critical`. It compares the final assessed risk without lowering it. Critical, root and double-confirmation requests are always excluded.
- Optional `expires_at` is an RFC 3339 wall-clock deadline: authorization ends at that instant. Changing device time changes the window. Optional `uses` is a positive integer; omission means unlimited uses. Failure, cancellation or failed target revalidation does not refund consumed uses.

!!! warning "Dangerous grants default off"

    Dangerous matching requires both `allow_dangerous_grants = true` and the startup flag `--allow-dangerous-grants`, which emits a warning. Service start/restart forwards the explicit flag to its child. Root, Critical and double-confirmation exclusions still apply. Currently every Dangerous tool requires double confirmation, so this flag cannot automatically approve UI taps or text input. It only permits Dangerous assessments without double confirmation; it is not unattended UI authorization.

Templates reload before each required confirmation; expiry, exhaustion and revocation need no restart. Counters live in a private grant directory under the configuration's state directory (0700 directory, 0600 files), with a process-shared file lock and atomic persistence. Corrupt or unsafe state cannot grant approval. TUI execution/security settings show a snapshot when opened; Web advanced rules provide a refreshable read-only list and immediate ID revocation. Revocation does not stop an already-approved or running action. Web management retains the existing owner-interface trust boundary; protocol clients gain no remote approval endpoint.

Audits distinguish grants with `approval = "approved_by_grant"` and `grant_id`. Grant decisions never create remembered task or run permissions.
