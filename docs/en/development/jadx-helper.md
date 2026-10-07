# JADX helper development

The standalone source is maintained in [nl2sh/jadx-helper](https://github.com/nl2sh/jadx-helper). Clone that repository before running its build commands.


This module builds an unsigned Android APK and repackages its runtime entries into `jadx-helper.jar`. It contains `classes.dex` and a narrow `com.nl2sh.jadx.Main` entrypoint for `app_process`; it is never installed as an Android application. The entrypoint accepts an APK path, an exact class name, and an output Java file path. nl2sh provides bounded arguments and output, a timeout, SHA-256 verification, and strong confirmation.

The build pins JADX core and DEX input to 1.5.1, matching the upstream Android library example, and targets Android API 26+. It uses Android Gradle Plugin 8.7.3, Gradle Wrapper 8.9, JDK 17, and Android SDK API 35. Run `./build-helper.sh` on Linux/macOS or `.\build-helper.ps1` in Windows PowerShell; no system Gradle installation is required. The Wrapper verifies the downloaded Gradle distribution with its published SHA-256. Gradle repackages all required runtime entries with reproducible ordering, timestamps, and compression; Windows and Linux builds therefore produce the same pinned JAR. Both scripts write `dist/jadx-helper.jar`, its SHA-256 file, and `metadata.json`; `dist/` is not committed. Include the applicable JADX and dependency licenses when distributing the JAR.

Single-class smoke tests passed on API 35 x86_64, API 28 ARMv7, and API 26 x86_64 ART. Native releases select the helper through an authenticated compatibility manifest; the historical v1.0.4 asset is no longer the default policy. Unsigned local source builds can use an explicit offline DEX helper or a custom HTTPS URL with a user-supplied SHA-256. Strong confirmation still precedes acquisition and decompilation. A manual invocation must provide a private writable temporary directory, for example `CLASSPATH=/data/local/tmp/jadx-helper.jar /system/bin/app_process -Djava.io.tmpdir=/data/local/tmp/jadx-work / com.nl2sh.jadx.Main <apk> <class> <output>`. Larger multidex APKs, memory use, and timeout cleanup need broader coverage.

Reproducibility comparisons require the same Git revision and pinned toolchain because AGP packages revision metadata. The build packages `assets/nl2sh-runtime.json`; packaging scripts derive `metadata.json` from those bytes and add the actual SHA-256 and size. Runtime `--info` and packaged metadata share build version/protocol fields. Tagged releases set versions explicitly. The main native release signs the aggregated JAR with its release trust root.

## Entrypoint contract and failures

Invoke `com.nl2sh.jadx.Main` with exactly three arguments: input APK, original fully qualified class name, and output Java file. The output parent and `java.io.tmpdir` must already exist and be writable. Output is UTF-8 and an existing output file is overwritten. The helper uses one worker, SIMPLE decompilation, no code cache, no imports/debug information/inlining/renaming, and a filter for the exact class plus its `$` inner classes. Resources are skipped and XML parsing is explicitly disabled. It does not export an entire project or guarantee recompilable source.

| Exit | Meaning |
| --- | --- |
| 0 | Source was written |
| 2 | Argument count is not three |
| 3 | JADX returned empty source |
| 4 | Original class name was not found |
| 5 | Loading, decompilation or file writing raised an exception; inspect stderr |

These codes describe the helper itself. Native nl2sh applies its own timeout and bounded output handling. A manual `app_process` invocation does not pass through nl2sh approval or inherit its bounds; use the native tool for the normal Agent workflow. For a missing class, verify its original name rather than its display/deobfuscated alias. A DEX/JAR loading error requires checking the artifact format and device compatibility; a temporary-directory error requires a private writable directory.

## Runtime information

`CLASSPATH=/data/local/tmp/jadx-helper.jar /system/bin/app_process / com.nl2sh.jadx.Main --info`
returns one JSON object with `protocol: 1`, the build's `helper_version`, pinned `jadx_core`, and
`features: ["single_class", "inner_classes"]`. It exits successfully without opening an APK or
writing files. Clients must reject unsupported protocol versions before decompilation. SHA-256
and DEX validation remain required; runtime information does not authenticate downloaded code.

Protocol 1 `--info`, SHA-256 and the existing three-argument single-class entrypoint passed
on API 26 x86_64 ART. `./build-helper.sh` and `./gradlew --no-daemon :app:lintRelease` passed.
