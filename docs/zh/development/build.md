# 构建与 Android 验证

## 主机开发

需要 stable Rust（edition 2021）、Node.js 22+ / npm、Unix 开发环境。Cargo 复制 Web 源码到 OUT_DIR，在副本 npm ci/build 并嵌入资源；设备运行不需要 Node.js。HTTP 仅 rustls。

```bash
cargo fmt --all -- --check
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

## Android 交叉编译

主机安装 NDK r28c 或兼容版本与 Rust targets：

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi
export ANDROID_NDK_HOME=/path/to/android-ndk
./cross-compile.sh
RUST_TARGET=armv7-linux-androideabi ./cross-compile.sh
./android-build-run.sh
```

默认 API 26，`ANDROID_API_LEVEL` 可覆盖但不应低于项目支持下限。Windows 使用原生 NDK Windows 工具链和 `android-build-run.ps1`，无需 Bash/WSL：

```powershell
$env:ANDROID_NDK_HOME = 'C:/Android/Sdk/ndk/28.2.13676358'
./android-build-run.ps1
```

脚本按设备 ABI 选择目标，显式目标不匹配时拒绝。设备不需要 Termux、Bash 或 GNU 工具。不要把本地 SDK 路径提交到项目。

## Termux 打包

`./pack-termux-release.sh` 输出 aarch64/arm deb。Windows `pack-termux-release.ps1` 用 Windows NDK 编译、WSL dpkg-deb 封包；WSL 不重复编译。包管理构建 `NL2SH_PACKAGE_MANAGER_BUILD=1` 禁用 self-update。

开发部署 `android-build-tmux-run.sh` 前，在 Termux 配置 openssh/tmux、passwd、sshd；脚本按 ABI 构建并 SSH/tmux 安装，变量见 [环境参考](../reference/environment-variables.md)。

## 真机验收

验证启动/退出终端恢复、只读 id/getprop、修改确认、危险拒绝、normal/auto/root、超时/Ctrl+C、全屏程序返回与 resize，以及两种模型协议。交叉编译成功不等于真机通过；验证记录写明目标平台与命令。
