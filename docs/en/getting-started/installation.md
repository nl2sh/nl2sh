# Installation and updates

| Environment | Installation | Requirements |
| --- | --- | --- |
| Computer connected to Android | Release ZIP and ADB launcher | Host adb, Android API 26+, ARM64/ARMv7/x86_64 |
| Android controller connected to a target device | nl2sh-helper APK | Controller Android API 26+; target ARM64/ARMv7 with TCP ADB or Android 11+ Wireless debugging |
| Computer bootstrap | Bash / PowerShell / CMD installer | Host network access and adb |
| Android Termux | TUR, signed APT, or local deb | Termux package manager |
| Developer | Cross-compile source | Stable Rust, Node.js 22+, Android NDK |

## Release installation

Download `nl2sh-android.zip` and `SHA256SUMS` from [GitHub Releases](https://github.com/nl2sh/nl2sh/releases/latest), compare the ZIP SHA-256, and extract the entire archive. It includes `bin/arm64-v8a/nl2sh`, `bin/armeabi-v7a/nl2sh`, and `bin/x86_64/nl2sh`; launchers select the ABI automatically. Desktop Linux binaries cannot replace Android builds.

```bash
sha256sum nl2sh-android.zip
cd nl2sh-android
./android-run-linux.sh
```

On Windows compare `Get-FileHash ./nl2sh-android.zip -Algorithm SHA256`, extract, and run `android-run-windows.bat`. Prepare the device using [ADB / native Android](android-adb.md).

## nl2sh-helper

If another Android device is your controller, download a signed APK from [nl2sh-helper Releases](https://github.com/nl2sh/nl2sh-helper/releases/latest), which requires repository access, and install it on the controller. In the helper, choose TCP ADB if it is already enabled on the target, or use a pairing code or QR code for a target running Android 11+ Wireless debugging. Accept the ADB authorization prompt on the target for a first TCP connection. For pairing codes, enter the temporary address, port, and code shown on the target.

After connecting, the helper downloads the latest nl2sh ARM64/ARMv7 binary and SHA-256 file for the target ABI, verifies them, deploys the binary to `/data/local/tmp/nl2sh`, and starts its Web service in the background. Tap the button to open nl2sh in a browser, configure a provider and API key in the Web UI's Quick Start, then send your first task. The controller must be able to reach port 9999 on the target. The Web UI has no login and listens on all IPv4 interfaces, so use a trusted network. The helper does not support x86_64 targets, provide the TUI, or set model credentials automatically. See the [helper guide](https://github.com/nl2sh/nl2sh-helper/blob/main/docs/en/guide.md) for connection details and troubleshooting.

## Bootstrap installation

Pass the API key through your private terminal environment and never commit it.

=== "Linux"

    ```bash
    export NL2SH_API_KEY='your-api-key'
    curl -fsSL https://raw.githubusercontent.com/nl2sh/nl2sh/master/install-android.sh \
      | bash -s -- --provider deepseek --model deepseek-flash
    ```

=== "PowerShell"

    ```powershell
    $env:NL2SH_API_KEY = 'your-api-key'
    & ([scriptblock]::Create((irm https://raw.githubusercontent.com/nl2sh/nl2sh/master/install-android.ps1))) `
      -Provider deepseek -Model deepseek-flash
    ```

=== "CMD"

    ```bat
    set "NL2SH_API_KEY=your-api-key"
    curl.exe -fsSL https://raw.githubusercontent.com/nl2sh/nl2sh/master/install-android.bat -o "%TEMP%\install-nl2sh.bat"
    call "%TEMP%\install-nl2sh.bat" --provider deepseek --model deepseek-flash
    ```

Installers download the ZIP and checksums from one Release. The default directory is `nl2sh-android` under the current directory. Reruns reuse a complete installation; explicit provider/model/endpoint arguments update those fields only. Incomplete directories are refused. Linux pipe installation reconnects the controlling terminal for launch; without one, run the launcher interactively later.

Providers include `openrouter`, `openai`, `deepseek`, `moonshot`/`kimi`, `siliconflow`, `ollama`, and `custom`. Custom services require `--endpoint` / `-Endpoint`.

## Gitee mirror

Explicitly select [Gitee](https://gitee.com/nl2sh/nl2sh): replace the script URL with `https://gitee.com/nl2sh/nl2sh/raw/master/install-android.sh` (or `.ps1` / `.bat`) and pass `--repository https://gitee.com/nl2sh/nl2sh` (`-Repository` in PowerShell). Release assets must also be mirrored; source alone is insufficient. There is no implicit fallback after GitHub fails.

## Updates

Direct Android builds support `nl2sh update` or TUI `/update`: download the ABI-specific executable, verify SHA-256, and atomically replace it. Restart to use the new program. Launchers deploy their local package according to its digest, so download the updated host package too; launching an old package can overwrite the device update. Termux package-manager builds use `pkg upgrade nl2sh`.

Continue with [daily startup](android-adb.md), [Termux](termux.md), or [source builds](../development/build.md).

x86_64 devices and emulators require a new release archive containing that ABI. For an existing two-ABI installation, back up configuration and extract the new archive into a new directory. Launchers prefer native x86_64 without relying on ARM translation. Self-update uses `nl2sh-android-x86_64` and its `.sha256`; Termux packages continue to use package-manager updates.

When reusing an older directory without `bin/x86_64/nl2sh`, installers issue a warning while allowing existing ARM installations to continue. For x86_64, back up `config.toml` and select a new directory with Bash/CMD `--install-dir` or PowerShell `-InstallDir`, using a release that contains x86_64. Script help lists all three ABIs and the native x86_64 preference.
