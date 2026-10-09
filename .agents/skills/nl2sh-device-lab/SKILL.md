---
name: nl2sh-device-lab
description: Develop and validate nl2sh against an Android device or emulator through its native MCP tools, with source identity, diagnostic evidence and repeatable regression cases.
---

# nl2sh device lab

Codex owns source analysis, implementation and decisions. The device nl2sh owns
device observations and may propose hypotheses and recommendations. It cannot
modify host source or commit code.

Read repository AGENTS.md and its required project context before edits. Inspect
`src/protocol/`, the relevant registered tool and existing deployment scripts;
use the current native server rather than historical bridge/gateway instructions.

## Choose a mode

- **Deterministic:** discover with `nl2sh_inspect` and `nl2sh_tools`, then use
  `nl2sh_invoke` for a known tool with arguments matching its actual schema.
  No device model is needed. Prefer this for assertions and repeatable regression.
- **Delegated:** use `nl2sh_ask` for open-ended investigation. State the question,
  requested evidence, constraints and unresolved hypotheses. Save returned
  `contextId` and send it as `context_id` for follow-up questions. Device model
  use incurs inference cost. For long tasks use A2A SendMessage with
  `returnImmediately` and poll the known task rather than resubmitting.

## Verification loop

Read [the diagnostic contract](references/diagnosis-contract.md) when collecting
or reviewing evidence and assertions.
For reproducible builds, cases, deployment and report comparison, read
[the helper workflow](references/workflow.md) and use `scripts/device_lab.py`.

1. Identify the source revision and worktree changes; establish the running
   binary's build identity before interpreting any behavior as code verification.
   An equal package version alone proves nothing. Dirty builds require a build
   identifier or exact artifact SHA-256; unknown/mismatched identity is unverified.
2. Record the environment and available schemas. Run the baseline case and
   retain bounded actual results, including partial output and failures.
3. Compare evidence against the local implementation. Label observations,
   hypotheses and recommendations separately. An Agent answer is not a tool
   observation; TASK_STATE_COMPLETED does not prove internal tools succeeded.
4. Implement and run repository checks. Build using `cross-compile.sh` for the
   selected device ABI. Deploy and recover through an independent host ADB
   channel; never ask the MCP process to replace or restart itself.
5. Rediscover the actual protocol port and credentials after restart, reconnect,
   verify the running build, and repeat the same case. Compare assertions and
   relevant environment differences, not just natural-language answers.
6. Report the code change, evidence, before/after outcomes and limitations.
   Missing devices/tools, unexecuted cases and incomplete evidence must remain
   `inconclusive` or `manual_review`, never a pass.

## Boundaries and recovery

Keep Security → Confirmation → Execution intact. Do not enable
`protocol_auto_approve`, unsafe policy, root or optional dangerous tools to make
tests pass. Mutations still need the same-UID device-local approval terminal;
explicit deployment authorization does not grant future tool approvals.

On timeout or disconnect, inspect a known task ID before retrying; do not replay
an uncertain mutation. Cancellation does not undo side effects. Service restart
interrupts tasks without replay and may rotate tokens and change ports. Stop
only the service owned by the selected configuration, never processes by name.
Keep tokens, model keys and raw sensitive evidence out of version control and
reports. Use private temporary configurations and bounded private artifacts.
