# 快速开始

按下面四步开始，不需要先了解 Rust 或工具协议。

1. [选择安装方式](installation.md)：电脑通过 ADB，或 Android 上的 Termux。
2. [连接 Android](android-adb.md)，或[安装 Termux 包](termux.md)。
3. [配置模型服务](configure-provider.md)：准备服务地址、模型名称和 API Key。
4. [完成第一个只读任务](first-task.md)，了解工具结果和确认窗口。

nl2sh 不是 Android 聊天 APK。默认交付是 Android API 26+ 的 shell 可执行文件；可选 companion APK 只提供系统交互能力。完整 UI 自动化需要 shell/root UID，普通 Termux UID 不具备这些权限。
