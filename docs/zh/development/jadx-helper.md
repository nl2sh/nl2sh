# JADX helper 开发

独立源码在 [nl2sh/jadx-helper](https://github.com/nl2sh/jadx-helper) 维护，构建前先克隆该仓库。


模块把未签名 Android APK 的运行时内容重封装为 `jadx-helper.jar`，包含 classes.dex 与 `com.nl2sh.jadx.Main`；不安装为应用，不使用设备 Java 命令。入口接收 APK、精确类名、输出 Java 路径，native 端提供超时、输出限额、摘要验证与强确认。

固定 JADX core/DEX input 1.5.1，Android API 26+，AGP 8.7.3，Gradle Wrapper 8.9，JDK 17，SDK API 35。Linux/macOS 执行 `./build-helper.sh`，Windows 执行 `./build-helper.ps1`，无需系统 Gradle。

Wrapper 验证 Gradle 发行包摘要；重封装条目顺序、时间戳与压缩方式固定。两个入口输出 dist/jadx-helper.jar、摘要和 metadata.json，dist 不提交。分发时附适用 JADX 和依赖许可证。

已验证 API35 x86_64、API28 ARMv7 和 API26 x86_64 ART 的单类反编译；大型 multidex、内存峰值与超时清理需扩展覆盖。原生发布通过认证的兼容性 Manifest 选择 helper，不再默认使用历史 v1.0.4 资产。未签名的本地源码构建可显式指定离线 DEX helper，或提供自定义 HTTPS 地址与用户指定的 SHA-256。获取与反编译仍须强确认。手工 app_process 必须提供私有可写临时目录：

```bash
CLASSPATH=/data/local/tmp/jadx-helper.jar /system/bin/app_process   -Djava.io.tmpdir=/data/local/tmp/jadx-work / com.nl2sh.jadx.Main APK CLASS OUTPUT
```

以上路径是设备路径，先创建自己的私有工作目录；不要使用常规 JVM JAR 替代 DEX JAR。

可复现摘要比较须使用同一 Git revision 与固定工具链：APK 包含 AGP 的 Git revision 元数据，提取模块历史会改变此元数据，因此独立工程重建的 JAR 不一定与历史 nl2sh `v1.0.4` 摘要相同。构建包含 `assets/nl2sh-runtime.json`；打包脚本从它生成 `metadata.json`，添加实际摘要与大小。运行时 `--info` 与资产元数据共享构建版本和协议字段，标签发布显式设置版本。原生主发布使用其信任根为汇总的 JAR 签名。

## 入口契约与失败处理

调用 `com.nl2sh.jadx.Main` 时必须提供三个参数：输入 APK、原始完整类名、输出 Java 文件。输出父目录和 `java.io.tmpdir` 必须事先存在且可写。输出为 UTF-8，已有输出文件会被覆盖。实现使用单线程、SIMPLE 反编译、无代码缓存，不生成 imports，不使用调试信息、内联或重命名；过滤器匹配指定类及其 `$` 内部类。跳过资源，明确禁用 XML 解析，不导出整个工程，也不保证结果可以重新编译。

| 退出码 | 含义 |
| --- | --- |
| 0 | 已写入源码 |
| 2 | 参数数量不是三个 |
| 3 | JADX 返回空源码 |
| 4 | 找不到指定原始类名 |
| 5 | 加载、反编译或文件写入抛出异常；查看 stderr |

这些退出码属于 helper 自身，原生 nl2sh 另行提供超时和有界输出处理。手工 `app_process` 调用不经过 nl2sh 审批，也不继承其限制；正常 Agent 工作流应使用原生工具。找不到类时核对原始类名，而非展示或反混淆别名；DEX/JAR 加载失败时检查产物格式和设备兼容性；临时目录错误时提供私有可写目录。

## 运行时版本信息

`CLASSPATH=/data/local/tmp/jadx-helper.jar /system/bin/app_process / com.nl2sh.jadx.Main --info`
返回一行 JSON：`protocol: 1`、构建版本 `helper_version`、固定依赖版本 `jadx_core` 以及
`features: ["single_class", "inner_classes"]`。此调用成功退出，不打开 APK、不写入文件。
客户端应在反编译前拒绝不支持的协议版本。仍需进行 SHA-256 和 DEX 校验；版本信息不能认证下载代码。

协议 1 的 `--info`、SHA-256 和原有三参数单类反编译入口通过 API 26 x86_64 ART 验证；
`./build-helper.sh` 与 `./gradlew --no-daemon :app:lintRelease` 通过。
