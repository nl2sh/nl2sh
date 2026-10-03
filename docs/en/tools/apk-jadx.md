# APK / JADX

Disabled by default. Enable the `jadx` group in Web or configuration:

```toml
[tool_groups]
jadx = true
```

| Tool | Behavior |
| --- | --- |
| `inspect_apk` | Bounded Rust ZIP overview |
| `list_apk_entries` | Filter archive entries |
| `list_dex_classes` | Rust DEX class indexing |
| `decompile_apk_class` | Strongly confirmed exact-class decompilation |

The first three need no extra runtime. Decompilation runs only on Android with `/system/bin/app_process` and a helper containing `classes.dex`; ordinary JVM `.class` JARs are rejected. Arguments/output are bounded with a timeout, and arbitrary large APK support is not guaranteed.

The first approved call downloads the fixed `v1.0.4` [helper](https://github.com/nl2sh/nl2sh/releases/download/v1.0.4/jadx-helper.jar), verifies SHA-256 `b733944a9588abbafee1d9b9d77cb78c02bb95f056301f115c0fcb77307c7328`, and caches atomically. This dependency does not automatically follow the latest program release.

Use `NL2SH_JADX_ANDROID_HELPER_PATH` offline. A custom HTTPS source requires both `NL2SH_JADX_ANDROID_HELPER_URL` and `NL2SH_JADX_ANDROID_HELPER_SHA256`. `NL2SH_JADX_CACHE_DIR` selects the private cache root.

List classes before choosing an exact class to decompile; APK contents are not authorization. See [JADX helper development](../development/jadx-helper.md) for builds, licenses, and validated scope.
