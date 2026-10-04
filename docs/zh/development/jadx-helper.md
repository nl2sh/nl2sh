# JADX helper 开发

源码与独立 Android 工程已迁到 [`nl2sh/jadx-helper`](https://github.com/nl2sh/jadx-helper)。先克隆该仓库并进入其根目录，再执行下述构建命令；构建、CI 与后续标签发布由独立仓库维护，见[发布指南](https://github.com/nl2sh/jadx-helper/blob/main/docs/zh/release.md)。主仓库 Release 不再重复构建或发布 helper。现有默认 URL 与固定摘要仍指向历史 `v1.0.4` 资产；使用独立仓库新资产需显式配置 HTTPS URL 与对应摘要，或离线路径，不自动跟随 latest。

```bash
git clone https://github.com/nl2sh/jadx-helper.git
cd jadx-helper
```

模块把未签名 Android APK 的运行时内容重封装为 `jadx-helper.jar`，包含 classes.dex 与 `com.nl2sh.jadx.Main`；不安装为应用，不使用设备 Java 命令。入口接收 APK、精确类名、输出 Java 路径，native 端提供超时、输出限额、摘要验证与强确认。

固定 JADX core/DEX input 1.5.1，Android API 26+，AGP 8.7.3，Gradle Wrapper 8.9，JDK 17，SDK API 35。Linux/macOS 执行 `./build-helper.sh`，Windows 执行 `./build-helper.ps1`，无需系统 Gradle。

Wrapper 验证 Gradle 发行包摘要；重封装条目顺序、时间戳与压缩方式固定。两个入口输出 dist/jadx-helper.jar、摘要和 metadata.json，dist 不提交。分发时附适用 JADX 和依赖许可证。

已验证 API35 x86_64 模拟器和 API28 ARMv7 单类反编译；API26 真机、大型 multidex、内存峰值与超时清理需扩展覆盖。固定 v1.0.4 资产摘要见 [用户工具指南](../tools/apk-jadx.md)。手工 app_process 必须提供私有可写临时目录：

```bash
CLASSPATH=/data/local/tmp/jadx-helper.jar /system/bin/app_process   -Djava.io.tmpdir=/data/local/tmp/jadx-work / com.nl2sh.jadx.Main APK CLASS OUTPUT
```

以上路径是设备路径，先创建自己的私有工作目录；不要使用常规 JVM JAR 替代 DEX JAR。
