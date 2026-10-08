# Android devices and UI

## Read-only diagnosis

`inspect_android_environment` returns system/API/ABI facts, command availability, memory, and `/data` capacity without installation. `inspect_android_app` queries foreground or specified app activity, processes, memory, version, and location. `android_dumpsys`, `android_logcat`, `android_settings`, and `android_content_query` accept restricted parameters without setting writes, service calls, or ContentProvider mutations.

Aggregates cover notifications, crashes/ANRs, thermal/power, network statistics, storage, Wi-Fi/Ethernet, Doze, permissions, and connectivity. Prefer bounded targeted queries; narrow scope after timeout. Successful ping in `android_connectivity` does not prove HTTPS downloads work.

Read-only diagnostics run with the current process identity in captured, non-interactive mode, even when Shell root mode is configured. `android_dumpsys` accepts reviewed query forms; `android_logcat` accepts tag/priority filters rather than command options. Unknown or modifying `dumpsys`/`logcat` options in raw Shell require strong confirmation. Independent diagnostics may run concurrently; cancellation waits for their child processes to be reaped.

## UI feedback loop

Read a bounded tree with `android.screen_dump`, then select actions using observed package/text/resource ID/bounds. `android.tap_text` / `android.tap_node` revalidate identity after approval. Partial trees cannot prove a unique semantic target and are refused.

`android.launch_app` resolves a package MAIN/LAUNCHER component before launch. App stopping, coordinate taps, long presses, swipes, scrolls, navigation, and text entry require confirmation. `android.scroll` can omit coordinates and defaults downward; `direction: "up"` reverses it. Read the UI again and check `success`; a successful command exit does not prove the business task completed.

Without a companion, `android.input_text` accepts printable ASCII only. See [Android Bridge](../advanced/android-bridge.md) for Unicode and live Accessibility nodes. Full UI functionality requires shell/root UID, unavailable to ordinary Termux UID.

`inspect_android_ui` / `inject_android_input` provide the earlier bounds-validated path. Preparation and execution reread the tree; coordinates must remain inside the same available node.

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

Capture requests scheduler, wakeup, Binder and gfx/view atrace events, adding process metadata and `android.surfaceflinger.frametimeline` only when advertised. A fixed binary Perfetto configuration disables compact_sched; model-supplied configuration and commands are rejected. Duration defaults to 10 seconds with a 120-second maximum; the buffer defaults to 8 MiB with a 32 MiB maximum; files are capped at 64 MiB. Perfetto owns the session and automatically ends it at the duration/file limit, allowing stop across bridge processes. Only a randomly named session created by this tool is stopped; arbitrary PIDs are never signaled. Managed traces cannot be analyzed while recording. Private metadata resides in `system-traces/` under the configuration's state directory. Perfetto creates protobuf files with mode `0600` at `/data/misc/perfetto-traces/nl2sh-<trace_id>.pftrace`, the location allowed by Android SELinux. It cannot write directly to arbitrary application directories. At most 16 captures are retained; delete old evidence and its metadata explicitly. Nothing is uploaded. The package filter affects application atrace only: system scheduling/Binder remain global and may contain sensitive names.

Reports include coverage counts, target PID, thresholds, anomaly counts with at most 50 examples per category, and running/waiting/wakeup summaries for at most 30 threads:

- Main thread: long waits from wakeup or runnable preemption until scheduling, excluding normal sleep; this is not an ANR diagnosis.
- RenderThread: long runnable waits and atrace slices, requiring recorded thread identity.
- Frames: an over-budget Choreographer#doFrame is evidence of a long callback. Legacy FrameTimeline late/drop/jank flags are separate presentation evidence; combined counts are not unique dropped frames.
- Binder: send-to-receive delay for a matching debug_id, excluding handler time and synchronous reply round-trip.
- CPU: long runnable waits and at least 100 observed wakeups/second are heuristics, not energy measurements or proven root causes.

Rust parsing is limited to 64 MiB, 100,000 packets, 500,000 events and 32,768 threads, with a 4 MiB packet limit. Malformed or oversized inputs fail. Events are sorted across CPUs. Loss markers reset pairing; non-boot clocks, compression, compact_sched and newer TrackEvent FrameTimeline yield unsupported/partial coverage. Unmatched and unfinished boundary intervals are excluded. Missing data or empty findings do not prove the device is healthy. See the [tool catalog](../reference/tool-catalog.md) for all parameters.
