# Android 设备与 UI

## 只读诊断

`inspect_android_environment` 返回系统/API/ABI、命令可用性、内存和 `/data` 容量，不安装软件。`inspect_android_app` 查询指定或前台应用的 Activity、进程、内存、版本和位置。`android_dumpsys`、`android_logcat`、`android_settings`、`android_content_query` 接受受限参数，不提供写设置、service call 或 ContentProvider 写入。

还提供通知、Crash/ANR、温控/功耗、流量、存储、Wi-Fi/以太网、Doze、权限与连接性聚合。默认有界定向查询；超时后先缩小范围。`android_connectivity` 的 ping 成功不代表 HTTPS 下载成功。

只读诊断使用当前进程身份，以非交互捕获模式运行，即使 Shell 配置了 root 模式。`android_dumpsys` 只接受经过审核的查询形式；`android_logcat` 接受标签/优先级过滤规则，不接受命令选项。原始 Shell 中未知或修改性的 `dumpsys`/`logcat` 选项要求强确认。独立诊断可并发执行，取消时等待子进程回收。

## 界面闭环

先 `android.screen_dump` 获取有界节点，再按真实包名、文字、资源 ID 或 bounds 选择动作；`android.tap_text` / `android.tap_node` 在确认后重新核对身份。部分树不能证明语义目标唯一，会拒绝点击。

`android.launch_app` 解析包内 MAIN/LAUNCHER 组件后启动；`android.stop_app`、`android.tap`、`android.swipe`、`android.scroll`、导航和输入属于需确认的操作。`android.scroll` 可省略坐标并默认向下，`direction: "up"` 向上。操作后重新读取界面并检查 `success`，不要从点击命令退出码推断业务完成。

无 companion 时 `android.input_text` 只接受可打印 ASCII；Unicode 和实时 Accessibility 树见 [Android Bridge](../advanced/android-bridge.md)。完整 UI 能力要求 shell/root，普通 Termux UID 不支持。

`inspect_android_ui` / `inject_android_input` 提供原有 bounds 校验入口；输入准备前和执行前重新读取树，坐标必须仍在匹配且可用节点内。

## 截图与视觉

`android.screenshot` / `android.read_screen` 无路径时用私有临时目录返回图片附件；指定持久路径需确认。`capture_android_screen` 确认后写指定 PNG。`view_screenshot` 支持 PNG/JPEG/WebP，必要时缩放并转 JPEG，附件不保存进会话。需要支持视觉的模型；MediaStore 的时间/尺寸不是画面证据。

例子：“查看最近的 ANR 并列出日志证据，不重启应用”；“读当前界面，解释搜索框位置，等待我确认后再输入”。所有参数见 [目录](../reference/tool-catalog.md)。

Bridge 协议 v2 使用单一 base64url JSON payload，带协议版本与请求 ID；支持旧伴侣的逐方法调用。
v2 动作失败或回复 ID 不匹配时不会回退重放。原有审批、目标身份复核与 shell/root 边界继续生效。
