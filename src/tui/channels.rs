//! Owner channel settings; private QR and credentials never enter conversation history.
use super::app::PopupView;
use crate::{
    channels::{ChannelAction, ChannelManager, ChannelStatus},
    config::UiLanguage,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;

pub(super) struct Editor {
    manager: Arc<ChannelManager>,
    statuses: Vec<ChannelStatus>,
    language: UiLanguage,
    selected: usize,
    scroll: u16,
    refreshed: std::time::Instant,
    app_id: String,
    secret: String,
    robot_uin: String,
    error: Option<String>,
    action: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
}
impl Editor {
    pub async fn new(manager: Arc<ChannelManager>, language: UiLanguage) -> Self {
        let statuses = manager.snapshot().await.unwrap_or_default();
        let app_id = statuses
            .iter()
            .find(|s| s.kind == "qq")
            .and_then(|s| s.account.clone())
            .unwrap_or_default();
        let robot_uin = statuses
            .iter()
            .find(|s| s.kind == "qq")
            .and_then(|s| s.robot_uin.clone())
            .unwrap_or_default();
        Self {
            manager,
            statuses,
            language,
            selected: 0,
            scroll: 0,
            refreshed: std::time::Instant::now(),
            app_id,
            secret: String::new(),
            robot_uin,
            error: None,
            action: None,
        }
    }
    pub async fn poll(&mut self) {
        if self.action.as_ref().is_some_and(|a| a.is_finished()) {
            if let Some(action) = self.action.take() {
                match action.await {
                    Ok(Ok(())) => {
                        self.secret.clear();
                        self.error = None
                    }
                    Ok(Err(e)) => self.error = Some(e.to_string()),
                    Err(_) => self.error = Some("Channel action failed".into()),
                }
            }
        }
        if self.refreshed.elapsed() < std::time::Duration::from_secs(1) {
            return;
        }
        self.refreshed = std::time::Instant::now();
        if let Ok(statuses) = self.manager.snapshot().await {
            self.statuses = statuses;
        } else {
            self.error = Some("Channel status unavailable; retry or reopen settings".into());
        }
    }
    pub fn view(&self) -> PopupView {
        let zh = self.language == UiLanguage::ZhCn;
        let mut lines = vec![if zh {
            "渠道设置 · 微信 iLink / QQ 官方机器人"
        } else {
            "Channels · WeChat iLink / Official QQ Bot"
        }
        .into()];
        for kind in ["weixin", "qq"] {
            if let Some(s) = self.statuses.iter().find(|s| s.kind == kind) {
                lines.push(format!(
                    "{} · {} · {} · 收到/received {} 回复/sent {} 待审批/pending {}",
                    kind,
                    s.state,
                    s.account.as_deref().unwrap_or("unbound"),
                    s.received,
                    s.sent,
                    s.pending
                ));
                if let Some(error) = &s.error {
                    lines.push(error.clone());
                }
                if let Some(url) = &s.invite_url {
                    lines.push(
                        if zh {
                            "QQ 添加机器人 / 入群 URL（用手机 QQ 扫码）："
                        } else {
                            "QQ invitation URL (scan with mobile QQ):"
                        }
                        .into(),
                    );
                    lines.push(url.clone());
                    if let Ok(qr) = crate::channels::qr::text(url) {
                        lines.extend(qr);
                    }
                }
                if let Some(qr) = &s.qr_url {
                    lines.push(
                        if zh {
                            "微信扫码 URL（打开链接查看二维码）："
                        } else {
                            "WeChat QR URL (open to view QR code):"
                        }
                        .into(),
                    );
                    lines.push(qr.clone());
                }
            }
        }
        lines.push(String::new());
        let actions = if zh {
            vec![
                "微信：获取 / 刷新绑定二维码".into(),
                "微信：启动".into(),
                "微信：停止".into(),
                "微信：解绑（删除凭据）".into(),
                format!("QQ AppID: [{}]", self.app_id),
                format!(
                    "QQ AppSecret: [{}]（空白保留已保存密钥）",
                    "*".repeat(self.secret.chars().count())
                ),
                format!("QQ 机器人 QQ 号（邀请二维码）: [{}]", self.robot_uin),
                "QQ：保存并连接".into(),
                "QQ：启动".into(),
                "QQ：停止".into(),
                "QQ：解绑（删除凭据）".into(),
            ]
        } else {
            vec![
                "WeChat: get / refresh binding QR URL".into(),
                "WeChat: start".into(),
                "WeChat: stop".into(),
                "WeChat: unbind (delete credentials)".into(),
                format!("QQ AppID: [{}]", self.app_id),
                format!(
                    "QQ AppSecret: [{}] (blank preserves saved secret)",
                    "*".repeat(self.secret.chars().count())
                ),
                format!("QQ bot QQ number (invitation QR): [{}]", self.robot_uin),
                "QQ: save and connect".into(),
                "QQ: start".into(),
                "QQ: stop".into(),
                "QQ: unbind (delete credentials)".into(),
            ]
        };
        for (index, action) in actions.iter().enumerate() {
            lines.push(format!(
                "{} {action}",
                if index == self.selected { ">" } else { " " }
            ));
        }
        if let Some(error) = &self.error {
            lines.push(format!("⚠ {error}"));
        }
        if self.action.is_some() {
            lines.push(
                if zh {
                    "正在应用设置…"
                } else {
                    "Applying settings…"
                }
                .into(),
            );
        }
        PopupView{title:if zh{"渠道设置"}else{"Channel settings"}.into(),lines,footer:vec![if zh{"↑/↓ 选择；输入编辑 QQ 字段；Enter 执行；解绑按 Ctrl+D；PageUp/Down 滚动；Esc 关闭"}else{"Up/Down select; type QQ fields; Enter apply; Ctrl+D unbind; PageUp/Down scroll; Esc close"}.into(),if zh{"原会话：/approve 一次；/approve session 会话；/approve always 永久；/deny 拒绝；危险操作首次需 CONFIRM"}else{"Original chat: /approve once; /approve session; /approve always; /deny; first dangerous approval requires CONFIRM"}.into()],scroll:self.scroll,min_height:18,dangerous:false,informational:true}
    }
    pub fn key(&mut self, key: KeyEvent) -> bool {
        if key.code == KeyCode::Esc {
            return true;
        }
        if self.action.is_some() {
            return false;
        }
        match key.code {
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(5),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(5).min(100),
            KeyCode::Up => self.selected = (self.selected + 10) % 11,
            KeyCode::Down | KeyCode::Tab => self.selected = (self.selected + 1) % 11,
            KeyCode::Backspace => {
                if self.selected == 4 {
                    self.app_id.pop();
                    self.robot_uin.clear();
                } else if self.selected == 5 {
                    self.secret.pop();
                } else if self.selected == 6 {
                    self.robot_uin.pop();
                }
            }
            KeyCode::Char(ch) if matches!(self.selected, 4 | 5 | 6) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) && !ch.is_control() {
                    let value = if self.selected == 4 {
                        &mut self.app_id
                    } else if self.selected == 5 {
                        &mut self.secret
                    } else {
                        &mut self.robot_uin
                    };
                    if value.len() < 512 {
                        value.push(ch);
                    }
                    if self.selected == 4 {
                        self.robot_uin.clear();
                    }
                }
            }
            KeyCode::Enter | KeyCode::Char('d') => {
                let unbind =
                    key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL);
                let action = match self.selected {
                    0 => Some(ChannelAction::BindWeixin),
                    1 => Some(ChannelAction::Start {
                        kind: "weixin".into(),
                    }),
                    2 => Some(ChannelAction::Stop {
                        kind: "weixin".into(),
                    }),
                    3 if unbind => Some(ChannelAction::Forget {
                        kind: "weixin".into(),
                    }),
                    7 => Some(ChannelAction::ConfigureQq {
                        app_id: self.app_id.clone(),
                        secret: self.secret.clone(),
                        robot_uin: self.robot_uin.clone(),
                    }),
                    8 => Some(ChannelAction::Start { kind: "qq".into() }),
                    9 => Some(ChannelAction::Stop { kind: "qq".into() }),
                    10 if unbind => Some(ChannelAction::Forget { kind: "qq".into() }),
                    _ => None,
                };
                if let Some(action) = action {
                    let manager = self.manager.clone();
                    self.action = Some(tokio::spawn(async move { manager.apply(action).await }));
                }
            }
            _ => {}
        }
        false
    }
}
