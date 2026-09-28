# nl2sh Android Accessibility companion

This optional APK provides live accessibility nodes and Unicode text entry to the single-file nl2sh executable. It contains no Agent, model client, network listener, or approval interface. The device-side nl2sh Tool Runtime still assesses and confirms every action before it sends a request.

## Build and enable

Install Android SDK Platform 35 and Build Tools 35.0.0, set `ANDROID_HOME`, then run:

```sh
cd android-bridge
./gradlew :app:assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

Open **nl2sh Android Bridge** on the device and use its button to open Accessibility settings. Enable the service there. A sideloaded APK may require enabling restricted settings for this app in Android's App info screen before Accessibility can be enabled. The APK does not enable the service automatically.

The companion exposes `content://com.nl2sh.bridge.ops` through Android Binder. The exported provider requires Android's DUMP permission and checks that the caller UID is shell (2000) or root (0) before dispatching any action. There is no pairing secret or network port. Native shell/root nl2sh runs can use it after the service is enabled. An ordinary Termux app UID cannot call this provider; Android also denies that UID the shell permissions needed for `input` event injection and `screencap`, so full UI automation requires running nl2sh as shell/root. Termux remains supported for nl2sh's other functions.

When enabled, `android.screen_dump` and semantic node lookup can use its bounded live tree. `android.tap_text` and `android.tap_node` use the node's Accessibility click action and recheck its package, class, resource ID, text, description, and bounds after approval. Incomplete trees cannot prove a unique semantic target and are rejected for node lookup and semantic taps. `android.input_text` uses `ACTION_SET_TEXT` for Unicode, appending to a focused editable non-password control; the control's package, class, resource ID, and bounds are checked before confirmation and again before text is written. A semantic write with no package identity is rejected. Upgrade the native nl2sh binary and companion together for these package-bound actions. Controls that do not implement that action return an explicit failure. `android.swipe` and `android.scroll` use bounded Accessibility gestures and report completion or cancellation; if the companion is unavailable before approval, they use their shell backend. ASCII text can still use Android's shell `input text`. `android.screenshot`, package launch/stop, coordinate tap, and navigation continue to use their shell backends.

Node text and descriptions longer than 256 characters are shortened in the returned tree. Hashes of the complete values let `android.tap_text` match a caller-supplied full value and bind bounds-selected nodes through confirmation and the final Accessibility click without expanding the tree response. A dense tree that reaches the Binder reply budget is returned as a marked partial snapshot; semantic lookup and clicks reject that snapshot rather than assuming a node is unique. Native companion calls use a quiet pipe to preserve the encoded reply when ordinary shell execution is configured for PTY.

Disabling the Accessibility service makes the provider return an unavailable result. A pending action fails if the service goes away after nl2sh confirmation; nl2sh does not silently switch backends after approval.
