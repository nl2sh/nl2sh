# Root and execution identity

| `execute_user_mode` | Behavior |
| --- | --- |
| `auto` | Execute directly at UID 0; keep ordinary commands at current identity and try `su` only when needed |
| `normal` | Never invoke `su` automatically |
| `root` | Require UID 0 or working `su` for every command, without fallback |

Root is not a prerequisite. Android permissions determine accessible files, services, and app state. The model's `requires_root` is a hint; local checks choose the elevation plan and bind it to the exact approved command.

Read-only commands under Root conditions also require confirmation. Task exact-command grants and process-lifetime ordinary-mutation grants exclude Root, Dangerous, Critical, and strong-confirmation operations. `su` uses parameterized arguments.

Launcher root adbd / `su` fallback and Agent execution mode operate at different layers: launchers determine the program identity; `execute_user_mode` controls subsequent command plans. Do not relax credential-file permissions to fix an identity mismatch.

See [permission troubleshooting](../troubleshooting/permissions.md) for Termux versus shell/root UID.
