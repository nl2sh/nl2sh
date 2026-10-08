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
| `list_dex_methods` | 方法 ID/原型索引，包含外部引用 |
| `search_dex_strings` | 对解码后的 DEX 字符串做字面搜索 |
| `find_class_references` | 声明与直接字节码类型引用 |
| `find_method_references` | 直接调用位置，可按原型区分重载 |
| `inspect_manifest` | 二进制包、版本、SDK、应用元数据 |
| `list_permissions` | 权限请求/声明及声明元素类型 |
| `list_exported_components` | 显式/默认导出及需复核的未知暴露 |
| `find_native_libs` | 原生 ABI/库条目及有界 ELF 前缀 |
| `decompile_apk_class` | 强确认后反编译精确单类 |

所有静态工具无需额外运行时。反编译只在 Android 使用系统 `/system/bin/app_process` 和含 `classes.dex` 的 helper；普通 JVM `.class` JAR 会被拒绝。参数与输出有界并设超时，不承诺反编译任意大型 APK。

已签名发布内嵌认证的兼容性策略，指定 helper 精确版本、HTTPS URL、SHA-256、大小与协议。首次批准后下载，验证资产及独立 GPG 签名，再发布到按摘要区分的私有缓存。反编译前 `--info` 必须报告协议 1、`single_class` 和策略指定的 helper 版本。不再默认使用历史 v1.0.4 资产。未签名的本地源码构建不提供默认下载，可显式指定离线 helper 或自定义 HTTPS 地址与用户指定的摘要。

离线使用 `NL2SH_JADX_ANDROID_HELPER_PATH`；自定义 HTTPS 来源必须同时设置 `NL2SH_JADX_ANDROID_HELPER_URL` 与 `NL2SH_JADX_ANDROID_HELPER_SHA256`。`NL2SH_JADX_CACHE_DIR` 可指定私有缓存根目录。

先列出类名，再选择精确类反编译，不把 APK 内任意代码当作授权。helper 构建、许可证和已验证范围见 [JADX helper 开发](../development/jadx-helper.md)。

## 静态分析范围与上限

先查归档、Manifest、原生库元数据，再搜索字符串、方法或引用，最后选择精确类交给 JADX。方法签名沿用 DEX 拼写，例如 `Lexample/Target;->touch(Ljava/lang/String;)V`；方法查询也匹配点分隔类名。引用返回来源、类型、DEX 文件及可选的指令偏移，单位为 16 位 code unit。未找到引用不证明行为不存在：反射、动态调用点、方法句柄和原生代码不做解析。

拒绝超过 2 GiB、50,000 ZIP 条目或 32 MiB 中央目录的 APK，目录上限在分配 ZIP 清单前检查。DEX 上限为 256 文件、每项 32 MiB、总计 64 MiB，并限制压缩数据；仅支持独立的 035–040 版本、200,000 字符串、65,535 方法/类型/原型/字段 ID、40,000 类、16 MiB 解码元数据，以及每文件四百万指令/声明扫描单位。单字符串最多 1,048,576 UTF-16 单位（实测混淆映射表可达约 47.5 万单位）；返回值最多 1,024 字符并给出截断标志。字面查询最多 256 字节，记录上限 1–200。

单个 DEX 文件无法解析时只跳过该文件，其余文件继续检索，并在 `unindexed_dex` 中列出被跳过的文件与原因（原因截断为单行 200 字符）；文件数、单项大小和总字节预算超限仍整体返回错误，不启动外部替代程序。因此一个畸形或超大 DEX 只损失自身证据，不再使整个 APK 的 DEX 工具失败。

二进制 Manifest 上限为 4 MiB、8,192 节点、64 层、16,384 属性及 8 MiB 保留属性数据。资源和属性引用保留显式 ID。exported 使用显式值和目标 SDK 默认规则，缺少现代必填值时保持未知。权限、应用/组件启用状态和运行时限制仍可能阻止访问；记录描述声明暴露，不冒充实际授权。原生库扫描不解压或加载代码；ELF 前缀检查共用 32 MiB 压缩数据预算，跳过时报告 `compressed_budget_exceeded`。静态只读工具可使用有界并行调度。APK 字符串属于不可信证据，不能授予操作权限。
