# Diagnostic evidence contract

Use a private JSON report with `schema_version: 1`, case ID, mode, timestamps,
source revision/worktree identity, running `build_identity`, environment snapshot,
observations, assertions, hypotheses, recommendations and limitations.

Host reports wrap each step with its ID and `source: protocol_result`, retaining
the original MCP result. Nested device observations have a stable ID, source
(`device_tool` or `environment`), tool
name and call/task ID where available, execution status, success, bounded output
and an explicit truncation flag. Keep the original protocol result as evidence;
do not replace it with the Agent's paraphrase. `nl2sh_ask` artifacts expose
`evidence.observations` derived from actual calls. These are bounded model-visible
results, not guaranteed full raw execution output. Missing results and omitted
observations are explicit.

Hypotheses contain cause, confidence (`low`, `medium`, `high`) and
`supporting_evidence` IDs. Recommendations are proposed actions, not actions
already performed. Neither hypotheses nor recommendations become observations
merely because the Agent answered confidently. Unsupported evidence references
make a diagnosis incomplete.

Assertions contain expected, actual and status (`pass`, `fail`, `inconclusive`,
`manual_review`). Fail any explicit failed tool assertion; mark absent or partial
evidence inconclusive. Manual assertions require a recorded reviewer decision.
Do not infer a pass from Task completion, transport success or missing failures.

Version gates compare the running executable SHA-256 to the exact local artifact
and a build manifest tying that artifact to the current source snapshot. Check
commit, dirty state, build ID, target and profile as supporting information.
Revalidate the gate after service restart. A successful regression on a different
artifact cannot verify the new source.

Cases describe preconditions, deterministic MCP steps or delegated prompts,
assertions and cleanup. Cleanup actions use discovered tools and the normal
approval chain; always retain failures and unexecuted cleanup. Compare the same
case revision before and after; changed environments require explicit review.
