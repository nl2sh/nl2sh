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
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
export ANDROID_NDK_HOME=/path/to/android-ndk
./cross-compile.sh
RUST_TARGET=armv7-linux-androideabi ./cross-compile.sh
RUST_TARGET=x86_64-linux-android ./cross-compile.sh
./android-build-run.sh
```

默认 API 26，`ANDROID_API_LEVEL` 可覆盖但不应低于项目支持下限。Windows 使用原生 NDK Windows 工具链和 `android-build-run.ps1`，无需 Bash/WSL：

```powershell
$env:ANDROID_NDK_HOME = 'C:/Android/Sdk/ndk/28.2.13676358'
./android-build-run.ps1
```

脚本按设备 ABI 选择目标，显式目标不匹配时拒绝。设备不需要 Termux、Bash 或 GNU 工具。不要把本地 SDK 路径提交到项目。

源码构建脚本和预编译运行启动器自动选择唯一处于 `device` 状态的 ADB 设备，包括序列号含空格的无线调试 mDNS 设备；多个设备时提示选择，仅没有可用设备时要求输入 IP。也可通过 `$env:ADB_SERIAL` 指定完整序列号。

支持的映射如下；x86_64 设备优先使用原生程序，即使 ABI 列表也含 ARM 转译支持。统一打包脚本 `pack-release.sh` / `pack-release.ps1` 构建三个 ABI。

| Android ABI | Rust target | Termux |
| --- | --- | --- |
| `arm64-v8a` | `aarch64-linux-android` | `aarch64` |
| `armeabi-v7a` | `armv7-linux-androideabi` | `arm` |
| `x86_64` | `x86_64-linux-android` | `x86_64` |

## Termux 打包

`./pack-termux-release.sh` 输出 aarch64/arm/x86_64 deb。Windows `pack-termux-release.ps1` 用 Windows NDK 编译、WSL dpkg-deb 封包；WSL 不重复编译。包管理构建 `NL2SH_PACKAGE_MANAGER_BUILD=1` 禁用 self-update。

开发部署 `android-build-tmux-run.sh` 前，在 Termux 配置 openssh/tmux、passwd、sshd；脚本按 ABI 构建并 SSH/tmux 安装，变量见 [环境参考](../reference/environment-variables.md)。

## 真机验收

验证启动/退出终端恢复、只读 id/getprop、修改确认、危险拒绝、normal/auto/root、超时/Ctrl+C、全屏程序返回与 resize，以及两种模型协议。交叉编译成功不等于真机通过；验证记录写明目标平台与命令。


## 托管后台 Shell 回归

`examples/background_shell_device_check.rs` 是无模型的交互式驱动，使用生产 Tool Runtime 和 `StdioConfirmer`，只接受 execute_shell_command/read_output/kill。按目标 ABI 设置与交叉编译相同的 NDK linker/CC/AR 后构建该 example，在设备真实终端逐行输入 `{"tool":"execute_shell_command","arguments":{"command":"logcat -v threadtime -s nl2sh_background_regression:I","background":true,"background_timeout_secs":60}}`。保存返回 child_id，再读取和停止；修改/危险审批仍由本地终端逐次决定。输入 `exit` 清理本驱动所有后台命令，不替换现有服务。回归覆盖独立双流偏移、16 个活动句柄上限、重复启停的 fd/进程回收、TERM 抵抗/超时及正常退出；不要在异常父进程死亡、OOM 或逃逸进程组尚未验证时宣称持久守护能力。
