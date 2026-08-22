use crate::{config::UiLanguage, security::RiskLevel};

const OPEN_SOURCE_SUPPORT: &str = "⭐ 这个项目完全开源、单二进制、本地执行。点个 Star 或提个 Issue 已经是莫大支持。点击支持 -> https://github.com/Ernest-su/nl2sh";
const DONATION_SUPPORT: &str = "❤️ 如果 nl2sh 帮你少敲了几条 adb 命令、省下了调试 Android 设备的时间，欢迎请我喝杯咖啡 ☕  点击赞赏 -> https://suqishuo.cn/uploads/wechatpay.png";
pub(crate) const BUDDHA_ART_PREFIX: &str = "\u{1e}BUDDHA:";
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

fn support_history() -> Vec<String> {
    vec![
        OPEN_SOURCE_SUPPORT.into(),
        DONATION_SUPPORT.into(),
        format!("{BUDDHA_ART_PREFIX}{BUDDHA_ART}"),
    ]
}

pub(crate) fn startup_history(language: UiLanguage, ascii: bool) -> Vec<String> {
    let agent = if ascii { "[AGENT]" } else { "🤖" };
    let hint = if ascii { "[HINT]" } else { "💡" };
    let mut history = match language {
        UiLanguage::ZhCn => vec![
            format!("{agent} 欢迎使用 nl2sh，请直接描述要完成的 Android Shell 任务。"),
            format!("{hint} 这是 KONKA 内部便捷体验版本。已自动设置内置大模型，已检测到是 Konka 的小伙伴在使用。欢迎使用反馈。"),
            format!("{hint} 常用示例：查看已安装应用及版本信息"),
            format!("{hint} 常用示例：查看系统版本、CPU、内存和存储空间"),
            format!("{hint} 常用示例：查找占用空间最大的十个文件"),
            format!("{hint} 常用示例：查看正在运行的进程和网络连接"),
            format!("{hint} 常用命令：/help 帮助；/clear 清空；/exit 退出；/config 完整配置"),
            format!("{hint} 配置命令：/provider 配置 API；/model 配置模型"),
            format!("{hint} 操作说明：滚轮浏览历史；Shift+拖选文字后用右键菜单复制"),
            format!("{hint} 操作说明：Ctrl+C 取消任务或清空输入；Ctrl+Q 安全退出"),
        ],
        UiLanguage::En => vec![
            format!("{agent} Welcome to nl2sh. Describe an Android shell task to begin."),
            format!("{hint} This is the KONKA internal convenience edition. The built-in model is configured automatically for Konka colleagues. Feedback is welcome."),
            format!("{hint} Example: show installed applications and version information"),
            format!("{hint} Example: show Android version, CPU, memory, and storage"),
            format!("{hint} Example: find the ten largest files"),
            format!("{hint} Example: show running processes and network connections"),
            format!("{hint} Commands: /help help; /clear clear; /exit quit; /config configure all"),
            format!("{hint} Setup: /provider configures the API; /model configures the model"),
            format!("{hint} Controls: wheel browses history; Shift+drag selects text for context-menu copy"),
            format!("{hint} Controls: Ctrl+C cancels or clears input; Ctrl+Q quits safely"),
        ],
    };
    history.extend(support_history());
    history
}

pub(crate) fn help_history(language: UiLanguage, ascii: bool) -> Vec<String> {
    let hint = if ascii { "[HINT]" } else { "💡" };
    let mut history = match language {
        UiLanguage::ZhCn => vec![
            format!("{hint} /help 显示此帮助"),
            format!("{hint} /clear 清空当前会话的对话、模型上下文和输入历史；审计日志保留"),
            format!("{hint} /exit 安全退出"),
            format!("{hint} /config 重新配置模型服务"),
            format!("{hint} /provider 配置 API Endpoint、API Key 和 API 类型"),
            format!("{hint} /model 配置模型名称"),
            format!("{hint} Enter 提交；Up/Down 输入历史；F2 展开工具结果"),
            format!("{hint} 滚轮或 PageUp/PageDown 浏览；Shift+拖选后右键复制"),
            format!("{hint} Ctrl+C 取消任务或清空输入；Ctrl+Q 安全退出"),
        ],
        UiLanguage::En => vec![
            format!("{hint} /help show this help"),
            format!("{hint} /clear clear this session's conversation, model context, and input history; keep the audit log"),
            format!("{hint} /exit quit safely"),
            format!("{hint} /config reconfigure the model provider"),
            format!("{hint} /provider configure API endpoint, key, and protocol"),
            format!("{hint} /model configure the model identifier"),
            format!("{hint} Enter submits; Up/Down recalls input; F2 expands tool results"),
            format!("{hint} Wheel or PageUp/PageDown scrolls; Shift+drag selects for context-menu copy"),
            format!("{hint} Ctrl+C cancels or clears input; Ctrl+Q quits safely"),
        ],
    };
    history.extend(support_history());
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
        let chinese = startup_history(UiLanguage::ZhCn, true);
        let english = startup_history(UiLanguage::En, true);
        assert!(chinese.len() >= 8);
        assert!(english.len() >= 8);
        assert!(chinese.iter().any(|line| line.contains("应用")));
        assert!(chinese
            .iter()
            .any(|line| line.contains("KONKA 内部便捷体验版本")));
        assert!(chinese.iter().any(|line| line.contains("/config")));
        assert!(chinese.iter().any(|line| line.contains("Shift+拖选")));
        assert!(english.iter().any(|line| line.contains("applications")));
        assert!(english
            .iter()
            .any(|line| line.contains("KONKA internal convenience edition")));
        assert!(english.iter().any(|line| line.contains("Shift+drag")));
        assert!(english.iter().any(|line| line.contains("Ctrl+Q")));
        assert!(chinese.iter().any(|line| line.contains("Ernest-su/nl2sh")));
        assert!(chinese.iter().any(|line| line.contains("点击支持 ->")));
        assert!(chinese.iter().any(|line| line.contains("点击赞赏 ->")));
        assert!(chinese.iter().any(|line| line.contains("佛祖保佑")));

        let help = help_history(UiLanguage::ZhCn, true);
        assert!(help.iter().any(|line| line.contains("/help")));
        assert!(help.iter().any(|line| line.contains("/exit")));
        assert!(help.iter().any(|line| line.contains("审计日志保留")));
        assert!(help.iter().any(|line| line.contains("suqishuo.cn")));
        assert!(help.iter().any(|line| line.contains("adb 万事如意")));
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
