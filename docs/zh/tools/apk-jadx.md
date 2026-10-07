# APK / JADX

默认关闭；在 Web 工具页开启 `jadx` 组，或配置：

```toml
[tool_groups]
jadx = true
```

| 工具 | 行为 |
| --- | --- |
| `inspect_apk` | Rust 有界 ZIP 概览 |
| `list_apk_entries` | 过滤列出归档条目 |
| `list_dex_classes` | Rust 读取 DEX 类索引 |
| `decompile_apk_class` | 强确认后反编译精确单类 |

前三项无需额外运行时。反编译只在 Android 使用系统 `/system/bin/app_process` 和含 `classes.dex` 的 helper；普通 JVM `.class` JAR 会被拒绝。参数与输出有界并设超时，不承诺反编译任意大型 APK。

已签名发布内嵌认证的兼容性策略，指定 helper 精确版本、HTTPS URL、SHA-256、大小与协议。首次批准后下载，验证资产及独立 GPG 签名，再发布到按摘要区分的私有缓存。反编译前 `--info` 必须报告协议 1、`single_class` 和策略指定的 helper 版本。不再默认使用历史 v1.0.4 资产。未签名的本地源码构建不提供默认下载，可显式指定离线 helper 或自定义 HTTPS 地址与用户指定的摘要。

离线使用 `NL2SH_JADX_ANDROID_HELPER_PATH`；自定义 HTTPS 来源必须同时设置 `NL2SH_JADX_ANDROID_HELPER_URL` 与 `NL2SH_JADX_ANDROID_HELPER_SHA256`。`NL2SH_JADX_CACHE_DIR` 可指定私有缓存根目录。

先列出类名，再选择精确类反编译，不把 APK 内任意代码当作授权。helper 构建、许可证和已验证范围见 [JADX helper 开发](../development/jadx-helper.md)。
