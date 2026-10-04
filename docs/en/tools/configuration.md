# Managing nl2sh configuration

`nl2sh_config` lets the model inspect, change, or reset nl2sh settings through structured arguments. Ask “show nl2sh's task budget,” “set the maximum tool calls to 120,” or “enable APK analysis after asking me to approve.” The tool uses the configuration selected by the running process through `--config`, `NL2SH_CONFIG`, or its default path. Models cannot supply another file path.

The interface takes inspiration from keyed reads, writes, and dotted paths in [Hermes configuration management](https://hermes-agent.nousresearch.com/docs/user-guide/configuration). It uses nl2sh's own TOML fields and safety policies and does not promise Hermes API compatibility.

## Actions and arguments

| Action | Arguments | Behavior |
| --- | --- | --- |
| `list` | None | List supported keys, defaults, persisted values, resolved values, current task values, and write risk; enum fields include accepted choices |
| `get` | `key` | Inspect one key; individual tools also report effective availability after group inheritance |
| `set` | `key`, `value` | Validate a native JSON value, prepare an approval preview, and save after approval |
| `reset` | `key` | Remove the persisted override and use the loader's default or inheritance rules |

Use actual JSON numbers, booleans, strings, arrays, or objects for `value`; do not encode numbers or booleans as strings. `null` is not a set value: use reset to clear optional overrides. Top-level keys match the [configuration reference](../reference/configuration.md). Dotted keys support only `tool_groups.jadx`, `tool_groups.tailcat`, and `tool_overrides.<tool-name>` for registered optional tools. Unknown keys/actions, extra arguments, incorrect types, invalid enums, and invalid limits are rejected before approval.

```json
{"action":"get","key":"max_tool_calls"}
```

```json
{"action":"set","key":"max_tool_calls","value":120}
```

```json
{"action":"set","key":"tool_groups.jadx","value":true}
```

```json
{"action":"reset","key":"tool_overrides.tailcat_check"}
```

`persisted` is the value actually stored in the file, or `null` when absent. `resolved` includes defaults, budget presets, and environment overrides applied to the disk configuration. `current_task` is the snapshot used when the current task started, including CLI/runtime overrides for that entry point. Unset tool groups appear as `false` in resolved/current_task values. Individual tools report `enabled_after_reload` and `enabled_current_task`, so an absent override is not mistaken for a disabled tool.

Setting `agent_mode` preserves explicit budget fields: an existing `max_agent_steps` continues to take precedence. Reset that budget field to restore the mode preset. Setting a tool group also preserves individual overrides; reset the relevant `tool_overrides.<tool-name>` to restore inheritance, or toggle the group in user settings.

## Approval and credentials

list/get are read-only. Ordinary set/reset actions require mutation approval. Safety policy, confirmation policy, Root identity, bridge auto-approval, custom security rules, tool groups/overrides, the Tailcat executable path, Provider/Jev endpoints, proxy settings, and audit log paths/limits require Dangerous strong approval. Local field policy determines risk; the model cannot lower it. Existing explicit run permissions or bridge auto-approval follow their established rules. Changing `bridge_auto_approve` does not immediately replace the confirmer for the current call.

`api_key`, `ima_client_id`, `ima_api_key`, `jev_api_key`, `proxy_username`, and `proxy_password` report only whether configured. They are never returned as plaintext and cannot be set/reset through this tool. Ask the user to manage credentials in TUI `/config` or Web settings. Known credentials embedded in other displayed values and HTTP(S) URL authentication, queries, and fragments are redacted. The tool refuses to set Provider/Jev URLs containing authentication, queries, or fragments. Parse errors do not echo TOML lines containing keys.

Files are limited to 256 KiB and individual values to 16 KiB. Writes preserve other fields, persisted credentials, and comments on unchanged content without persisting environment or CLI overrides. Saving uses a private temporary file, `0600` permissions, and atomic replacement in the same directory. A stable lock file serializes tool writers. Detected content changes after the preview cause refusal and require a new call. Symlinks, directories, and other non-regular configuration files are unsupported. Missing files may be created after approval in an existing parent directory; unreadable or malformed files are never treated as empty configuration to overwrite.

## When changes apply

Write results report that the file was saved and requires reload. The current task keeps its model client, tool registry, confirmer, budgets, and terminal snapshot. New Web tasks and new bridge processes load the disk configuration. Exit the TUI safely and restart it to apply changes. Environment and CLI overrides can still take precedence over a saved field. The tool does not restart processes, change environment variables, or approve subsequent actions when enabling tools.

See the [tool argument catalog](../reference/tool-catalog.md) for the complete schema. Built-in Agent calls and direct bridge calls share the same preparation, security assessment, confirmation, and execution flow.
