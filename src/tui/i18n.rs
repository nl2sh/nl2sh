use crate::{config::UiLanguage, security::RiskLevel};

const OPEN_SOURCE_SUPPORT: &str = "⭐ 这个项目完全开源、单二进制、本地执行。点个 Star 或提个 Issue 已经是莫大支持。点击支持 -> https://github.com/nl2sh/nl2sh";
const DONATION_SUPPORT: &str = "❤️ 如果 nl2sh 帮你少敲了几条 adb 命令、省下了调试 Android 设备的时间，欢迎请我喝杯咖啡 ☕  点击赞赏 -> https://suqishuo.cn/uploads/wechatpay.png";
pub(crate) const BUDDHA_ART_PREFIX: &str = "\u{1e}BUDDHA:";
pub(crate) const WELCOME_TRAIN_ANCHOR: &str = "\u{1e}WELCOME_TRAIN";
const BUDDHA_ART: &str = r#"\\ \\ \\ \\ \\ \\ \\ \\ || || || || || || // // // // // // // //
\\ \\ \\ \\ \\ \\ \\        _ooOoo_          // // // // // // //
\\ \\ \\ \\ \\ \\          o8888888o            // // // // // //
\\ \\ \\ \\ \\             88" . "88               // // // // //
\\ \\ \\ \\                (| -_- |)                  // // // //
\\ \\ \\                   O\  =  /O                     // // //
\\ \\                   ____/`---'\____                     // //
\\                    .'  \\|     |//  `.                      //
==                   /  \\|||  :  |||//  \                     ==
==                  /  _||||| -:- |||||-  \                    ==
==                  |   | \\\  -  /// |   |                    ==
==                  | \_|  ''\---/''  |   |                    ==
==                  \  .-\__  `-`  ___/-. /                    ==
==                ___`. .'  /--.--\  `. . ___                  ==
==              ."" '<  `.___\_<|>_/___.'  >'"".               ==
==            | | :  `- \`.;`\ _ /`;.`/ - ` : | |              \\
//            \  \ `-.   \_ __\ /__ _/   .-` /  /              \\
//      ========`-.____`-.___\_____/___.-`____.-'========      \\
//                           `=---='                           \\
// //   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^  \\ \\
// // //   佛祖保佑   终端一念清净  adb 万事如意 _/\_    \\ \\ \\
// // // // // // || || || || || || || || || || \\ \\ \\ \\ \\ \\"#;

fn support_history(show_buddha_ascii_art: bool) -> Vec<String> {
    let mut history = vec![OPEN_SOURCE_SUPPORT.into(), DONATION_SUPPORT.into()];
    if show_buddha_ascii_art {
        history.push(format!("{BUDDHA_ART_PREFIX}{BUDDHA_ART}"));
    }
    history
}

pub(crate) fn startup_history(
    language: UiLanguage,
    ascii: bool,
    show_buddha_ascii_art: bool,
    show_train_ascii_art: bool,
) -> Vec<String> {
    let agent = if ascii { "[AGENT]" } else { "🤖" };
    let hint = if ascii { "[HINT]" } else { "💡" };
    let mut history = match language {
        UiLanguage::ZhCn => vec![
            format!("{agent} 欢迎使用 nl2sh，请直接描述要完成的 Android Shell 任务。"),
            format!("{hint} 常用示例：查看已安装应用及版本信息"),
            format!("{hint} 常用示例：查看系统版本、CPU、内存和存储空间"),
            format!("{hint} 常用示例：查找占用空间最大的十个文件"),
            format!("{hint} 常用示例：查看正在运行的进程和网络连接"),
            format!("{hint} 常用命令：/shell 普通终端；/help 帮助；/clear 清空；/exit 退出"),
            format!("{hint} 设置：/config 或 /setting 打开统一多 Tab 面板"),
            format!("{hint} 更新：/update 手工检查；启动时自动后台检查"),
            format!("{hint} 操作说明：滚轮浏览历史；Shift+拖选文字后用右键菜单复制"),
            format!("{hint} 操作说明：Ctrl+C 取消任务或清空输入；Ctrl+Q 安全退出"),
        ],
        UiLanguage::En => vec![
            format!("{agent} Welcome to nl2sh. Describe an Android shell task to begin."),
            format!("{hint} Example: show installed applications and version information"),
            format!("{hint} Example: show Android version, CPU, memory, and storage"),
            format!("{hint} Example: find the ten largest files"),
            format!("{hint} Example: show running processes and network connections"),
            format!("{hint} Commands: /shell terminal; /help help; /clear clear; /exit quit"),
            format!("{hint} Settings: /config or /setting opens the unified tabbed panel"),
            format!("{hint} Updates: /update checks manually; startup checks in the background"),
            format!("{hint} Controls: wheel browses history; Shift+drag selects text for context-menu copy"),
            format!("{hint} Controls: Ctrl+C cancels or clears input; Ctrl+Q quits safely"),
        ],
    };
    history.extend(support_history(show_buddha_ascii_art));
    if show_train_ascii_art {
        history.push(WELCOME_TRAIN_ANCHOR.into());
    }
    history
}

