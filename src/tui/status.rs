//! Local module overview. Readiness probes never authorize or install anything.
use super::app::PopupView;
use crate::{
    config::{Config, UiLanguage},
    runtime::RuntimeCapabilities,
    shell::ShellExecutor,
    tools::ToolRegistry,
};
use crossterm::event::{KeyCode, KeyEvent};
use std::time::{Duration, Instant};

pub(super) struct Viewer {
    config: Config,
    lines: Vec<String>,
    job: Option<tokio::task::JoinHandle<Vec<String>>>,
    updated: Option<Instant>,
    scroll: u16,
}
pub(super) struct Live<'a> {
    pub busy: bool,
    pub phase: &'a str,
    pub session: &'a str,
    pub turns: usize,
    pub run_permission: bool,
}
fn label<'a>(language: UiLanguage, zh: &'a str, en: &'a str) -> &'a str {
    if language == UiLanguage::ZhCn {
        zh
    } else {
        en
    }
}
fn safe(config: &Config, text: &str) -> String {
    let mut text = text.to_owned();
    for secret in [
        &config.api_key,
        &config.protocol_token,
        &config.proxy_password,
        &config.ima_client_id,
        &config.ima_api_key,
        &config.jev_api_key,
    ] {
        if !secret.is_empty() {
            text = text.replace(secret, "[REDACTED]");
        }
    }
    crate::limits::truncate_text(
        &text.chars().filter(|c| !c.is_control()).collect::<String>(),
        240,
    )
}
fn endpoint(config: &Config, raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(url) => safe(
            config,
            &format!(
                "{}://{}{}",
                url.scheme(),
                url.host_str().unwrap_or("?"),
                url.port().map(|p| format!(":{p}")).unwrap_or_default()
            ),
        ),
        Err(_) => label(
            config.ui_language,
            "未配置 / 地址无效",
            "not configured / invalid address",
        )
        .into(),
    }
}
impl Viewer {
    pub fn new(config: &Config) -> Self {
        let mut viewer = Self {
            config: config.clone(),
            lines: Vec::new(),
            job: None,
            updated: None,
            scroll: 0,
        };
        viewer.refresh();
        viewer
    }
    fn refresh(&mut self) {
        if self.job.is_none() {
            let config = self.config.clone();
            self.job = Some(tokio::spawn(collect(config)));
        }
    }
    pub async fn poll(&mut self) {
        if self.job.as_ref().is_some_and(|j| j.is_finished()) {
            if let Some(job) = self.job.take() {
                self.lines = job.await.unwrap_or_else(|_| {
                    vec![label(
                        self.config.ui_language,
                        "状态读取失败，按 R 重试",
                        "Status unavailable; press R to retry",
                    )
                    .into()]
                });
                self.updated = Some(Instant::now());
            }
        }
    }
    pub fn key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => return true,
            KeyCode::Char('r' | 'R') => self.refresh(),
            KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down => self.scroll = self.scroll.saturating_add(1).min(1000),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(8),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(8).min(1000),
            KeyCode::Home => self.scroll = 0,
            _ => {}
        }
        false
    }
    pub fn scroll(&mut self, down: bool) {
        self.scroll = if down {
            self.scroll.saturating_add(3).min(1000)
        } else {
            self.scroll.saturating_sub(3)
        };
    }
    pub fn view(&self, live: Live<'_>) -> PopupView {
        let cfg = &self.config;
        let l = cfg.ui_language;
        let mut lines = vec![
            format!(
                "nl2sh v{} · {} / {}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
            format!(
                "{}: {} · {}",
                label(l, "任务", "Task"),
                label(
                    l,
                    if live.busy { "运行中" } else { "空闲" },
                    if live.busy { "running" } else { "idle" }
                ),
                safe(cfg, live.phase)
            ),
            format!(
                "{}: {} · {} {}",
                label(l, "当前会话", "Current session"),
                safe(cfg, live.session),
                live.turns,
                label(l, "轮上下文", "context turns")
            ),
            format!(
                "{}: {:?} / {:?} · {:?} · {} {}",
                label(l, "安全与执行", "Safety / execution"),
                cfg.security_level,
                cfg.execute_confirm_policy,
                cfg.execute_user_mode,
                label(l, "普通修改运行期许可", "Run-wide mutation approval"),
                live.run_permission
            ),
            String::new(),
            format!(
                "{}: {} · {}",
                label(
                    l,
                    "模型配置（未做连通测试）",
                    "Model configuration (not a connectivity test)"
                ),
                safe(cfg, &cfg.model),
                if cfg.provider_is_configured() {
                    label(l, "已配置", "configured")
                } else {
                    label(l, "未配置", "not configured")
                }
            ),
            format!(
                "{}: {} · {:?}",
                label(l, "服务地址", "Provider origin"),
                endpoint(cfg, &cfg.endpoint),
                cfg.api_type
            ),
            format!(
                "{}: {} · {} {}",
                label(l, "代理", "Proxy"),
                cfg.proxy_enabled,
                label(l, "PTY", "PTY"),
                cfg.enable_pty
            ),
            format!(
                "{}: ima={} · Jev={}",
                label(
                    l,
                    "连接器配置（未做鉴权测试）",
                    "Connector configuration (authentication untested)"
                ),
                if cfg.ima_enabled && cfg.ima_is_configured() {
                    "configured"
                } else {
                    "disabled / incomplete"
                },
                if cfg.jev_api_key.is_empty() {
                    "not configured"
                } else {
                    "configured"
                }
            ),
            String::new(),
        ];
        lines.extend(self.lines.clone());
        if self.job.is_some() {
            lines.push(label(l, "正在读取模块状态…", "Reading module status…").into());
        }
        if let Some(updated) = self.updated {
            lines.push(format!(
                "{}: {}s",
                label(l, "快照距现在", "Snapshot age"),
                updated.elapsed().as_secs()
            ));
        }
        PopupView {
            title: label(l, "模块状态", "Module status").into(),
            lines,
            footer: vec![label(
                l,
                "↑/↓、PageUp/Down 或滚轮滚动；R 刷新；Enter / Esc 关闭",
                "Up/Down, PageUp/Down or wheel scroll; R refresh; Enter / Esc close",
            )
            .into()],
            scroll: self.scroll,
            min_height: 18,
            dangerous: false,
            informational: true,
        }
    }
}
impl Drop for Viewer {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            job.abort();
        }
    }
}
async fn collect(config: Config) -> Vec<String> {
    let l = config.ui_language;
    let path = config
        .source
        .clone()
        .unwrap_or_else(|| "config.toml".into());
    let executor = ShellExecutor::new(config.clone());
    let runtime = async {
        tokio::time::timeout(
            Duration::from_secs(8),
            RuntimeCapabilities::discover(&config, &executor),
        )
        .await
    };
    let protocol = async {
        tokio::time::timeout(
            Duration::from_secs(3),
            crate::protocol::connections::connection_info(&path),
        )
        .await
    };
    let channels = async {
        match crate::channels::ChannelManager::lookup(&path) {
            Some(manager) => tokio::time::timeout(Duration::from_secs(3), manager.snapshot())
                .await
                .ok()
                .and_then(Result::ok),
            None => None,
        }
    };
    let (runtime, protocol, channels) = tokio::join!(runtime, protocol, channels);
    let mut lines = Vec::new();
    match protocol {
        Ok(info) => {
            lines.push(format!(
                "MCP / A2A: {} · {}",
                info.state,
                info.transport.as_deref().unwrap_or("—")
            ));
            if let Some(url) = info.mcp_url {
                lines.push(format!("MCP: {}", safe(&config, &url)));
            }
            if let Some(url) = info.a2a_url {
                lines.push(format!("A2A: {}", safe(&config, &url)));
            }
        }
        Err(_) => lines.push("MCP / A2A: unknown (timeout)".into()),
    }
    lines.push(format!(
        "{}: {} · {}: {}",
        label(l, "协议随服务启动", "Protocol autostart"),
        config.protocol_start_with_service,
        label(l, "协议自动审批", "Protocol auto approval"),
        config.protocol_auto_approve
    ));
    lines.push(format!(
        "Web: {}",
        crate::web_ui::welcome_url()
            .map(|s| endpoint(&config, s))
            .unwrap_or_else(
                || label(l, "本进程无启动地址", "no startup address in this process").into()
            )
    ));
    if let Some(channels) = channels {
        for c in channels {
            lines.push(format!(
                "{}: {} · {}={} · {}={} / {}={} / {}={}",
                if c.kind == "weixin" {
                    "WeChat iLink"
                } else {
                    "QQ Bot"
                },
                safe(&config, &c.state),
                label(l, "启用", "enabled"),
                c.enabled,
                label(l, "收到", "received"),
                c.received,
                label(l, "回复", "sent"),
                c.sent,
                label(l, "待决", "pending"),
                c.pending
            ));
            if c.error.is_some() {
                lines.push(
                    label(
                        l,
                        "  存在错误；/channels 查看并管理",
                        "  Error present; inspect/manage with /channels",
                    )
                    .into(),
                );
            }
        }
    } else {
        lines.push(
            label(
                l,
                "微信 / QQ: 状态不可用；/channels 管理",
                "WeChat / QQ: status unavailable; manage with /channels",
            )
            .into(),
        );
    }
    lines.push(String::new());
    match runtime {
        Ok(r) => {
            let count = ToolRegistry::for_runtime(&config, &r).definitions().len();
            lines.push(format!(
                "{}: UID {} · Android={} · shell={} · root={}",
                label(l, "运行环境", "Runtime"),
                r.uid,
                r.android,
                r.android_shell,
                r.root
            ));
            lines.push(format!(
                "{}: {} · APK/JADX={} · Tailcat={}",
                label(l, "当前可用工具", "Available tools"),
                count,
                crate::tools::tool_enabled(&config, "inspect_apk"),
                crate::tools::tool_enabled(&config, "tailcat_check")
            ));
            lines.push(match r.bridge {Some(b)=>format!("Android Bridge: v{} · protocol={} · accessibility={} · IME={}",safe(&config,&b.app_version),b.protocol_version,b.services.accessibility,b.services.ime),None=>label(l,"Android Bridge: 未发现可用能力（受平台 / 身份限制）","Android Bridge: no available capabilities discovered (platform / UID dependent)").into()});
            lines.push(format!(
                "JADX: {}",
                r.jadx
                    .map(|j| format!(
                        "helper={} · core={}",
                        safe(&config, &j.helper_version),
                        safe(&config, &j.jadx_core)
                    ))
                    .unwrap_or_else(|| if r.jadx_provisionable {
                        label(
                            l,
                            "未就绪；可通过审批安装",
                            "not ready; install requires approval",
                        )
                        .into()
                    } else {
                        label(l, "未发现可用 helper", "no available helper discovered").into()
                    })
            ));
            lines.push(format!(
                "Tailcat: {}",
                r.tailcat
                    .map(|v| safe(&config, &v))
                    .unwrap_or_else(|| label(
                        l,
                        "未发现可用程序",
                        "no available executable discovered"
                    )
                    .into())
            ));
        }
        Err(_) => lines.push(
            label(
                l,
                "运行能力探测超时；Bridge / JADX / Tailcat 状态未知",
                "Runtime probes timed out; Bridge / JADX / Tailcat status unknown",
            )
            .into(),
        ),
    }
    lines.into_iter().map(|s| safe(&config, &s)).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overview_hides_credentials_and_url_userinfo_query_and_terminal_controls() {
        let cfg = Config {
            api_key: "private-key".into(),
            protocol_token: "protocol-private".into(),
            ..Config::default()
        };
        assert_eq!(
            endpoint(
                &cfg,
                "https://user:password@example.com:443/v1?key=hidden#hidden"
            ),
            "https://example.com"
        );
        let viewer = Viewer {
            config: cfg,
            lines: vec![],
            job: None,
            updated: None,
            scroll: 0,
        };
        let view = viewer.view(Live {
            busy: true,
            phase: "private-key\u{1b} protocol-private",
            session: "test",
            turns: 2,
            run_permission: false,
        });
        let text = view.lines.join("\n");
        assert!(!text.contains("private-key"));
        assert!(!text.contains("protocol-private"));
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("context") || text.contains("上下文"));
    }
}
