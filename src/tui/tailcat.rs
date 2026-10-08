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
    adb_port: u16,
    port_edit: String,
    editing_port: bool,
    replace_port: bool,
    port_edited: bool,
    detection: Option<tokio::sync::oneshot::Receiver<u16>>,
    language: crate::config::UiLanguage,
}
impl Wizard {
    pub fn new(cfg: &Config) -> Self {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let detection_cfg = cfg.clone();
        tokio::spawn(async move {
            let _ = tx.send(quick::detect_adb_port(&detection_cfg).await);
        });
        Self {
            web: true,
            adb: true,
            started: quick::snapshot().map(|s| s.id != 0).unwrap_or(false),
            port: crate::web::welcome_url()
                .and_then(|u| u.trim_end_matches('/').rsplit(':').next()?.parse().ok()),
            scroll: 0,
            notice: String::new(),
            adb_port: quick::DEFAULT_ADB_PORT,
            port_edit: String::new(),
            editing_port: false,
            replace_port: false,
            port_edited: false,
            detection: Some(rx),
            language: cfg.ui_language,
        }
    }
    fn text(&self, zh: &str, en: &str) -> String {
        match self.language {
            crate::config::UiLanguage::ZhCn => zh,
            crate::config::UiLanguage::En => en,
        }
        .into()
    }
    pub fn poll(&mut self) {
        if let Some(rx) = self.detection.as_mut() {
            match rx.try_recv() {
                Ok(port) => {
                    if !self.port_edited && !self.started {
                        self.adb_port = port;
                    }
                    self.detection = None;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => self.detection = None,
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
        }
    }
    pub fn view(&self) -> PopupView {
        let mut lines = vec![
            self.text(
                "共享端口，地址仅交给可信对端。",
                "Share ports only with trusted peers.",
            ),
            format!(
                "[{}] Web {}",
                if self.web { "x" } else { " " },
                self.port
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| self.text("未启动", "unavailable"))
            ),
            format!(
                "[{}] ADB {}{}",
                if self.adb { "x" } else { " " },
                if self.editing_port {
                    self.port_edit.clone()
                } else {
                    self.adb_port.to_string()
                },
                if self.editing_port { "▏" } else { "" }
            ),
        ];
        let mut footer = vec![self.text(
            "W/A: 选择  P: 改端口  Enter: 开始  Esc: 关闭",
            "W/A: Toggle  P: Edit port  Enter: Start  Esc: Close",
        )];
        if self.editing_port {
            footer = vec![self.text(
                "输入端口 · Enter 保存 · Esc 取消",
                "Type port · Enter save · Esc cancel",
            )];
        }
        if self.started {
            if let Ok(s) = quick::snapshot() {
                lines.push(s.status);
                if s.downloaded > 0 {
                    lines.push(format!(
                        "{}: {} / {} bytes",
                        self.text("下载", "Download"),
                        s.downloaded,
                        s.total.map(|n| n.to_string()).unwrap_or_else(|| "?".into())
                    ));
                }
                lines.extend(s.messages);
                for (i, c) in s.commands.iter().enumerate() {
                    lines.push(format!("⧉ [{}] {c}", i + 1));
                }
                footer = vec![self.text(
                    "1–2/C: 复制  R: 重试  PgUp/PgDn: 滚动  Esc: 关闭",
                    "1–2/C: Copy  R: Retry  PgUp/PgDn: Scroll  Esc: Close",
                )];
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
            min_height: 10,
            dangerous: false,
            informational: true,
        }
    }
    pub fn key(&mut self, code: KeyCode, cfg: &Config) -> Result<bool> {
        if self.editing_port {
            match code {
                KeyCode::Esc => self.editing_port = false,
                KeyCode::Enter => {
                    if let Ok(port) = self.port_edit.parse::<u16>() {
                        if port > 0 {
                            self.adb_port = port;
                            self.editing_port = false;
                            self.notice.clear();
                            return Ok(false);
                        }
                    }
                    self.notice = self.text("端口范围：1–65535", "Port range: 1–65535");
                }
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    if self.replace_port {
                        self.port_edit.clear();
                        self.replace_port = false;
                    }
                    if self.port_edit.len() < 5 {
                        self.port_edit.push(c);
                    }
                }
                KeyCode::Backspace => {
                    if self.replace_port {
                        self.port_edit.clear();
                        self.replace_port = false;
                    } else {
                        self.port_edit.pop();
                    }
                }
                KeyCode::Delete => {
                    self.port_edit.clear();
                    self.replace_port = false;
                }
                _ => {}
            }
            return Ok(false);
        }
        match code {
            KeyCode::Char('p' | 'P') | KeyCode::Tab if !self.started => {
                self.editing_port = true;
                self.port_edited = true;
                self.replace_port = true;
                self.port_edit = self.adb_port.to_string();
                self.notice.clear();
            }
            KeyCode::Esc => return Ok(true),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(8),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(8),
            KeyCode::Char('w' | 'W') if !self.started => self.web = !self.web,
            KeyCode::Char('a' | 'A') if !self.started => self.adb = !self.adb,
            KeyCode::Enter if !self.started => {
                if self.web && self.port.is_none() {
                    self.notice = self.text(
                        "Web 未启动，按 W 取消选择",
                        "Web unavailable; press W to deselect",
                    );
                    return Ok(false);
                }
                match quick::begin(
                    cfg.clone(),
                    if self.web { self.port } else { None },
                    self.adb.then_some(self.adb_port),
                ) {
                    Ok(_) => self.started = true,
                    Err(e) => self.notice = e.to_string(),
                }
            }
            KeyCode::Char('r' | 'R') if !quick::snapshot()?.busy => {
                self.started = false;
                self.scroll = 0;
            }
            KeyCode::Char(c) if matches!(c, 'c' | 'C') || ('1'..='2').contains(&c) => {
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
                    self.notice = self.text(
                        "已发送复制请求（需终端支持）",
                        "Copy requested (terminal support required)",
                    );
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
    fn cfg_language() -> crate::config::UiLanguage {
        crate::config::UiLanguage::ZhCn
    }
    #[test]
    fn port_selection_remains_local_and_unavailable_web_is_not_silently_omitted() -> Result<()> {
        let mut wizard = Wizard {
            web: true,
            adb: true,
            started: false,
            port: None,
            scroll: 0,
            notice: String::new(),
            adb_port: 5555,
            port_edit: String::new(),
            editing_port: false,
            replace_port: false,
            port_edited: false,
            detection: None,
            language: cfg_language(),
        };
        let cfg = Config::default();
        assert!(!wizard.key(KeyCode::Enter, &cfg)?);
        assert!(!wizard.started);
        assert!(wizard.notice.contains("Web"));
        wizard.key(KeyCode::Char('p'), &cfg)?;
        for c in "4567".chars() {
            wizard.key(KeyCode::Char(c), &cfg)?;
        }
        wizard.key(KeyCode::Enter, &cfg)?;
        assert_eq!(wizard.adb_port, 4567);
        assert!(!wizard.started);
        wizard.key(KeyCode::Char('w'), &cfg)?;
        wizard.key(KeyCode::Char('a'), &cfg)?;
        assert!(!wizard.web && !wizard.adb);
        assert!(wizard.key(KeyCode::Esc, &cfg)?);
        Ok(())
    }
}