pub(crate) fn help_history(
    language: UiLanguage,
    ascii: bool,
    show_buddha_ascii_art: bool,
) -> Vec<String> {
    let hint = if ascii { "[HINT]" } else { "💡" };
    let mut history = match language {
        UiLanguage::ZhCn => vec![
            format!("{hint} /help 显示此帮助"),
            format!("{hint} /clear 清空当前会话的对话、模型上下文和输入历史；审计日志保留"),
            format!("{hint} /new 开始新的空白会话；已保存会话和审计日志保留"),
            format!("{hint} /sessions 打开最近会话列表；输入序号或用 ↑/↓ 选择并恢复"),
            format!("{hint} /exit 安全退出"),
            format!("{hint} /shell 进入普通命令终端；输入 exit 或按 Ctrl+D 返回 TUI"),
            format!("{hint} !命令 直接安全检查并运行，不请求模型；例如 !pwd"),
            format!("{hint} /config 打开统一多 Tab 设置面板（Ctrl+S 保存）"),
            format!("{hint} /balance 查询支持的 Provider 余额，结果不写入日志"),
            format!("{hint} /setting 是 /config 的别名"),
            format!("{hint} /update 检查并提示安装最新版本"),
            format!("{hint} Enter 提交；Up/Down 输入历史；F2 展开工具结果"),
            format!("{hint} 滚轮或 PageUp/PageDown 浏览；Shift+拖选后右键复制"),
            format!("{hint} Ctrl+C 取消任务或清空输入；Ctrl+Q 安全退出"),
        ],
        UiLanguage::En => vec![
            format!("{hint} /help show this help"),
            format!("{hint} /clear clear this session's conversation, model context, and input history; keep the audit log"),
            format!("{hint} /new start a blank session; keep saved sessions and the audit log"),
            format!("{hint} /sessions opens recent sessions; type a number or use Up/Down to restore"),
            format!("{hint} /exit quit safely"),
            format!("{hint} /shell open a regular command terminal; type exit or press Ctrl+D to return to the TUI"),
            format!("{hint} !command checks and runs a command without calling the model; for example !pwd"),
            format!("{hint} /config open the unified tabbed settings panel (Ctrl+S saves)"),
            format!("{hint} /balance query supported provider balances without logging them"),
            format!("{hint} /setting is an alias for /config"),
            format!("{hint} /update check for and offer the latest release"),
            format!("{hint} Enter submits; Up/Down recalls input; F2 expands tool results"),
            format!("{hint} Wheel or PageUp/PageDown scrolls; Shift+drag selects for context-menu copy"),
            format!("{hint} Ctrl+C cancels or clears input; Ctrl+Q quits safely"),
        ],
    };
    history.extend(support_history(show_buddha_ascii_art));
    history
}

