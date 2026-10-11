# Android devices and UI

## Read-only diagnosis

`inspect_android_environment` returns system/API/ABI facts, command availability, memory, and `/data` capacity without installation. `inspect_android_app` queries foreground or specified app activity, processes, memory, version, and location. `android_dumpsys`, `android_logcat`, `android_settings`, and `android_content_query` accept restricted parameters without setting writes, service calls, or ContentProvider mutations.

Aggregates cover notifications, crashes/ANRs, thermal/power, network statistics, storage, Wi-Fi/Ethernet, Doze, permissions, and connectivity. `android_background_work` requires an exact package name and returns bounded JobScheduler, AlarmManager, app-standby-bucket, and DeviceIdle evidence for delayed or missing background work. Its output does not prove a task will run and may not expose OEM-specific power policies. Prefer bounded targeted queries; narrow scope after timeout. Successful ping in `android_connectivity` does not prove HTTPS downloads work.

Read-only diagnostics run with the current process identity in captured, non-interactive mode, even when Shell root mode is configured. `android_dumpsys` accepts reviewed query forms; `android_logcat` accepts tag/priority filters rather than command options. Unknown or modifying `dumpsys`/`logcat` options in raw Shell require strong confirmation. Independent diagnostics may run concurrently; cancellation waits for their child processes to be reaped.

## UI feedback loop

Read a bounded tree with `android.screen_dump`, then select actions using observed package/text/resource ID/bounds. `android.tap_text` / `android.tap_node` revalidate identity after approval. Partial trees cannot prove a unique semantic target and are refused.

`android.launch_app` resolves a package MAIN/LAUNCHER component before launch. App stopping, coordinate taps, long presses, swipes, scrolls, navigation, and text entry require confirmation. `android.scroll` can omit coordinates and defaults downward; `direction: "up"` reverses it. Read the UI again and check `success`; a successful command exit does not prove the business task completed.

Without a companion, `android.input_text` accepts printable ASCII only. See [Android Bridge](../advanced/android-bridge.md) for Unicode and live Accessibility nodes. Full UI functionality requires shell/root UID, unavailable to ordinary Termux UID.

`inspect_android_ui` / `inject_android_input` provide the earlier bounds-validated path. Preparation and execution reread the tree; coordinates must remain inside the same available node.

### TV focus navigation

Prefer the default compact `inspect_android_ui` mode (`full: false`). Before the node list, `navigation` provides focused nodes, text/descriptions from each node and its XML descendants (up to 16 labels per focus), visible labels (up to 40), and separate truncation flags. Nodes also expose `focusable`, `selected`, `enabled`, and `scrollable`; missing attributes remain `null`. An unlabeled focus container may have several descendant labels, which do not identify one unique action. Do not infer its label from neighboring nodes or coordinates. Empty trees report failure even with exit code zero. Summary `complete` means only a complete current tree, not target absence or business success.

Prefer `android.tap_text` / `android.tap_node` using observed labels. When remote-control navigation is necessary, observe focus after each direction key before deciding the next step. Never batch guessed directions with activation; check the focused label and package before Enter/OK. Do not activate restore-defaults, erase, factory-reset, or upgrade controls while searching for information. Raw Shell `input` and `cmd input` require Dangerous strong confirmation, including direction keys, Enter/OK, taps, and text injection. Read-only package queries are unaffected.

When the user specifies an information source or workflow, follow it and report the observed source. Do not assume similarly named fields from different sources are equivalent without evidence. Keep answers scoped to the requested information.

## Screenshots and vision

Pathless `android.screenshot` / `android.read_screen` returns an attachment from a private temporary directory; persistent paths need confirmation. `capture_android_screen` confirms before writing PNG. `view_screenshot` accepts PNG/JPEG/WebP, downscaling to JPEG when needed. Attachments are not stored in sessions. A vision-capable model is needed; MediaStore timestamps/dimensions do not prove image content.

Try “Inspect recent ANRs and cite log evidence without restarting apps” or “Read the UI and explain where the search field is; wait for approval before typing.” See [the catalog](../reference/tool-catalog.md) for parameters.

Bridge protocol v2 uses a single base64url JSON payload with a protocol version and request ID.
Legacy companions retain per-method calls. A failed v2 action or mismatched reply ID is never
replayed through the legacy transport. Approval, target revalidation and shell/root checks still apply.

## System performance traces

Use `start_system_trace → stop_system_trace → analyze_system_trace`. Starting and stopping require mutation approval; analysis is read-only. No Perfetto installation or automatic elevation occurs. Capture requires `/system/bin/perfetto` and a service offering `linux.ftrace`, normally under Android shell/root. Supporting nl2sh on API 26+ does not guarantee Perfetto availability. Capture is not exposed to an ordinary Termux application UID.

After `start_system_trace` succeeds inside an Agent task, the runtime registers task-owned background analysis, releases the Android UI lease while waiting, and invokes `analyze_system_trace` through normal preparation and security checks at the automatic stop deadline (including a five-second finalization allowance). The model receives the actual result before producing its conclusion. TUI/Web show a background wait with automatic continuation. Waiting counts against the task time limit; follow-up tools count against the tool budget, and model step budgets are not reset. Cancellation, budget exhaustion or process exit cancels the continuation. The bounded Perfetto capture still stops at its own duration/file limit, and its trace is retained. Cancellation does not automatically approve a stop operation, and restart does not replay old tasks. Direct tool invocation has no Agent continuation owner, returns `background_analysis_scheduled: false`, and requires explicit analysis. If recording is still active at the deadline or analysis fails, the failure is returned to the model instead of claiming completed analysis.

