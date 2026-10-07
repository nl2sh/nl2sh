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

## Runtime 重构验收（2026-10-07）

协议、service、Helper、兼容清单/签名、Descriptor、UI Lease/审计、静态分析和 Web 拆分全部实现。本地验证记录：

| 门禁 | 结果 |
| --- | --- |
| Rust | 完整工作区测试、全部目标、格式及包管理器特性检查通过；既有忽略测试仍保留忽略 |
| Android 原生 | NDK r28c/API 26 的 ARM64、ARMv7、x86_64 release 构建通过 |
| Android 项目 | 三个项目 Gradle test/lint/debug/release 门禁通过；本地暂缓生产 APK 签名 |
| 前端/文档 | 33 项前端测试、生产构建；每种语言 49 页、严格构建及派生参考检查通过 |
| 发布工具 | 15 项打包/启动器/Pages 测试、工作流解析、实际三个 ELF/APK/JAR 清单生成通过 |
| API 26/35 模拟器 | UID 2000 协议关联/畸形请求/App UID 拒绝、八项静态工具、跨进程 UI 排他/独立读取/释放恢复、service 幂等/重启/停止/严格端口冲突/实际回退端口及嵌入 HTTP 通过 |
| 两个模拟器的 Helper | 健康重连保留 PID、失败更新恢复二进制和归属、同摘要显式更新登记归属且不重启、扩展证书拒绝/原位安装/设置保留、GPG 夹具/篡改/清单漂移通过（各三项设备测试） |

模拟器使用本地未签名原生构建、debug APK 和公开测试签名夹具。此轮验收对 ARM 做 ELF 构建验证，未在 ARM 硬件执行。生产 GPG/APK 签名和公开资产须在 GitHub Actions 实际运行后核验；本次实现未创建标签或远端 Release。
