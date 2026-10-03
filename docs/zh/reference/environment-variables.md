# 环境变量

设备运行、主机安装、构建与网关是不同进程；在正确进程环境设置变量。设备不会自动继承电脑变量。密钥不要提交到 Git。

| 变量 | 用途 |
| --- | --- |
| `NL2SH_CONFIG` | 默认配置路径，CLI --config 优先 |
| `NL2SH_API_KEY` | 覆盖主模型 Key |
| `NL2SH_IMA_CLIENT_ID / NL2SH_IMA_API_KEY` | 独立 ima 凭据，齐全时启用 |
| `NL2SH_JEV_API_KEY / NL2SH_JEV_ENDPOINT / NL2SH_JEV_MODEL` | 覆盖音频质量模型设置 |
| `NL2SH_JADX_ANDROID_HELPER_PATH` | 离线 DEX helper 路径 |
| `NL2SH_JADX_ANDROID_HELPER_URL / NL2SH_JADX_ANDROID_HELPER_SHA256` | HTTPS 来源与独立摘要，必须成对 |
| `NL2SH_JADX_CACHE_DIR` | helper 私有缓存根目录 |
| `TERMUX_VERSION / PREFIX` | Termux 检测及 shell 路径 |
| `HOME / XDG_CONFIG_HOME / XDG_STATE_HOME` | Termux 默认 config/state 基础目录、~ 引用 |
| `NL2SH_WINDOWS_SCROLL` | Windows ADB 滚轮兼容标志 |
| `TERM / COLORTERM` | 终端能力和色彩检测 |
| `ADB_SERIAL / ANDROID_DIR / NL2SH_CONFIG_SOURCE` | 主机启动器选择设备、目录与显式配置部署 |
| `ANDROID_NDK_HOME / ANDROID_NDK_ROOT` | 主机构建 NDK 路径 |
| `ANDROID_API_LEVEL / RUST_TARGET` | 交叉编译 API（默认26）与目标架构 |
| `ANDROID_HOME / ANDROID_SDK_ROOT / JAVA_HOME` | 可选 Android 模块的 SDK / JDK |
| `NL2SH_PACKAGE_MANAGER_BUILD` | 构建期 1 表示禁用应用内自更新 |
| `ANDROID_TMP_DIR / TERMUX_SSH_LOCAL_PORT / TERMUX_SSH_REMOTE_PORT / TERMUX_TMUX_SESSION` | Termux SSH/tmux 开发部署脚本 |
| `NL2SH_A2A_TOKEN / NL2SH_A2A_URL / NL2SH_A2A_ALLOW_INSECURE_HTTP` | 网关令牌、客户端来源、远程 HTTP 显式选择 |
| `NL2SH_DEVICE_SERIAL / NL2SH_DEVICE_BINARY / NL2SH_DEVICE_CONFIG` | Compose 的 Android 设备、程序与配置 |
| `NL2SH_GATEWAY_BIND / NL2SH_GATEWAY_PORT / NL2SH_GATEWAY_URL / NL2SH_GATEWAY_BASE_IMAGE` | Compose 主机监听与公告地址、基础镜像 |
| `NL2SH_APT_GPG_KEY_ID / TERMUX_APT_GPG_PRIVATE_KEY` | APT 签名 key ID / CI Secret，私钥不进入源码 |
| `NL2SH_WEB_DIST` | Cargo build.rs 生成的嵌入资源目录，非用户设置 |
| `NL2SH_TAILCAT_TEST_BINARY / NL2SH_TAILCAT_TEST_PROXY` | 显式 Tailcat live 测试 |
| `TAILCAT_HOST_BIN / TAILCAT_DEVICE_BIN` | 双向传输测试程序路径 |

[网关细节](../advanced/a2a-mcp.md)与[配置优先级](configuration.md)。HTTP_PROXY / HTTPS_PROXY / ALL_PROXY 对主机下载工具的作用不等于 nl2sh 设备代理配置。
