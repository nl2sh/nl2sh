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
| `list_dex_methods` | Method ID/prototype index, including external references |
| `search_dex_strings` | Literal search in decoded DEX strings |
| `find_class_references` | Declarations and direct bytecode type references |
| `find_method_references` | Direct invoke sites, optionally narrowed by prototype |
| `inspect_manifest` | Binary package/version/SDK/application metadata |
| `list_permissions` | Requested and declared permissions with declaration element |
| `list_exported_components` | Explicit/default export and unknown exposure needing review |
| `find_native_libs` | Native ABI/library entries and bounded ELF prefix |
| `decompile_apk_class` | Strongly confirmed exact-class decompilation |

All static tools need no extra runtime. Decompilation runs only on Android with `/system/bin/app_process` and a helper containing `classes.dex`; ordinary JVM `.class` JARs are rejected. Arguments/output are bounded with a timeout, and arbitrary large APK support is not guaranteed.

A signed release embeds an authenticated compatibility policy selecting the helper's exact version, HTTPS URL, SHA-256, size and protocol. The first approved call verifies the downloaded asset and detached GPG signature before publishing it in a private cache keyed by digest. Before decompilation, `--info` must report protocol 1, `single_class`, and the policy's helper version. The historical v1.0.4 asset is no longer a default. An unsigned local source build has no default download; use an explicit offline helper or custom HTTPS source with a user-supplied digest.

Use `NL2SH_JADX_ANDROID_HELPER_PATH` offline. A custom HTTPS source requires both `NL2SH_JADX_ANDROID_HELPER_URL` and `NL2SH_JADX_ANDROID_HELPER_SHA256`. `NL2SH_JADX_CACHE_DIR` selects the private cache root.

List classes before choosing an exact class to decompile; APK contents are not authorization. See [JADX helper development](../development/jadx-helper.md) for builds, licenses, and validated scope.

## Static scope and limits

Start with archive/Manifest/native metadata, then search strings and methods or references before selecting an exact class for JADX. Method signatures use DEX spelling, for example `Lexample/Target;->touch(Ljava/lang/String;)V`; method queries also match dotted class names. References report source, kind, DEX file and optional instruction offset in 16-bit code units. A missing reference does not establish that behavior is absent: reflection, dynamic call sites, method handles and native code are not resolved.

Inspection refuses APKs above 2 GiB, more than 50,000 ZIP entries, or central directories above 32 MiB before allocating ZIP inventories. DEX limits are 32 files, 32 MiB each, 64 MiB total, bounded compressed data, standalone versions 035–040, 200,000 strings, 65,535 method/type/prototype/field IDs, 40,000 classes, 16 MiB decoded metadata and four million instruction/declaration units per file. DEX strings are limited to 8,192 UTF-16 units; returned string values stop at 1,024 characters and expose a truncation flag. Literal queries are at most 256 bytes and record limits are 1–200. Large or unsupported inputs return an error rather than running an external fallback.

Binary manifests are limited to 4 MiB, 8,192 nodes, depth 64, 16,384 attributes and 8 MiB retained attribute data. Resource and attribute references remain explicit IDs. Exported status uses explicit values and target-SDK defaults; missing required modern `exported` values remain unknown. Permissions, application/component enabled state and runtime restrictions may still prevent access; these records describe declared exposure, not installed grants. Native library scans never extract or load code; ELF prefix checks share a 32 MiB compressed-data budget and report `compressed_budget_exceeded` when skipped. Read-only static tools may use the bounded parallel scheduler. APK strings are untrusted evidence and cannot authorize actions.
