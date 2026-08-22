<p align="center">
  <img src="assets/logo.png" alt="nl2sh logo" width="220">
</p>

<h1 align="center">nl2sh</h1>

Natural Language to Shell 是面向 Android 原生 `adb shell` 的类 Hermes AI Agent。核心程序以单个 Android 可执行文件交付，无需 Termux 或设备端运行时依赖；同时提供丰富的 TUI，用于多轮对话、实时命令输出、安全确认、历史浏览和配置。它把自然语言交给 OpenAI 兼容模型，通过 Tool Calling 生成命令，在本地安全分类和确认之后执行，并把真实结果返回模型。

“类 Hermes”指的是自主 Agent 的产品形态和 Tool Calling 交互方式；nl2sh 专注 Android shell，不声称与 Hermes 的 API、插件或全部功能兼容。

## 特性与安全边界

- 默认使用多轮 Agent Tool Calling；也支持只生成单条命令的 Command 模式。
- 核心程序是单文件 Android 可执行程序，可直接推送到设备运行。
- 丰富 TUI 支持实时状态与输出、内嵌确认、历史滚动、工具结果折叠、Markdown 渲染、中英文界面和热重配置。
- 支持 Chat Completions 与 Responses API、自定义 OpenAI 兼容 endpoint。
- `balanced` 默认策略自动执行只读查询、确认修改操作、二次确认危险操作。
- LLM 不能决定确认、风险等级、root 提升或超时；用户编辑后的命令必须重新分类。
- 支持当前用户、自动提升和强制 root 模式。非 root 提升使用参数化的 `su -c <command>`，不拼接 shell 字符串。
- crossterm + ratatui 终端界面通过 RAII 和 panic hook 恢复 raw mode、alternate screen、鼠标捕获和光标；滚轮浏览历史，Shift+拖选后可使用宿主终端的右键菜单复制。
- 主要目标为 Android API 26+、`aarch64-linux-android` 和 `/data/local/tmp`。不依赖或专门支持 Termux。

默认执行路径使用 Unix `openpty`：slave 成为子进程 controlling terminal，master 非阻塞读取，stdout/stderr 合并，超时会清理整个进程组。识别到全屏/交互命令时，nl2sh 临时使用本地 raw mode，桥接 stdin/master 输出并同步窗口尺寸，结束后恢复终端。不同 Android 终端和全屏应用仍可能存在兼容差异，详见 `PROJECT_STATUS.md`。

## 构建

需要 stable Rust（edition 2021）。桌面 Unix 环境用于开发验证：

```bash
cargo build
cargo test
cargo build --release
```

HTTP 使用 rustls，未启用 native-tls。

项目使用 MIT License，详见 `LICENSE`。

## Android 交叉编译和部署

安装目标并配置 Android NDK：

```bash
rustup target add aarch64-linux-android
export ANDROID_NDK_HOME=/path/to/android-ndk
./cross-compile.sh
adb push target/aarch64-linux-android/release/nl2sh /data/local/tmp/
adb shell chmod +x /data/local/tmp/nl2sh
adb shell -t /data/local/tmp/nl2sh
```

脚本默认 API 26 和 `aarch64-linux-android`，可通过 `ANDROID_API_LEVEL` 与 `RUST_TARGET=armv7-linux-androideabi` 覆盖。也接受 `ANDROID_NDK_ROOT`。它不要求 Termux、bash 位于 Android 设备上或 GNU coreutils。

也可以一键完成 release 交叉编译、推送、授权并进入带 TTY 的 adb shell 启动应用：

```bash
export ANDROID_NDK_HOME=/path/to/android-ndk
./android-run.sh
```

Windows PowerShell 可使用原生 NDK Windows 工具链执行同一流程，不需要 Bash 或 WSL：

```powershell
$env:ANDROID_NDK_HOME = "C:\Android\Sdk\ndk\28.2.13676358"
.\android-run.ps1
```

默认推送到 `/data/local/tmp/nl2sh`。可用 `ANDROID_DIR=/data/local/tmp/tools` 修改设备目录，多设备时用 `ADB_SERIAL=<serial>` 指定设备，ARMv7 设备可同时设置 `RUST_TARGET=armv7-linux-androideabi`。脚本要求主机 `PATH` 中可找到 `adb`，设备端仅使用 Android 自带的 `mkdir`、`chmod` 和 shell。连接后会先执行 `adb root`、等待 adbd 重启并验证 `id -u`；root adbd 成功时，后续推送和启动均以 root 进行。设备不支持 `adb root` 时才尝试 `su -c`，两者都不可用且已有 `0600 config.toml` 不可读时会提前报错，不会放宽 API Key 配置文件权限。

