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
rustup target add aarch64-linux-android armv7-linux-androideabi
export ANDROID_NDK_HOME=/path/to/android-ndk
./cross-compile.sh
RUST_TARGET=armv7-linux-androideabi ./cross-compile.sh
./android-build-run.sh
```

API 26 defaults; `ANDROID_API_LEVEL` can override it without going below the project minimum. Windows uses native Windows NDK with `android-build-run.ps1`, without Bash/WSL:

```powershell
$env:ANDROID_NDK_HOME = 'C:/Android/Sdk/ndk/28.2.13676358'
./android-build-run.ps1
```

Launchers choose by device ABI and reject conflicting explicit targets. Devices need no Termux, Bash, or GNU utilities. Never commit local SDK paths.

## Termux packaging

`./pack-termux-release.sh` emits aarch64/arm debs. Windows `pack-termux-release.ps1` compiles with Windows NDK and packages with WSL dpkg-deb, without recompiling there. `NL2SH_PACKAGE_MANAGER_BUILD=1` disables self-update.

For `android-build-tmux-run.sh`, prepare Termux openssh/tmux, passwd, and sshd. The script builds by ABI and installs through SSH/tmux; see [environment variables](../reference/environment-variables.md).

## Device acceptance

Cover terminal startup/exit restoration, read-only id/getprop, mutation approval, rejection of dangerous actions, normal/auto/root modes, timeout/Ctrl+C, full-screen return/resize, and both model protocols. Cross-compilation does not prove device behavior; record target platforms and commands.
