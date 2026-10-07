# Release 与文档发布

## Android 发布

`.github/workflows/release.yml` 在 `v*` tag 推送时用 NDK r28c/API 26 构建 AArch64/ARMv7/x86_64、自更新裸程序和摘要、统一 ZIP/TAR、Termux deb 与签名 APT 快照，再发布 GitHub Release。手动 workflow_dispatch 创建草稿；草稿不更新线上站点。

本地 `pack-release.sh` / `pack-release.ps1` 创建同布局 ZIP。包内双语 README 链接到正式网站，并携带双语入门 Markdown 与对应图片；不再依赖已删除的使用说明.md。Termux deb 携带两种语言入口，更新由包管理器负责。

发布前核对版本、两种语言 changelog、配置例子、工具参考、脚本、TUR 版本/源码 SHA-256 及依赖许可证；执行 Rust/前端/脚本/文档门禁与 Android ABI 验证。tag 不代表工作流成功，以最终 job 和实际资产为准。

## 一个 GitHub Pages 站点

文档与自建 Termux APT 共用 `https://nl2sh.github.io/nl2sh/`。APT `dists/`、`pool/`、`nl2sh-repo.gpg` 根路径固定，不能用纯文档产物覆盖。

Release 发布签名 `termux-apt-repository.tar.gz` 资产。文档工作流对 PR 只检查，对 master、手动触发或成功 Release 完成后部署：构建双语文档 → 获取最新正式 Release APT 快照 → 验签与 SHA-256 检查 → 合并 → 上传 Pages artifact → deploy-pages。旧 Release 尚无快照资产时，从已有站点获取并验证同一签名仓库；失败停止部署，不发布缺 APT 的产物。发布任务共享并发组，避免相互覆盖。

配置 Pages Source 为 GitHub Actions；github-pages 环境允许 master。现有 APT 私钥 Secret `TERMUX_APT_GPG_PRIVATE_KEY` 只由 Release 签名任务读取，文档构建不读取。公钥指纹改变需显式同步信任配置，不跳过验证。

HTML 只存在构建产物，site/ 不提交到 master。参考 [GitHub Pages 官方工作流](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages)。

### 签名兼容性与资产

`packaging/runtime-components.json` 指定独立 Bridge/JADX 发布及最低助手版本，先发布这些 companion 版本。发布任务获取精确资产，检查包内元数据与 APK 签名，然后签名 `nl2sh-runtime-extensions.json`，再让三个 ABI 的原生构建内嵌它。最终 `nl2sh-runtime.json` 补入实际原生大小和摘要。两个 Manifest 和每个原生/Bridge/JADX 资产都有二进制 SHA-256 独立 GPG 签名。标签必须与 Cargo 版本一致。

签名复用 github-pages 环境中的 `TERMUX_APT_GPG_PRIVATE_KEY`，固定公钥指纹为 `5230D3A7CCBEED4616D39C51FC6AD1BC63F7D4D8`。Bridge 使用其已有 APK keystore secrets。伴侣仓库私有时，`NL2SH_COMPONENTS_TOKEN` 需允许读取对应 Release；否则使用仓库 token。缺失资产、密钥、协议不匹配或 APK 签名无效都会使发布失败。本地可暂缓生产签名；未签名原生源码构建需显式配置离线或自定义 JADX。

原生程序和助手先验证签名 Manifest，再使用其中资产 URL，安装前验证大小、SHA-256 和资产签名。新安装或升级不接受现代未签名发布；连接已安装的健康运行时仍无需检查发布。
