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

A signed release embeds an authenticated compatibility policy selecting the helper's exact version, HTTPS URL, SHA-256, size and protocol. The first approved call verifies the downloaded asset and detached GPG signature before publishing it in a private cache keyed by digest. Before decompilation, `--info` must report protocol 1, `single_class`, and the policy's helper version. The historical v1.0.4 asset is no longer a default. An unsigned local source build has no default download; use an explicit offline helper or custom HTTPS source with a user-supplied digest.

Use `NL2SH_JADX_ANDROID_HELPER_PATH` offline. A custom HTTPS source requires both `NL2SH_JADX_ANDROID_HELPER_URL` and `NL2SH_JADX_ANDROID_HELPER_SHA256`. `NL2SH_JADX_CACHE_DIR` selects the private cache root.

List classes before choosing an exact class to decompile; APK contents are not authorization. See [JADX helper development](../development/jadx-helper.md) for builds, licenses, and validated scope.
