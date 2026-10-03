# 安装与更新

| 使用环境 | 安装方式 | 所需条件 |
| --- | --- | --- |
| 电脑连接 Android | Release ZIP + ADB 启动器 | 主机 adb，Android API 26+，ARM64/ARMv7 |
| 电脑一键安装 | Bash / PowerShell / CMD 安装脚本 | 主机联网及 adb |
| Android Termux | TUR 或签名 APT / 本地 deb | Termux 包管理器 |
| 开发者 | 源码交叉编译 | stable Rust、Node.js 22+、Android NDK |

## Release 安装

从 [GitHub Releases](https://github.com/nl2sh/nl2sh/releases/latest) 下载 `nl2sh-android.zip` 与 `SHA256SUMS`，核对 ZIP 的 SHA-256 后完整解压。包同时包含 `bin/arm64-v8a/nl2sh` 与 `bin/armeabi-v7a/nl2sh`，启动器自动选择 ABI；不要只拿桌面 Linux 二进制推送到 Android。

```bash
sha256sum nl2sh-android.zip
cd nl2sh-android
./android-run-linux.sh
```

Windows 可用 `Get-FileHash ./nl2sh-android.zip -Algorithm SHA256` 比较摘要，解压后运行 `android-run-windows.bat`。设备准备见 [ADB / 原生 Android](android-adb.md)。

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

直接 Android 版本可使用 `nl2sh update` 或 TUI `/update`，按 ABI 下载裸二进制、核对 SHA-256 后原子替换；下次启动使用新程序。安装目录中的启动器仍会依据本地包摘要部署本地版本，因此要同步下载新版包，避免再次启动旧包覆盖设备更新。Termux 包管理构建使用 `pkg upgrade nl2sh`。

继续阅读：[日常启动](android-adb.md)、[Termux](termux.md)、[源码构建](../development/build.md)。
