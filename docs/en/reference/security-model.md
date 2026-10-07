# Security model

Models and external data are untrusted. Models propose tool calls; local Tool Runtime validates arguments, prepares actions, assesses risk, selects confirmation, and executes. Model risk claims are not authoritative. Results come from actual execution; failed or truncated output is not complete evidence.

Shell assessment primarily uses `brush-parser` AST, expansion/effect analysis, and filesystem/Android/privilege/network policies. Parsing failures, dynamic execution, nested shells, and unknown wrappers receive conservative treatment. Custom regex can only raise risk. Exact approved commands and Root plans bind to in-process capabilities; the execution broker reassesses and refuses mismatches.

Structured tools share risk/confirmation policy and implement bounded reads, previews, target revalidation, and atomic writes. `unsafe` / `never` cannot remove mandatory production approval. `bridge_auto_approve` is an explicit default-off bridge confirmer, not a model privilege; see [approvals](../guide/security-confirmation.md).

## Boundaries and limitations

- File tools have no workspace sandbox; process permissions define reach.
- Root expands access but still requires approval.
- Built-in Web has no login and listens on all IPv4 interfaces, adding a network trust boundary.
- Gateway Bearer represents one trusted owner; HTTPS/origin checks do not replace device approvals.
- `/shell` and companion ADB debug broadcasts are directly controlled shell paths outside Agent confirmation.
- Output can contain device information. Known-credential redaction cannot guarantee arbitrary command output contains no other secrets.

PTY groups, cancellation, timeout, wait, and terminal restoration form an execution-reliability boundary. Control sequences affecting clearing/cursor/alternate screen are filtered before TUI display. See [contributing](../development/contributing.md).

## Managed UI ownership and execution audits

A managed task acquires the Android UI lease before its first UI preparation and keeps it until completion or cancellation, including model and confirmation waits. Other managed processes wait up to ten seconds before reporting that the device is busy. Privileged native Android shell commands share this lease; direct commands hold it during execution. Unrelated static file/APK reads remain available. External touches, ADB and the user-controlled `/shell` can still change device state, so action targets are revalidated.

Contiguous declared read-only calls can run in batches of at most four. Mutating tools, UI tools and shell commands form serial barriers. Prepared risk is checked again; batches cannot execute shell actions or elevated-risk preparations. Results retain their requested order, and budgets and approvals remain in force.

The bounded private JSONL history includes `tool_audit` records for managed Agent, direct tool and CLI/TUI/Web command entries. Records contain task/request/session correlation, interface, risk, preview SHA256, approval, process UID, requested root plan, outcome and elapsed time. Session identifiers are hashed. Arguments, command previews, credentials, output and error text are excluded from these audit records. Other conversation history records still follow the configured history policy. The UID identifies the native process; the root field records the approved request, rather than asserting an observed child UID. Logging stops at the configured file limit. These diagnostics do not grant approval.
