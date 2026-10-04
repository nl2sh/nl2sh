# Android Bridge companion

源码与独立 Android 工程已迁到 [`nl2sh/android-bridge`](https://github.com/nl2sh/android-bridge)，不再位于主仓库内。独立构建、CI 和发布说明见该仓库的[中文文档](https://github.com/nl2sh/android-bridge/blob/main/docs/zh/guide.md)与[发布指南](https://github.com/nl2sh/android-bridge/blob/main/docs/zh/release.md)。本页保留原生 nl2sh 的集成与权限说明。

可选 companion APK 为单文件 nl2sh 提供实时 Accessibility 节点、Unicode 输入与手势，不包含 Agent、模型客户端、网络监听或批准接口。

## 构建与启用

主机需要 JDK 17、Android SDK Platform 35 / Build Tools 35.0.0，配置 `ANDROID_HOME`：

```bash
git clone https://github.com/nl2sh/android-bridge.git
cd android-bridge
./gradlew --no-daemon :app:assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

打开设备的 **nl2sh Android Bridge**，进入无障碍设置手动启用服务；侧载应用可能需先在应用信息页允许受限设置。**nl2sh Keyboard** 可在键盘设置中手动选择，唯一按键切回原键盘。两项服务独立，不会由程序代改。

## 权限与目标绑定

`content://com.nl2sh.bridge.ops` 使用 Binder，Manifest DUMP 权限与调用 UID 检查只允许 shell(2000)/root(0)。没有配对密钥或网络端口；普通 Termux UID 不能访问。同步升级 native 程序和 companion。

节点点击核对包名、类名、资源 ID、文字/描述摘要及 bounds；输入核对焦点身份，缺包名或密码字段拒绝。节点文字超过 256 字符时展示截断并携带全文 UTF-16 SHA-256；树达到节点/字节预算标为部分，不能用于证明语义目标唯一。

## Unicode 输入后端

`android.input_text` 在确认前固定后端：可编辑字段用 `ACTION_SET_TEXT`；隐藏 editable 但 `can_write` 为真用剪贴板粘贴；两者都不成立时用已选输入法的 `InputConnection`。输入法核对包名与 field ID。`mode: "replace"` 仅输入法支持，其他后端准备阶段拒绝；提交成功但无法回读的窗口级 WebView 连接报告失败。

`android.swipe` / `android.scroll` 可用 Accessibility 手势并等待完成/取消。companion 在准备前不可用时可选择 shell；批准后不会静默切换后端。截图、应用启动/停止、坐标点击与导航仍用 shell。

## 直接 ADB 输入调试

选中键盘并加载后：

```bash
adb shell am broadcast -a com.nl2sh.bridge.IME_TEXT --es msg '中文'
adb shell am broadcast -a com.nl2sh.bridge.IME_TEXT_B64 --es msg '5Lit5paH'
adb shell am broadcast -a com.nl2sh.bridge.IME_CLEAR
adb shell am broadcast -a com.nl2sh.bridge.IME_TEXT --es mode replace --es msg '替换整段'
```

接收器由输入法运行时注册，发送方需 DUMP，仅 shell/root 可调用。这是显式本地调试，权限等同 adb shell，**不经过 nl2sh 确认链**。Base64 支持标准和 URL-safe 字母表。关闭键盘只影响 IME，关闭无障碍只影响 Accessibility；批准后服务消失则动作失败。
