# 工具能力

工具由显式注册表向模型提供名称与 JSON Schema。参数先验证，操作先准备，再评估风险、确认并执行；工具输出标记完整/部分/失败/超时，不能只根据模型文字判断成功。

| 任务 | 文档 |
| --- | --- |
| 设备诊断与 UI | [Android](android.md) |
| 读取、搜索、补丁 | [文件系统](filesystem.md) |
| WAV / PCM 特征与质量 | [音频](audio.md) |
| ZIP / DEX / 单类反编译 | [APK / JADX](apk-jadx.md) |
| 文件传输与端口服务 | [Tailcat](tailcat.md) |
| 公网 HTTP / TLS | [网络](network.md) |
| ima、便签、图表 | [知识与呈现](knowledge.md) |
| nl2sh 配置查询、修改与重置 | [自身配置](configuration.md) |

APK/JADX 和 Tailcat 默认关闭，Web 工具页或 TUI `/config` 的“工具”分类可按组或单项启用。`tool_overrides` 优先于 `tool_groups`；关闭工具不会进入模型定义或直接调用。切换组开关会清除组内单项覆盖。新 Web 任务读取配置；TUI 保存后自动重载，无需重启。开启工具不等于批准动作。ima 需要独立凭据。

[完整工具参数目录](../reference/tool-catalog.md)由代码导出，包含所有可选项；实际可用性仍由配置、能力和入口决定。设备协议服务支持需要长期存活的 Tailcat 监听器操作。

## 后台 Shell 采集

`execute_shell_command` 的 `background: true` 在同一安全分类、命令编辑重评估和确认链之后启动命令，立即返回 `status: "started"` 与随机 `child_id`。这只证明成功创建子进程，不能证明命令已成功完成。适合持续 `logcat` 等无需输入的采集；`interactive: true` 与后台模式不能组合，`interactive_execute_timeout_secs: 0` 仍是前台等待。

```json
{"command":"logcat -v threadtime", "background":true, "background_timeout_secs":300}
```

通过 `read_output` 查询实际状态及双流输出；stdout 和 stderr 的偏移分别累加，使用返回的 `stdout.next_offset` / `stderr.next_offset` 继续读取：

```json
{"child_id":"<启动返回的 UUID>", "offset":0, "stderr_offset":0, "max_bytes":1024}
```

`offset` 为原始 stdout 字节偏移，`stderr_offset` 为原始 stderr 字节偏移；不是字符数或返回文本长度。无效 UTF-8 以替换字符显示，分页可能拆开多字节字符，终端控制序列经过过滤。没有新输出不代表退出；检查 `finished`、`exit_code`、`signal`、`timed_out` 和 `error`。命令实际失败与读取工具成功是两个不同状态。结束采集时调用 `kill`，该修改操作仍需确认；只接受托管 `child_id`，不接受任意 PID。停止发送 TERM，500 ms 后向原进程组发送 KILL，并等待回收；保留的已结束句柄可重复停止。

- 每进程最多保留 16 个句柄，满额时只淘汰最旧的已结束项；全部运行中则拒绝启动。stdout/stderr 各保留最近 1 MiB，持续排空管道，单次每流读取 1–16384 字节。旧偏移被淘汰时 `truncated: true`，`offset` / `available_offset` 指明实际保留起点。
- `background_timeout_secs` 默认 3600，范围 1–86400；到期自动清理，不使用前台命令超时。后台 stdin 是 `/dev/null`，stdout/stderr 是独立管道，无 PTY、终端挂起或实时屏幕写入；应用可能因非 TTY 改变缓冲行为。
- 句柄属于当前 nl2sh 进程和配置身份，同配置后续任务可以查询；任务结束、取消或客户端断开不会停止已启动采集。nl2sh 正常退出清理后台命令，重启不恢复句柄或输出。独立进程和其他配置不能接管；宿主突然退出、Android OOM/厂商回收不保证清理或存活，不提供持久守护服务。
- 遵循 `execute_user_mode`，但非 root nl2sh 的后台 `su` 提权被拒绝，避免产生无法可靠停止的 Root 进程；已以 root 运行的 nl2sh 可以托管，命令风险与确认不降低。`tcpdump` 等是否可运行还取决于当前 UID、SELinux 和设备权限。
- Android shell/root 身份下，可能修改状态的后台命令保留 UI 资源租约直到回收，同任务后续动作和其他 UI/Shell 任务需重新获取租约，可能报告忙碌；只读采集不会在任务完成后保留该租约。`read_output` 和 `kill` 无需该租约，仍可查询与停止。不要用后台命令进行持续 UI 自动化。
- 仅支持留在原进程组中的命令；不要自行 `nohup`、`setsid`、双重 fork 或 daemonize。父 shell 自然退出也会清理原组后代；逃逸后代不在托管范围，输出管道未关闭会返回 `error`，不得当作完整采集。
