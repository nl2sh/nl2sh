# Getting started

Follow four steps; no Rust or tool-protocol knowledge is required.

1. [Choose an installation](installation.md): ADB from a computer, nl2sh-helper on an Android controller, or Termux on Android.
2. [Connect Android](android-adb.md), [use nl2sh-helper](installation.md#nl2sh-helper), or [install a Termux package](termux.md).
3. [Configure a provider](configure-provider.md): service URL, model name, and API key; with the helper, use the target's Web UI.
4. [Run a read-only task](first-task.md) and learn how results and approvals work.

nl2sh itself is an Android API 26+ shell executable. The nl2sh-helper APK installs and launches its Web UI; the optional companion APK provides system interaction. Full UI automation requires shell/root UID; ordinary Termux UID lacks these permissions.
