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
