# Android Bridge companion

The standalone Android project is maintained in [`nl2sh/android-bridge`](https://github.com/nl2sh/android-bridge), outside the main repository. See its [English guide](https://github.com/nl2sh/android-bridge/blob/main/docs/en/guide.md) and [release guide](https://github.com/nl2sh/android-bridge/blob/main/docs/en/release.md) for independent builds, CI, and releases. This page covers integration and permissions for native nl2sh.

This optional APK provides live accessibility nodes and Unicode text entry to the single-file nl2sh executable. It contains no Agent, model client, network listener, or approval interface. The device-side nl2sh Tool Runtime still assesses and confirms every action before it sends a request.

## Build and enable

Install Android SDK Platform 35 and Build Tools 35.0.0, set `ANDROID_HOME`, then run:

```sh
git clone https://github.com/nl2sh/android-bridge.git
cd android-bridge
./gradlew --no-daemon :app:assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

Open **nl2sh Android Bridge** on the device and use its buttons to open Accessibility settings or the keyboard settings. Enable the Accessibility service there. A sideloaded APK may require enabling restricted settings for this app in Android's App info screen before Accessibility can be enabled. The APK does not enable the service automatically.

The companion also ships an input method, **nl2sh Keyboard**. Selecting it in the system keyboard settings is optional and enables Unicode entry in fields that report no editable state, because text is then committed through the app's `InputConnection` exactly like ADBKeyboard. Its only key switches back to the previous keyboard, and MainActivity shows which keyboard is currently selected. nl2sh never changes the keyboard for you.

The companion exposes `content://com.nl2sh.bridge.ops` through Android Binder. The exported provider requires Android's DUMP permission and checks that the caller UID is shell (2000) or root (0) before dispatching any action. There is no pairing secret or network port. Native shell/root nl2sh runs can use it after the service is enabled. An ordinary Termux app UID cannot call this provider; Android also denies that UID the shell permissions needed for `input` event injection and `screencap`, so full UI automation requires running nl2sh as shell/root. Termux remains supported for nl2sh's other functions.

When enabled, `android.screen_dump` and semantic node lookup can use its bounded live tree. `android.tap_text` and `android.tap_node` use the node's Accessibility click action and recheck its package, class, resource ID, text, description, and bounds after approval. Incomplete trees cannot prove a unique semantic target and are rejected for node lookup and semantic taps. `android.input_text` uses `ACTION_SET_TEXT` for Unicode, appending to a focused editable non-password control; the control's package, class, resource ID, and bounds are checked before confirmation and again before text is written. A semantic write with no package identity is rejected. Controls that do not implement that action return an explicit failure. `android.swipe` and `android.scroll` use bounded Accessibility gestures and report completion or cancellation; if the companion is unavailable before approval, they use their shell backend. ASCII text can still use Android's shell `input text`. `android.screenshot`, package launch/stop, coordinate tap, and navigation continue to use their shell backends.

`android.input_text` picks one Unicode write backend before it asks for confirmation and keeps it: `ACTION_SET_TEXT` when the focused node reports `isEditable`, the clipboard path when the node hides that flag but still advertises a text action, and the companion input method when the node advertises none at all. `focused_target` reports that capability read-only as `can_write`, so no backend is ever swapped after approval. The input method path commits through the focused editor and is bound to its package and field ID, both rechecked after confirmation; it is also the only backend that can clear a field, which `mode: "replace"` requests. Password editors are always refused. An editor that accepts a commit but cannot report its text — a window-level connection such as a WebView search bar — is reported as a failure instead of a silent no-op. Upgrade the native nl2sh binary and companion together for these package-bound actions.

## Direct shell input

With **nl2sh Keyboard** selected, text can also be typed from an ADB session:

```sh
adb shell am broadcast -a com.nl2sh.bridge.IME_TEXT --es msg '中文'
adb shell am broadcast -a com.nl2sh.bridge.IME_TEXT_B64 --es msg '5Lit5paH'
adb shell am broadcast -a com.nl2sh.bridge.IME_CLEAR
adb shell am broadcast -a com.nl2sh.bridge.IME_TEXT --es mode replace --es msg '替换整段'
```

The receiver is registered by the input method itself with `android.permission.DUMP` as the sender permission, so only shell and root can deliver these broadcasts, and only while the keyboard is loaded. Unlike `android.input_text` this path does not pass through the nl2sh confirmation chain: it carries exactly the authority of `adb shell`, which can already inject text with `input`, and is meant for explicit local debugging. `IME_TEXT_B64` accepts both the standard and URL-safe alphabets.

Node text and descriptions longer than 256 characters are shortened in the returned tree. Hashes of the complete values let `android.tap_text` match a caller-supplied full value and bind bounds-selected nodes through confirmation and the final Accessibility click without expanding the tree response. A dense tree that reaches the Binder reply budget is returned as a marked partial snapshot; semantic lookup and clicks reject that snapshot rather than assuming a node is unique. Native companion calls use a quiet pipe to preserve the encoded reply when ordinary shell execution is configured for PTY.

Disabling the Accessibility service makes the provider return an unavailable result. A pending action fails if the service goes away after nl2sh confirmation; nl2sh does not silently switch backends after approval. Disabling the keyboard makes the IME actions report that the keyboard is not loaded; the Accessibility actions are unaffected, and the two services are enabled and used independently.
