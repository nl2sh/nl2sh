# 安装与更新

| 使用环境 | 安装方式 | 所需条件 |
| --- | --- | --- |
| 电脑连接 Android | Release ZIP + ADB 启动器 | 主机 adb，Android API 26+，ARM64/ARMv7/x86_64 |
| Android 控制端连接目标设备 | nl2sh-helper APK | 控制端 Android API 26+；目标设备 ARM64/ARMv7、可用的 TCP ADB 或 Android 11+ 无线调试 |
| 电脑一键安装 | Bash / PowerShell / CMD 安装脚本 | 主机联网及 adb |
| Android Termux | TUR 或签名 APT / 本地 deb | Termux 包管理器 |
| 开发者 | 源码交叉编译 | stable Rust、Node.js 22+、Android NDK |

## Release 安装

从 [GitHub Releases](https://github.com/nl2sh/nl2sh/releases/latest) 下载 `nl2sh-android.zip` 与 `SHA256SUMS`，核对 ZIP 的 SHA-256 后完整解压。包同时包含 `bin/arm64-v8a/nl2sh` 、`bin/armeabi-v7a/nl2sh` 与 `bin/x86_64/nl2sh`，启动器自动选择 ABI；不要只拿桌面 Linux 二进制推送到 Android。

```bash
sha256sum nl2sh-android.zip
cd nl2sh-android
./android-run-linux.sh
```

Windows 可用 `Get-FileHash ./nl2sh-android.zip -Algorithm SHA256` 比较摘要，解压后运行 `android-run-windows.bat`。设备准备见 [ADB / 原生 Android](android-adb.md)。

## nl2sh-helper

如果用另一台 Android 设备作控制端，可从需仓库访问权限的 [nl2sh-helper Releases](https://github.com/nl2sh/nl2sh-helper/releases/latest) 下载已签名 APK，并安装在控制端。打开助手后，选择 TCP ADB（目标设备已开启 TCP ADB），或选择配对码/二维码连接目标 Android 11+ 的无线调试。首次 TCP 连接需在目标设备批准 ADB 授权；配对码方式按目标设备显示的临时地址、端口和配对码填写。

连接会复用健康的已安装运行时，不检查发布或重启。首次安装及显式更新从认证的发布 Manifest 选择 ARM64、ARMv7 或 x86_64，验证原生资产大小、摘要与签名，部署到 `/data/local/tmp/nl2sh`。原生 service 返回实际 Web 端口；更新、重启、停止是独立动作，更新失败恢复先前已验证程序与归属。打开返回的浏览器地址，在 Web“快速开始”配置模型凭据。控制端须能访问实际端口；Web 无登录且监听所有 IPv4 接口，仅在可信网络使用。详见[助手指南](https://github.com/nl2sh/nl2sh-helper/blob/main/docs/zh/guide.md)。

## 一键安装

API Key 通过环境变量传入；请在自己的私有终端中填写真实值，不要提交到仓库。

=== "Linux"

    ```bash
    export NL2SH_API_KEY='你的密钥'
    curl -fsSL https://raw.githubusercontent.com/nl2sh/nl2sh/master/install-android.sh \
      | bash -s -- --provider deepseek --model deepseek-flash
    ```

=== "PowerShell"

    ```powershell
    $env:NL2SH_API_KEY = '你的密钥'
    & ([scriptblock]::Create((irm https://raw.githubusercontent.com/nl2sh/nl2sh/master/install-android.ps1))) `
      -Provider deepseek -Model deepseek-flash
    ```

=== "CMD"

    ```bat
    set "NL2SH_API_KEY=你的密钥"
    curl.exe -fsSL https://raw.githubusercontent.com/nl2sh/nl2sh/master/install-android.bat -o "%TEMP%\install-nl2sh.bat"
    call "%TEMP%\install-nl2sh.bat" --provider deepseek --model deepseek-flash
    ```

安装器下载同一 Release 的 ZIP 与校验文件。默认目录为当前目录下的 `nl2sh-android`；再次运行会复用完整目录，显式 Provider/模型/Endpoint 参数只更新相应设置，不完整目录会拒绝覆盖。Linux 管道安装会在启动时重新连接 controlling terminal；没有交互终端时，稍后手动运行启动器。

支持 `openrouter`、`openai`、`deepseek`、`moonshot`/`kimi`、`siliconflow`、`ollama`、`custom`；自定义服务需提供 `--endpoint` / `-Endpoint`。

## Gitee 镜像

可显式选择 [Gitee](https://gitee.com/nl2sh/nl2sh)：把上述脚本 URL 换为 `https://gitee.com/nl2sh/nl2sh/raw/master/install-android.sh`（或 `.ps1` / `.bat`），并传入 `--repository https://gitee.com/nl2sh/nl2sh`（PowerShell 用 `-Repository`）。该来源需同步 Release 资产，仅有源码不足以安装；不会在 GitHub 失败后隐式切换来源。

## 更新

直接 Android 版本可使用 `nl2sh update` 或 TUI `/update`，按 ABI 下载裸二进制、校验签名兼容性 Manifest、精确大小、SHA-256、独立 GPG 签名与 ELF ABI 后原子替换；下次启动使用新程序。安装目录中的启动器仍会依据本地包摘要部署本地版本，因此要同步下载新版包，避免再次启动旧包覆盖设备更新。Termux 包管理构建使用 `pkg upgrade nl2sh`。

继续阅读：[日常启动](android-adb.md)、[Termux](termux.md)、[源码构建](../development/build.md)。

x86_64 设备与模拟器需要包含该 ABI 的新版发布包；旧的双 ABI 安装目录需先备份配置，再解压新版包到新目录。启动器优先选择原生 x86_64，不依赖 ARM 转译。内置自更新使用 `nl2sh-android-x86_64` 和对应 `.sha256`；Termux 包仍通过包管理器更新。

安装器复用旧目录时，若缺少 `bin/x86_64/nl2sh` 会明确警告；旧 ARM 安装仍可继续使用。x86_64 用户请备份 `config.toml`，用 Bash/CMD 的 `--install-dir` 或 PowerShell 的 `-InstallDir` 指向新目录，选择包含 x86_64 的发行包。脚本帮助已列明三种 ABI 与原生 x86_64 优先策略。

`/api/info` 返回安装更新归属和内嵌运行时策略状态：已验证、未签名源码构建没有策略、或无效。助手管理的安装应通过助手更新；归属记录损坏时自更新被阻止，需先检查记录。
