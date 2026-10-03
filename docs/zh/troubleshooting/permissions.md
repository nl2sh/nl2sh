# 权限 / Root 排查

先确认程序实际 UID：ADB shell 通常 2000，Root 为 0，Termux 是应用 UID。普通 Termux 可做自身目录与模型任务，但不能访问需要 shell/root 的 UI 注入、截图或 companion Binder。

`permission denied` 时核对目标路径、属主、模式、SELinux 与 Android 服务权限，不把读取失败当文件不存在。`execute_user_mode = "normal"` 不调用 su；root 模式 su 失败明确拒绝，不降级执行。主机 root adbd 失败不等于设备不能通过 su 提权。

私有配置使用 0600；启动器若身份不匹配会提前失败。不要改成全局可读来解决 Key 配置问题，可在正确身份下重新部署显式配置。

companion 需要用户手动启用无障碍/键盘，可能先允许受限设置。两者独立；Unicode replace 只支持 IME。服务在批准后消失、树不完整、包名/身份改变会拒绝动作，这些检查不要关闭。

详见 [Root](../guide/root.md)、[Android Bridge](../advanced/android-bridge.md)与[安全模型](../reference/security-model.md)。
