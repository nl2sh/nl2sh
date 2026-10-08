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

## 系统性能 Trace

按 `start_system_trace → stop_system_trace → analyze_system_trace` 使用。开始和停止都需要修改确认，分析只读；不安装 Perfetto、不自动提权。采集要求设备 `/system/bin/perfetto` 和服务中的 `linux.ftrace` 可用，通常需要 Android shell/root；API 26+ 可运行 nl2sh 不代表设备一定提供 Perfetto。普通 Termux 应用 UID 不提供采集入口。

示例参数（开始后在设备上复现卡顿，再停止）：

```json
{"package":"com.example.app","duration_secs":30,"buffer_mb":8}
```

开始返回 `trace_id`；后两步都用 `{"trace_id":"返回的 ID"}`，分析可加 `package` 或 `pid`（二选一），以及 `threshold_ms: 50`、`frame_budget_ms: 16.667`。刷新率为 120 Hz 时可把帧预算设为约 8.333 ms。也可只读分析已存在的外部原始 protobuf 文件：`{"path":"/data/local/tmp/example.pftrace","pid":1234}`。路径不接受符号链接，压缩 Trace 和通用 TrackEvent 不在支持范围内。

采集固定请求调度、唤醒、Binder 和 gfx/view atrace；探测到时才加入进程元数据与 `android.surfaceflinger.frametimeline`。使用二进制 Perfetto 配置并关闭 compact_sched，不接受模型提供的配置或命令。时长默认 10 秒、最多 120 秒，缓冲默认 8 MiB、最多 32 MiB，文件最多 64 MiB。Perfetto 服务持有会话并在到期或文件上限时自动结束，允许跨 bridge 进程停止；只停止本工具创建的随机会话，不向任意 PID 发信号。主动采集中不能用 trace_id 分析。私有元数据保存在配置对应状态目录的 `system-traces/`，protobuf 文件由 Perfetto 以 `0600` 创建在系统允许的 `/data/misc/perfetto-traces/nl2sh-<trace_id>.pftrace`（受 Android SELinux 约束，不能直接写入任意应用目录）。最多保留 16 次；旧证据及对应元数据需用户显式删除，不上传。包过滤只限制应用 atrace，系统调度/Binder 仍全局采集，可能包含敏感名称。

报告列出覆盖计数、目标 PID、阈值、各类异常计数与最多 50 条例证，以及最多 30 个线程的运行/等待/唤醒摘要：

- 主线程：只计算 wakeup 或可运行抢占到下一次调度的长等待，排除正常睡眠；不是 ANR 诊断。
- RenderThread：长可运行等待与长 atrace 切片；缺少线程身份时不会猜测。
- 帧：Choreographer#doFrame 超帧预算仅是长回调证据；旧格式 FrameTimeline 的 late/drop/jank 标志单独报告，不把两者当成唯一掉帧数。
- Binder：同 debug_id 的发送到接收延迟，不包括处理或同步回复往返。
- CPU：长可运行等待与观测窗口内至少 100 次/秒的唤醒启发式，不等同功耗测量或已证明根因。

Rust 解析最多 64 MiB、10 万 packet、50 万事件、32768 个线程，单 packet 4 MiB；超限和损坏数据拒绝分析。跨 CPU 按时间排序；丢失标志清空配对状态，非 boot 时钟、压缩、compact_sched 和新版 TrackEvent FrameTimeline 标记为未支持/部分证据。未配对、边界未完成区间被排除；空结果或缺失数据源不能证明设备健康。完整参数见[工具目录](../reference/tool-catalog.md)。
