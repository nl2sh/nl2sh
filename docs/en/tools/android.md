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