已有预编译的 Android `nl2sh` 时，可把它与对应脚本放在同一目录，直接从推送步骤开始，无需 Rust 或 NDK。Linux 使用 `android-run-linux.sh`，Windows PowerShell 使用 `android-run-windows.ps1`；两者同样支持 `ANDROID_DIR` 和 `ADB_SERIAL`：

```bash
chmod +x android-run-linux.sh nl2sh
./android-run-linux.sh
```

```powershell
$env:ADB_SERIAL = "device-serial"
.\android-run-windows.ps1
```

### 通过 GitHub Actions 自动发布

仓库内置 `.github/workflows/release.yml`。推送 `v*` tag（如 `git tag v0.2.0 && git push origin v0.2.0`）会触发 GitHub Actions：并行交叉编译 `aarch64-linux-android`（arm64-v8a）与 `armv7-linux-androideabi`（armeabi-v7a）两个 release 产物，每个产物连同 Linux/Windows 启动脚本、`config.toml.example` 和面向普通用户的 `使用说明.md` 打包成 `.tar.gz` 与 `.zip`，并附带 `SHA256SUMS` 发布到对应 tag 的 GitHub Release。Actions 页的 `workflow_dispatch` 可手动触发并生成草稿 Release（tag 通过输入指定）。

下载适合设备 ABI 的包解压后，Linux/macOS 直接运行 `./android-run-linux.sh`，Windows PowerShell 运行 `.\android-run-windows.ps1`；脚本会查找同目录的预编译 `nl2sh` 并完成 root adbd/`su` 回退部署，无需 Rust 或 NDK。

### Android 提示 `No such file or directory`

如果 `/data/local/tmp/nl2sh` 明明存在且已有执行权限，但运行时仍提示：

```text
/system/bin/sh: ./nl2sh: No such file or directory
```

这通常不是文件路径不存在，而是二进制 ABI 与设备不匹配，导致 Android 找不到 ELF 指定的动态加载器。例如，只支持 `armeabi-v7a` 的 32 位设备不能运行默认生成的 `aarch64-linux-android` 64 位程序；该程序请求 `/system/bin/linker64`，而 32 位设备只有 `/system/bin/linker`。

先检查设备 ABI 和远端二进制：

```powershell
adb shell getprop ro.product.cpu.abi
adb shell getprop ro.product.cpu.abilist
adb shell file /data/local/tmp/nl2sh
```

- `arm64-v8a`：使用默认的 `aarch64-linux-android`。
- `armeabi-v7a` 且 ABI 列表中没有 `arm64-v8a`：必须构建 `armv7-linux-androideabi`。

Windows PowerShell 构建并部署 ARMv7 版本：

```powershell
rustup target add armv7-linux-androideabi
$env:RUST_TARGET = "armv7-linux-androideabi"
.\android-run.ps1
```

Linux/macOS 使用相同目标：

```bash
rustup target add armv7-linux-androideabi
RUST_TARGET=armv7-linux-androideabi ./android-run.sh
```

重新推送后，`adb shell file /data/local/tmp/nl2sh` 在 ARMv7 设备上应显示 `ELF 32-bit`、`ARM` 和 `/system/bin/linker`，不应显示 `64-bit arm64` 或 `/system/bin/linker64`。如果 ABI 已匹配，再检查文件权限、ELF interpreter 是否存在，以及二进制是否确实由 Android NDK 而非桌面工具链构建。

## 配置

默认配置位于解析符号链接后的可执行文件目录，名称为 `config.toml`。配置不存在且以 TUI 启动时，程序会用 2 秒超时探测 KONKA 内部便捷体验服务；若收到任意 HTTP 响应，则以 `0600` 权限自动创建使用 `deepseek-v4-flash-0731` 模型和 Responses 协议的 `KK-FREE-TEST` 配置。已有配置文件绝不参与探测或改写；内网服务不可达时仍直接进入 TUI，不自动启动向导，可使用 `/config` 完成全部配置，或分别用 `/provider` 配置 Endpoint、API Key、API 类型并用 `/model` 配置模型。配置完成前普通任务不会发送给模型。`nl2sh --init` 仍可显式创建配置且不会覆盖已有文件。请注意终端屏幕和录屏中可能保留输入的 Key。也可传入 `--config /path/config.toml`。

