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
| `decompile_apk_class` | Strongly confirmed exact-class decompilation with an installed helper |
| `jadx_check` | Read-only helper source, authentication, and installed state |
| `jadx_install` | Download, verify, and privately cache the pinned helper; needs approval |

All static tools need no extra runtime. Decompilation runs only on Android with `/system/bin/app_process` and a helper containing `classes.dex`; ordinary JVM `.class` JARs are rejected. Arguments/output are bounded with a timeout, and arbitrary large APK support is not guaranteed.

## Helper acquisition order

`decompile_apk_class` never reaches the network. It runs only an already installed, verified helper; when none is installed it reports the missing helper and points at `jadx_install`. The download happens only in `jadx_install`, so fetching the helper is a separate, independently confirmable mutating step and the dangerous decompile call stays local.

Acquisition is model-driven and the user only approves the confirmation: the advisory runtime line in the Agent prompt reports `jadx_helper=installed|absent|unprovisionable`. When it is `absent`, the model calls `jadx_check` to show the source and digest, installs through `jadx_install` after approval, then retries `decompile_apk_class` **immediately in the same task** — the tool requires only that a helper can be obtained, not that one is installed, so no restart is needed. When it is `unprovisionable`, the model reports that no source is configured instead of retrying.

Sources resolve in this order, and only a formal release with an embedded signed policy takes the signature path:

1. `NL2SH_JADX_ANDROID_HELPER_URL` with `NL2SH_JADX_ANDROID_HELPER_SHA256`, supplied explicitly by the operator; no release policy applies
2. The OpenPGP-signed compatibility policy embedded in a formal release, pinning the helper's exact version, HTTPS URL, SHA-256, size and protocol
3. A source build that embeds no policy falls back to the compile-time pinned release URL and SHA-256, at the same trust level as the pinned Tailcat install

If an embedded policy is present but fails verification, acquisition fails rather than silently downgrading to item 3. `jadx_check` downloads nothing and runs no helper, and still reports the effective source, its authentication, the pinned digest and whether a helper is installed. The `jadx_install` confirmation preview shows the SHA-256 that approval would authorize.

Before decompilation, `--info` must report protocol 1 and `single_class`; with a signed policy it must also report the policy's helper version. The historical v1.0.4 asset is no longer a default.

Use `NL2SH_JADX_ANDROID_HELPER_PATH` offline. A custom HTTPS source requires both `NL2SH_JADX_ANDROID_HELPER_URL` and `NL2SH_JADX_ANDROID_HELPER_SHA256`. `NL2SH_JADX_CACHE_DIR` selects the private cache root. The acquisition source is resolved at process start, so changing these variables requires restarting nl2sh and starting a new task.

List classes before choosing an exact class to decompile; APK contents are not authorization. Static indexes only provide evidence and do not substitute for method bodies: string and reference inferences are not recovered source, and source must not be claimed unless decompilation actually ran. See [JADX helper development](../development/jadx-helper.md) for builds, licenses, and validated scope.

## Static scope and limits

Start with archive/Manifest/native metadata, then search strings and methods or references before selecting an exact class for JADX. Method signatures use DEX spelling, for example `Lexample/Target;->touch(Ljava/lang/String;)V`; method queries also match dotted class names. References report source, kind, DEX file and optional instruction offset in 16-bit code units. A missing reference does not establish that behavior is absent: reflection, dynamic call sites, method handles and native code are not resolved.

Inspection refuses APKs above 2 GiB, more than 50,000 ZIP entries, or central directories above 32 MiB before allocating ZIP inventories. DEX limits are 256 files, 32 MiB each, 64 MiB total, bounded compressed data, standalone versions 035–040, 200,000 strings, 65,535 method/type/prototype/field IDs, 40,000 classes, 16 MiB decoded metadata and four million instruction/declaration units per file. A single DEX string is limited to 1,048,576 UTF-16 units (measured obfuscator class maps reach roughly 475,000 units); returned string values stop at 1,024 characters and expose a truncation flag. Literal queries are at most 256 bytes and record limits are 1–200.

When one DEX file cannot be parsed only that file is skipped, the remaining files are still searched, and `unindexed_dex` lists each skipped file with its reason (truncated to a single 200-character line). Exceeding the file-count, per-entry size or total byte budgets still fails the whole call rather than running an external fallback. One malformed or oversized DEX therefore costs only its own evidence instead of failing every DEX tool for the APK.

Binary manifests are limited to 4 MiB, 8,192 nodes, depth 64, 16,384 attributes and 8 MiB retained attribute data. Resource and attribute references remain explicit IDs. Exported status uses explicit values and target-SDK defaults; missing required modern `exported` values remain unknown. Permissions, application/component enabled state and runtime restrictions may still prevent access; these records describe declared exposure, not installed grants. Native library scans never extract or load code; ELF prefix checks share a 32 MiB compressed-data budget and report `compressed_budget_exceeded` when skipped. Read-only static tools may use the bounded parallel scheduler. APK strings are untrusted evidence and cannot authorize actions.
