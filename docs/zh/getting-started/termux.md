# Termux

## TUR 安装

```bash
pkg install tur-repo
pkg install nl2sh
nl2sh --version
nl2sh
```

首次进入 TUI 用 `/config` 配置模型。Termux 是兼容环境，程序识别 `TERMUX_VERSION` 或标准 `PREFIX` 后使用 `$PREFIX/bin/sh` 与 XDG 路径；不会假定可选软件已安装。

## 自建签名 APT

独立自建渠道仅发布 `aarch64` 与 `arm`。添加源：

```bash
mkdir -p "$PREFIX/etc/apt/keyrings"
curl -fsSL https://nl2sh.github.io/nl2sh/nl2sh-repo.gpg   -o "$PREFIX/etc/apt/keyrings/nl2sh.gpg"
echo "deb [signed-by=$PREFIX/etc/apt/keyrings/nl2sh.gpg] https://nl2sh.github.io/nl2sh stable main"   > "$PREFIX/etc/apt/sources.list.d/nl2sh.list"
pkg update
pkg install nl2sh
```

仓库公钥指纹为 `5230 D3A7 CCBE ED46 16D3 9C51 FC6A D1BC 63F7 D4D8`。文档站与 APT 共用站点，软件源根地址保持不变。

## 本地 deb

从 Release 下载匹配包，用 `dpkg --print-architecture` 核对，执行 `apt install ./nl2sh_版本_aarch64.deb` 或 `_arm.deb`。每次只选一个版本。桌面 Linux 程序与 Android deb 不可互换；当前自建 Release 不发布 x86 包。

## 配置、更新与卸载

默认配置是 `~/.config/nl2sh/config.toml`，状态是 `~/.local/state/nl2sh`；`XDG_CONFIG_HOME`、`XDG_STATE_HOME` 可改变基础路径。显式配置路径的状态跟随该配置目录。

```bash
pkg update
pkg upgrade nl2sh
# 本地 deb 更新：再次 apt install 新包
# 卸载程序，用户状态不会自动删除
apt remove nl2sh
```

包管理构建禁用程序内自更新。Root 不是安装前提，但普通 Termux UID 不能使用 shell/root 专属的 UI 自动化权限；详见 [权限排查](../troubleshooting/permissions.md)。构建 deb 见 [开发构建](../development/build.md)。