```bash
cp config.toml.example config.toml
```

配置优先级为 CLI 参数、`NL2SH_API_KEY`、`config.toml`、字段默认值。CLI 可用 `--endpoint`、`--model`、`--api-type` 覆盖 provider 设置；覆盖后统一校验，因此可以修正文件中的对应无效值。空 API Key 适用于本地服务，此时不会发送空 Authorization header。不要提交真实 key。

`api_type` 可选 `responses` 或 `chat_completions`。兼容服务对协议的支持并不一致，nl2sh 不会因一次业务错误擅自切换协议。

`execute_user_mode`：

- `auto`：UID 0 直接运行；普通命令保持当前用户；明确需要 root 时才尝试 `su -c`。
- `normal`：永不自动调用 `su`。
- `root`：UID 非 0 时必须通过 `su`，失败时不静默降级。

`history_log_file` 默认为 `nl2sh.log`，相对路径按 `config.toml` 所在目录解析。日志采用逐行 JSON，记录用户输入、命令、输出、结果和错误并在每条记录后刷新；新文件权限为 `0600`。日志可能包含命令输出中的设备信息，排查完成后应按实际保密要求保管或清理，但不会写入 API Key。\n\n输出资源默认受限：实时 TUI 为 256 KiB、单个捕获流为 1 MiB、单个发给模型的 Tool Result 为 128 KiB、单条日志事件为 256 KiB、单个日志文件为 10 MiB。对应配置项为 `ui_live_output_max_bytes`、`tool_output_max_bytes`、`model_tool_output_max_bytes`、`history_log_event_max_bytes` 和 `history_log_max_bytes`。所有内容截断都会插入 `NL2SH ... TRUNCATED` 标记；日志达到文件上限后停止追加，不会静默形成不完整记录。

`ui_language` 控制终端界面语言，可选 `zh_cn` 或 `en`，默认 `zh_cn`。显式初始化及 `/config` 完整配置会询问界面语言，之后的向导、状态栏、确认弹窗和快捷键说明使用所选语言。`/config` 配置全部模型服务字段，`/provider` 只配置 API Endpoint、API Key 和 API 类型，`/model` 只配置模型名称；服务商选择会优先定位当前 URL 对应的内置项，未知 URL 则进入自定义项。API Key 留空会保留当前配置。`/help` 显示本地帮助，`/clear` 清空当前会话的可见对话、模型上下文和输入历史但保留 JSONL 审计日志，`/exit` 安全退出。启动欢迎页与 `/help` 还会显示项目支持、在线赞赏链接和纯文本终端祝福图；TUI 不内嵌或渲染 Logo、二维码等图片。这些启动内容不会发送给模型。

每个 Android Agent 任务开始时会向 system prompt 附加一次低敏感运行环境摘要，包括 API level、ABI、`/system/bin/sh`、当前 UID 和 root/su 能力。摘要仅用于提高命令兼容性，不包含型号、序列号、Android ID、IP、账号或应用列表，也不会改变安全分类、确认和提权策略；内存、存储与网络等易变信息仍由工具按需查询。

## 使用

```bash
nl2sh                         # TUI Agent 模式
nl2sh "列出 /data 最大的十个文件" # 单次 Agent 请求
nl2sh --mode command "查看内存"   # 生成、分类并执行单条命令
nl2sh --mode command --dry-run "查看内存"
nl2sh --endpoint http://127.0.0.1:11434/v1 --model local --api-type chat_completions "查看系统"
nl2sh --no-pty --ascii
```

