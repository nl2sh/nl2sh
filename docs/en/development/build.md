# Builds and Android validation

## Host development

Use stable Rust (edition 2021), Node.js 22+ / npm, and a Unix development environment. Cargo copies Web sources to OUT_DIR, runs npm ci/build there, and embeds assets. Device runtime needs no Node.js. HTTP uses rustls only.

```bash
cargo fmt --all -- --check
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

## Android cross-compilation

Install NDK r28c or a compatible version and Rust targets on the host:

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
export ANDROID_NDK_HOME=/path/to/android-ndk
./cross-compile.sh
RUST_TARGET=armv7-linux-androideabi ./cross-compile.sh
RUST_TARGET=x86_64-linux-android ./cross-compile.sh
./android-build-run.sh
```

API 26 defaults; `ANDROID_API_LEVEL` can override it without going below the project minimum. Windows uses native Windows NDK with `android-build-run.ps1`, without Bash/WSL:

```powershell
$env:ANDROID_NDK_HOME = 'C:/Android/Sdk/ndk/28.2.13676358'
./android-build-run.ps1
```

Launchers choose by device ABI and reject conflicting explicit targets. Devices need no Termux, Bash, or GNU utilities. Never commit local SDK paths.

Source build scripts and precompiled launchers automatically select the only ADB device in the `device` state, including wireless debugging mDNS serials containing spaces. It prompts for a selection when multiple devices are available and requests an IP only when no usable device is connected. Set `$env:ADB_SERIAL` to select a complete serial explicitly.

Supported mappings are below. x86_64 devices select the native binary even when ARM translation ABIs are advertised. `pack-release.sh` / `pack-release.ps1` build all three ABIs.

| Android ABI | Rust target | Termux |
| --- | --- | --- |
| `arm64-v8a` | `aarch64-linux-android` | `aarch64` |
| `armeabi-v7a` | `armv7-linux-androideabi` | `arm` |
| `x86_64` | `x86_64-linux-android` | `x86_64` |

## Termux packaging

`./pack-termux-release.sh` emits aarch64/arm/x86_64 debs. Windows `pack-termux-release.ps1` compiles with Windows NDK and packages with WSL dpkg-deb, without recompiling there. `NL2SH_PACKAGE_MANAGER_BUILD=1` disables self-update.

For `android-build-tmux-run.sh`, prepare Termux openssh/tmux, passwd, and sshd. The script builds by ABI and installs through SSH/tmux; see [environment variables](../reference/environment-variables.md).

## Device acceptance

Cover terminal startup/exit restoration, read-only id/getprop, mutation approval, rejection of dangerous actions, normal/auto/root modes, timeout/Ctrl+C, full-screen return/resize, and both model protocols. Cross-compilation does not prove device behavior; record target platforms and commands.


## Managed background shell regression

`examples/background_shell_device_check.rs` is an interactive driver without a model. It uses the production Tool Runtime and `StdioConfirmer` and accepts only execute_shell_command/read_output/kill. Configure the same NDK linker/CC/AR used for cross-compilation and build this example for the device ABI. In a real device terminal, submit one line such as `{"tool":"execute_shell_command","arguments":{"command":"logcat -v threadtime -s nl2sh_background_regression:I","background":true,"background_timeout_secs":60}}`. Save the returned child_id, then read and stop it. Each required mutation/high-risk approval remains a local terminal decision. Enter `exit` to clean all children owned by this driver, without replacing an existing service. Regression cases cover independent stream offsets, the 16-active-handle limit, fd/process cleanup across repeated starts/stops, TERM resistance/timeouts and normal exit. Do not claim persistent daemon behavior from these tests: abnormal parent death, OOM and escaped process groups remain separate validation boundaries.
