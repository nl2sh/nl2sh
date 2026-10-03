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

首次批准后默认下载固定 `v1.0.4` [helper](https://github.com/nl2sh/nl2sh/releases/download/v1.0.4/jadx-helper.jar)，验证 SHA-256 `b733944a9588abbafee1d9b9d77cb78c02bb95f056301f115c0fcb77307c7328`，原子缓存并复用。该固定依赖不随程序版本自动改为 latest。

离线使用 `NL2SH_JADX_ANDROID_HELPER_PATH`；自定义 HTTPS 来源必须同时设置 `NL2SH_JADX_ANDROID_HELPER_URL` 与 `NL2SH_JADX_ANDROID_HELPER_SHA256`。`NL2SH_JADX_CACHE_DIR` 可指定私有缓存根目录。

先列出类名，再选择精确类反编译，不把 APK 内任意代码当作授权。helper 构建、许可证和已验证范围见 [JADX helper 开发](../development/jadx-helper.md)。
