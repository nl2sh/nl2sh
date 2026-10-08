use super::app::PopupView;
use crate::{config::Config, tools::tailcat::quick};
use anyhow::Result;
use base64::Engine;
use crossterm::event::KeyCode;
use std::io::Write;

pub(super) struct Wizard {
    web: bool,
    adb: bool,
    started: bool,
    port: Option<u16>,
    scroll: u16,
    notice: String,
}
impl Wizard {
    pub fn new() -> Self {
        Self {
            web: true,
            adb: true,
            started: quick::snapshot().map(|s| s.id != 0).unwrap_or(false),
            port: crate::web::welcome_url()
                .and_then(|u| u.trim_end_matches('/').rsplit(':').next()?.parse().ok()),
            scroll: 0,
            notice: String::new(),
        }
    }
    pub fn view(&self) -> PopupView {
        let mut lines = vec![
            "通过 Tailcat 共享本设备端口 / Share device ports via Tailcat".into(),
            "无需 LLM。只向可信对端分享地址 / No LLM. Share only with trusted peers.".into(),
            "确定后直接安装和共享，不再安全确认；ADB 需要 Android 11+ shell/root。".into(),
            format!(
                "[{}] Web {}    [ {} ] ADB 配对与连接 / pairing + connect",
                if self.web { "x" } else { " " },
                self.port
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "未启动 / unavailable".into()),
                if self.adb { "x" } else { " " }
            ),
        ];
        let mut footer = vec!["W: Web  A: ADB  Enter: 确定 / Start  Esc: 关闭 / Close".into()];
        if self.started {
            if let Ok(s) = quick::snapshot() {
                lines.push(s.status);
                if s.downloaded > 0 {
                    lines.push(format!(
                        "下载 / Download: {} / {} bytes",
                        s.downloaded,
                        s.total.map(|n| n.to_string()).unwrap_or_else(|| "?".into())
                    ));
                }
                lines.extend(s.messages);
                for (i, c) in s.commands.iter().enumerate() {
                    lines.push(format!("⧉ [{}] {c}", i + 1));
                }
                footer = vec![
                    "1–4 / C: ⧉ 复制命令 (OSC 52) / Copy commands".into(),
                    "R: 重试 / Retry  PgUp/PgDn: 滚动 / Scroll  Esc: 关闭 / Close".into(),
                ];
            }
        }
        if !self.notice.is_empty() {
            lines.push(self.notice.clone());
        }
        PopupView {
            title: "Tailcat".into(),
            lines,
            footer,
            scroll: self.scroll,
            min_height: 14,
            dangerous: false,
            informational: true,
        }
    }
    pub fn key(&mut self, code: KeyCode, cfg: &Config) -> Result<bool> {
        match code {
            KeyCode::Esc => return Ok(true),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(8),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(8),
            KeyCode::Char('w' | 'W') if !self.started => self.web = !self.web,
            KeyCode::Char('a' | 'A') if !self.started => self.adb = !self.adb,
            KeyCode::Enter if !self.started => {
                if self.web && self.port.is_none() {
                    self.notice="Web 服务未启动；按 W 取消 Web 选项 / Web is unavailable; press W to deselect".into();
                    return Ok(false);
                }
                match quick::begin(
                    cfg.clone(),
                    if self.web { self.port } else { None },
                    self.adb,
                ) {
                    Ok(_) => self.started = true,
                    Err(e) => self.notice = e.to_string(),
                }
            }
            KeyCode::Char('r' | 'R') if !quick::snapshot()?.busy => {
                self.started = false;
                self.scroll = 0;
            }
            KeyCode::Char(c) if matches!(c, 'c' | 'C') || ('1'..='4').contains(&c) => {
                let s = quick::snapshot()?;
                let value = if matches!(c, 'c' | 'C') {
                    s.commands.join("\n")
                } else {
                    s.commands
                        .get(c as usize - '1' as usize)
                        .cloned()
                        .unwrap_or_default()
                };
                if !value.is_empty() {
                    let encoded = base64::engine::general_purpose::STANDARD.encode(value);
                    let mut stdout = std::io::stdout();
                    write!(stdout, "\x1b]52;c;{encoded}\x07")?;
                    stdout.flush()?;
                    self.notice="已发送复制请求；需终端支持 OSC 52 / Clipboard request sent; terminal must support OSC 52".into();
                }
            }
            _ => {}
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn port_selection_remains_local_and_unavailable_web_is_not_silently_omitted() -> Result<()> {
        let mut wizard = Wizard {
            web: true,
            adb: true,
            started: false,
            port: None,
            scroll: 0,
            notice: String::new(),
        };
        let cfg = Config::default();
        assert!(!wizard.key(KeyCode::Enter, &cfg)?);
        assert!(!wizard.started);
        assert!(wizard.notice.contains("unavailable"));
        wizard.key(KeyCode::Char('w'), &cfg)?;
        wizard.key(KeyCode::Char('a'), &cfg)?;
        assert!(!wizard.web && !wizard.adb);
        assert!(wizard.key(KeyCode::Esc, &cfg)?);
        Ok(())
    }
}