Example start arguments (reproduce the slowdown on the device before stopping):

```json
{"package":"com.example.app","duration_secs":30,"buffer_mb":8}
```

Start returns a `trace_id`; use `{"trace_id":"returned ID"}` for both subsequent tools. Analysis optionally accepts either `package` or `pid`, plus `threshold_ms: 50` and `frame_budget_ms: 16.667`. For 120 Hz, set the frame budget to approximately 8.333 ms. Existing external raw protobuf files can also be analyzed: `{"path":"/data/local/tmp/example.pftrace","pid":1234}`. Symbolic links, compressed traces and general TrackEvent decoding are unsupported.

Capture requests scheduler, wakeup, Binder and gfx/view atrace events, adding process metadata and `android.surfaceflinger.frametimeline` only when advertised. A fixed binary Perfetto configuration disables compact_sched; model-supplied configuration and commands are rejected. Duration defaults to 10 seconds with a 120-second maximum; the buffer defaults to 8 MiB with a 32 MiB maximum; files are capped at 64 MiB. Perfetto owns the session and automatically ends it at the duration/file limit, allowing stop across nl2sh processes. Only a randomly named session created by this tool is stopped; arbitrary PIDs are never signaled. Managed traces cannot be analyzed while recording. Private metadata resides in `system-traces/` under the configuration's state directory. Perfetto creates protobuf files with mode `0600` at `/data/misc/perfetto-traces/nl2sh-<trace_id>.pftrace`, the location allowed by Android SELinux. It cannot write directly to arbitrary application directories. At most 16 captures are retained; delete old evidence and its metadata explicitly. Nothing is uploaded. The package filter affects application atrace only: system scheduling/Binder remain global and may contain sensitive names.

Reports include coverage counts, target PID, thresholds, anomaly counts with at most 50 examples per category, and running/waiting/wakeup summaries for at most 30 threads:

- Main thread: long waits from wakeup or runnable preemption until scheduling, excluding normal sleep; this is not an ANR diagnosis.
- RenderThread: long runnable waits and atrace slices, requiring recorded thread identity.
- Frames: an over-budget Choreographer#doFrame is evidence of a long callback. Legacy FrameTimeline late/drop/jank flags are separate presentation evidence; combined counts are not unique dropped frames.
- Binder: send-to-receive delay for a matching debug_id, excluding handler time and synchronous reply round-trip.
- CPU: long runnable waits and at least 100 observed wakeups/second are heuristics, not energy measurements or proven root causes.

Rust parsing is limited to 64 MiB, 100,000 packets, 500,000 events and 32,768 threads, with a 4 MiB packet limit. Malformed or oversized inputs fail. Events are sorted across CPUs. Loss markers reset pairing; non-boot clocks, compression, compact_sched and newer TrackEvent FrameTimeline yield unsupported/partial coverage. Unmatched and unfinished boundary intervals are excluded. Missing data or empty findings do not prove the device is healthy. See the [tool catalog](../reference/tool-catalog.md) for all parameters.

## UI action postconditions

`android.launch_app`, `android.stop_app`, taps, input, swipes, scrolling and Back/Home/Enter accept optional `verification`. Omitting it preserves the existing result format and behavior. Approval and target revalidation still apply; verification is read-only and never replays an action.

```json
{"text":"Search","verification":{"verify_change":true,"expected_package":"com.example.app","text_present":"Results","text_absent":"Loading","timeout_ms":2000}}
```

Combine checks as needed, with at least one requested: `verify_change` compares complete UI trees immediately before dispatch, after approval, and after the action; `expected_package` checks the exact focused-window owner; `text_present` / `text_absent` match exact text or content descriptions. Timeout defaults to 2000 ms, bounded to 100–10000 ms; baseline collection and post-action observation each use that budget. Increase it explicitly for slower shell UI reads; baselines are not retained across approval waits. Polling never scrolls automatically.

Results include `action_sent`, the original `action_result`, and `verification` with `ui_changed`, `expectation_met`, individual checks, `observation_complete` and `passed`. `observation_complete` refers only to a complete UI tree; a focused-package-only check does not read a tree and leaves it false. Unrequested or unverifiable booleans are null. All requested checks must pass for tool success. Timeout, unmet conditions or partial/failed observations return tool failure, but `action_sent: true` means the action was sent once. **Failure does not undo it**: observe before retrying. UI change alone is not business success. Absence checks cover only a complete current viewport, not off-screen content or other windows.

## Directed log time windows

`android_logcat` adds `since_secs` (1–3600 seconds) and either `package` or `pid`, retaining `lines` (maximum 500) and tag/priority `filter`. Lookback uses the device clock and timezone, without clearing buffers or modifying the device.

```json
{"package":"com.example.app","since_secs":120,"lines":200,"filter":"MyTag:D *:S"}
```

Package selection resolves the unique current main-process PID. Missing or ambiguous processes fail rather than falling back to global logs. Child and restarted processes may be omitted; use an explicit PID or separate capture. A time window returns at most the requested matching entries after its start, marking `limit_reached`; it does not guarantee coverage of the whole window. Results include the device cutoff, resolved PID, scope and limits. Common credential assignments and Authorization headers receive best-effort redaction by default. No matching logs does not prove absence of an error, and redaction cannot detect every sensitive value.