pub(crate) fn mode_agent(language: UiLanguage) -> &'static str {
    match language {
        UiLanguage::ZhCn => "智能体",
        UiLanguage::En => "Agent",
    }
}

pub(crate) fn idle(language: UiLanguage) -> &'static str {
    match language {
        UiLanguage::ZhCn => "空闲",
        UiLanguage::En => "idle",
    }
}

pub(crate) fn risk(language: UiLanguage, risk: RiskLevel) -> &'static str {
    match (language, risk) {
        (UiLanguage::ZhCn, RiskLevel::ReadOnly) => "只读",
        (UiLanguage::ZhCn, RiskLevel::Mutating) => "修改",
        (UiLanguage::ZhCn, RiskLevel::Dangerous) => "危险",
        (UiLanguage::ZhCn, RiskLevel::Critical) => "严重危险",
        (UiLanguage::En, RiskLevel::ReadOnly) => "ReadOnly",
        (UiLanguage::En, RiskLevel::Mutating) => "Mutating",
        (UiLanguage::En, RiskLevel::Dangerous) => "Dangerous",
        (UiLanguage::En, RiskLevel::Critical) => "Critical",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn startup_help_is_populated_in_both_languages() {
        let chinese = startup_history(UiLanguage::ZhCn, true, true, true);
        let english = startup_history(UiLanguage::En, true, true, true);
        assert!(chinese.len() >= 8);
        assert!(english.len() >= 8);
        assert!(chinese.iter().any(|line| line.contains("应用")));
        assert!(chinese.iter().any(|line| line.contains("/config")));
        assert!(chinese.iter().any(|line| line.contains("Shift+拖选")));
        assert!(english.iter().any(|line| line.contains("applications")));
        assert!(english.iter().any(|line| line.contains("Shift+drag")));
        assert!(english.iter().any(|line| line.contains("Ctrl+Q")));
        assert!(chinese.iter().any(|line| line.contains("nl2sh/nl2sh")));
        assert!(chinese.iter().any(|line| line.contains("点击支持 ->")));
        assert!(chinese.iter().any(|line| line.contains("点击赞赏 ->")));
        assert!(chinese.iter().any(|line| line.contains("佛祖保佑")));

        let help = help_history(UiLanguage::ZhCn, true, true);
        assert!(help.iter().any(|line| line.contains("/help")));
        assert!(help.iter().any(|line| line.contains("/exit")));
        assert!(help.iter().any(|line| line.contains("审计日志保留")));
        assert!(help.iter().any(|line| line.contains("suqishuo.cn")));
        assert!(help.iter().any(|line| line.contains("adb 万事如意")));
    }

    #[test]
    fn startup_ascii_art_switches_are_independent() {
        let train_only = startup_history(UiLanguage::En, true, false, true);
        assert!(!train_only
            .iter()
            .any(|line| line.starts_with(BUDDHA_ART_PREFIX)));
        assert!(train_only.iter().any(|line| line == WELCOME_TRAIN_ANCHOR));

        let buddha_only = startup_history(UiLanguage::En, true, true, false);
        assert!(buddha_only
            .iter()
            .any(|line| line.starts_with(BUDDHA_ART_PREFIX)));
        assert!(!buddha_only.iter().any(|line| line == WELCOME_TRAIN_ANCHOR));
    }

    #[test]
    fn buddha_blessing_row_respects_the_ascii_frame_width() {
        let mut lines = BUDDHA_ART.lines();
        let frame_width = lines.next().map(UnicodeWidthStr::width).unwrap_or(0);
        let blessing_width = BUDDHA_ART
            .lines()
            .find(|line| line.contains("佛祖保佑"))
            .map(UnicodeWidthStr::width)
            .unwrap_or(0);
        let closing_width = BUDDHA_ART
            .lines()
            .next_back()
            .map(UnicodeWidthStr::width)
            .unwrap_or(0);
        assert_eq!(frame_width, 65);
        assert_eq!(blessing_width, frame_width);
        assert_eq!(closing_width, frame_width);
    }
}