Command 模式生成、分类后执行单条命令；`--dry-run` 只展示。修改或危险命令在无 TTY 时会拒绝，不能用管道伪造确认。需要多轮执行和结果回传时使用默认 Agent 模式。Agent TUI 在请求和捕获式执行期间保持活跃，并在界面内显示输出与确认弹窗；审批弹窗支持方向键与 Enter，也可直接使用 `1-6` 或 `y/n/a/e/i/t` 选择允许一次、当前任务允许完全相同命令、拒绝、编辑、交互执行或捕获执行。任务级允许不适用于 Root 或危险命令，不持久化且不做命令前缀匹配。命令运行时实时展示有界输出，完成后工具结果默认折叠，按 F2 可展开或收起；超出各层配置上限时，日志和模型上下文会携带明确截断标记。Agent 总结支持终端 Markdown 渲染。TUI 使用统一的现代深色语义主题，并按终端能力选择 TrueColor 或 ANSI 256 palette；普通正文保持灰白，青蓝表示交互与焦点，绿色、黄色和红色分别保留给成功、警告和错误。底部输入框使用低对比度深色背景和青蓝色闪烁光标，支持 Left/Right/Home/End/Delete 编辑及 Up/Down 调取当前会话输入历史；输入 `/` 时显示垂直命令候选，使用 Up/Down 选择、Enter 补全，当前提供 `/help`、`/clear`、`/config`、`/provider` 和 `/model`。TUI 启用鼠标追踪以稳定接收滚轮；复制屏幕文字时按住 Shift 拖选，由宿主终端高亮选区，再通过右键系统菜单复制。对话区只保留上下边框，避免选取内容混入左右边框。另支持 PageUp/PageDown 浏览历史、Enter 提交、Ctrl+C 取消当前任务（空闲时清空输入）、Ctrl+Q 安全退出。

风险等级为 `ReadOnly`、`Mutating`、`Dangerous`、`Critical`。内置检测覆盖危险删除、格式化、块设备写入、递归根权限修改、重启/关机、分区擦除和读写 remount；自定义规则使用 `[[security_rules]]` 添加，不能替换内置规则。

包版本查询（如 `pm list packages --show-versioncode`、`dumpsys package … | grep versionName`，以及只读的命令替换循环）按只读操作处理，不会因为使用 `$()` 本身反复弹出安全确认；替换内部的修改或危险命令仍会重新分类并确认。

例如，输入“查看当前内存使用情况”后，TUI 会实时显示命令输出、状态和 Agent 的最终总结：

![Querying current memory usage in the TUI](screenshots/query_memory.jpg)

## 测试和真机 smoke test

```bash
cargo fmt --all -- --check
cargo check
cargo test
cargo build --release
cargo clippy --all-targets -- -D warnings
```

Android 真机建议依次验证：启动/退出后终端恢复；`id` 和 `getprop` 只读执行；`touch` 确认；`rm -rf /` 二次确认并拒绝；normal/auto/root 三种模式；命令超时和 Ctrl+C；Chat Completions 与 Responses 各一个 endpoint。

## 已知限制

- 已通过 Android NDK r28c、API 26 的 AArch64/ARMv7 release 交叉编译，并在 API 34 ARMv7 设备完成 Agent、PTY 和 TUI 基础 smoke；root 提权及全屏程序仍待扩展验证。
- Agent TUI 在 LLM 和捕获式命令期间保持同一 frame；全屏交互程序需要临时挂起 TUI，结束后自动恢复。
- 交互 PTY 已支持双向桥接和 resize，但尚未在 Android 真机的各类全屏应用上验证。
- Responses 对话适配覆盖常见 function call 结构，不保证所有兼容厂商的扩展字段。

## 支持项目

⭐ 这个项目完全开源、单二进制、本地执行。点个 Star 或提个 Issue 已经是莫大支持。[点击支持 →](https://github.com/Ernest-su/nl2sh)

❤️ 如果 nl2sh 帮你少敲了几条 adb 命令、省下了调试 Android 设备的时间，欢迎请我喝杯咖啡 ☕

<p align="center">
  <a href="https://suqishuo.cn/uploads/wechatpay.png">
    <img src="https://suqishuo.cn/uploads/wechatpay.png" alt="微信赞赏码" width="320">
  </a>
</p>

<p align="center"><a href="https://suqishuo.cn/uploads/wechatpay.png">点击查看微信赞赏码</a></p>

## 文档

- `使用说明.md`：面向下载预编译压缩包的 Linux/Windows 用户。
- `ARCHITECTURE.md`：模块、数据流、安全、执行与扩展架构。
- `UI_DESIGN.md`：TUI 深色主题、语义颜色、组件样式、终端 fallback 与验收标准。
- `AGENTS.md`：后续 AI 维护约束。
- `PROJECT_PLAN.md`：阶段计划与实际状态。
- `PROJECT_STATUS.md`：当前验证和限制。
- `CHANGELOG.md`：用户可见变更。
