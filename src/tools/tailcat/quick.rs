//! Model-free Tailcat wizard shared by the browser and terminal.
use crate::{
    agent::{ConfirmationDecision, Confirmer},
    config::Config,
    security::SecurityAssessment,
    shell::ShellExecutor,
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Default, Serialize)]
/// Volatile wizard state; never enters model history.
pub struct Snapshot {
    /// Unique wizard identity.
    pub id: u64,
    /// Whether the wizard is still running.
    pub busy: bool,
    /// Current stage or final result.
    pub status: String,
    /// Actual received archive bytes.
    pub downloaded: u64,
    /// Reported archive size, if known.
    pub total: Option<u64>,
    /// Bounded tool results and local guidance.
    pub messages: Vec<String>,
    /// Peer commands generated from verified tool results.
    pub commands: Vec<String>,
}
#[derive(Default)]
struct State {
    snapshot: Snapshot,
    sequence: u64,
}
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}
/// Read current process-local wizard status.
pub fn snapshot() -> Result<Snapshot> {
    Ok(state()
        .lock()
        .map_err(|_| anyhow::anyhow!("Tailcat wizard lock poisoned"))?
        .snapshot
        .clone())
}
fn update(f: impl FnOnce(&mut Snapshot)) {
    if let Ok(mut s) = state().lock() {
        f(&mut s.snapshot);
    }
}
tokio::task_local! { static PROGRESS: bool; }
/// Report real download bytes only to the active wizard task.
pub(super) fn progress(bytes: u64, total: Option<u64>) {
    if PROGRESS.try_with(|v| *v).unwrap_or(false) {
        update(|s| {
            s.downloaded = bytes;
            s.total = total;
        });
    }
}
pub(super) fn phase(status: &str) {
    if PROGRESS.try_with(|v| *v).unwrap_or(false) {
        update(|s| s.status = status.into());
    }
}
const QUICK_TOOLS: &[&str] = &[
    "tailcat_check",
    "tailcat_install",
    "tailcat_stop",
    "tailcat_serve",
];

// Selecting ports and starting this local wizard authorizes its fixed operations.
// This private confirmer is never passed to Agent, bridge, or general tool callers.
struct UserInitiatedConfirmer;
#[async_trait]
impl Confirmer for UserInitiatedConfirmer {
    fn audit_source(&self) -> &'static str {
        "tailcat_wizard"
    }
    async fn confirm(
        &self,
        _preview: &str,
        _assessment: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        Ok(ConfirmationDecision::Approve)
    }
}
/// Begin a single wizard, explicitly enabling only its tools in a temporary config snapshot.
pub fn begin(mut cfg: Config, web_port: Option<u16>, adb_port: Option<u16>) -> Result<Snapshot> {
    if web_port.is_none() && adb_port.is_none() {
        bail!("Select at least one port")
    }
    if web_port == Some(0) || adb_port == Some(0) {
        bail!("Invalid port")
    }
    {
        let mut s = state()
            .lock()
            .map_err(|_| anyhow::anyhow!("Tailcat wizard lock poisoned"))?;
        if s.snapshot.busy {
            bail!("Tailcat wizard is already running")
        }
        s.sequence += 1;
        s.snapshot = Snapshot {
            id: s.sequence,
            busy: true,
            status: "检查 Tailcat / Checking Tailcat".into(),
            ..Snapshot::default()
        };
    }
    for name in QUICK_TOOLS {
        cfg.tool_overrides.insert((*name).into(), true);
    }
    cfg.enable_pty = false;
    tokio::spawn(PROGRESS.scope(true, async move {
        let result = run(&cfg, web_port, adb_port).await;
        update(|s| {
            s.busy = false;
            match result {
                Ok(()) => s.status = "完成 / Completed".into(),
                Err(e) => s.status = format!("未完成 / Incomplete: {e:#}"),
            }
        });
    }));
    snapshot()
}
async fn call(cfg: &Config, name: &str, args: Value) -> Result<String> {
    if !QUICK_TOOLS.contains(&name) {
        bail!("unsupported Tailcat shortcut operation {name}")
    }
    update(|s| s.status = format!("正在执行 / Running {name}"));
    let executor = ShellExecutor::new(cfg.clone());
    let r =
        crate::tools::runtime::invoke(cfg, &executor, &UserInitiatedConfirmer, name, args).await?;
    if !r.success {
        bail!("{}", r.output)
    }
    Ok(r.output)
}
/// Default ADB TCP port when device discovery is unavailable.
pub const DEFAULT_ADB_PORT: u16 = 5555;

/// Read existing Android ADB port properties; never enable debugging or open Settings.
pub async fn detect_adb_port(cfg: &Config) -> u16 {
    #[cfg(target_os = "android")]
    {
        use crate::shell::CommandExecutor;
        let mut cfg = cfg.clone();
        cfg.enable_pty = false;
        cfg.execute_user_mode = crate::config::ExecuteUserMode::Normal;
        cfg.execute_timeout_secs = 3;
        let executor = ShellExecutor::new(cfg);
        if let Ok(Ok(output)) = tokio::time::timeout(std::time::Duration::from_secs(4), executor.execute_machine(
            "getprop service.adb.tls.port; getprop service.adb.tcp.port; getprop persist.adb.tcp.port", false
        )).await {
            if output.exit_code == Some(0) && !output.timed_out && !output.interrupted {
                return parsed_adb_port(&output.stdout);
            }
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = cfg;
    DEFAULT_ADB_PORT
}
#[cfg(any(target_os = "android", test))]
fn parsed_adb_port(properties: &str) -> u16 {
    properties
        .lines()
        .find_map(|line| line.trim().parse::<u16>().ok().filter(|port| *port > 0))
        .unwrap_or(DEFAULT_ADB_PORT)
}

async fn run(cfg: &Config, web_port: Option<u16>, adb_port: Option<u16>) -> Result<()> {
    if !cfg.tailcat_binary_path.exists() {
        call(cfg, "tailcat_install", json!({})).await?;
    }
    call(cfg, "tailcat_check", json!({})).await?;
    let ports: Vec<_> = web_port.into_iter().chain(adb_port).collect();
    let port = ports.first().context("No ports selected")?;
    // User confirmation authorizes replacing this process's managed listener.
    // Stop is idempotent and waits for the old child before starting a new one.
    call(cfg, "tailcat_stop", json!({})).await?;
    let output = call(
        cfg,
        "tailcat_serve",
        json!({"port":port,"additional_ports":&ports[1..]}),
    )
    .await?;
    let address = output
        .lines()
        .find_map(|l| l.strip_prefix("address="))
        .context("Missing Tailcat address")?;
    let mut forward = format!("tailcat forward {address}");
    if let Some(port) = web_port {
        forward.push_str(&format!(" 19999:{port}"));
    }
    if let Some(port) = adb_port {
        forward.push_str(&format!(" 13702:{port}"));
    }
    update(|s| {
        s.commands.push(forward);
        if adb_port.is_some() {
            s.commands.push("adb connect 127.0.0.1:13702".into());
        }
        if web_port.is_some() {
            s.messages.push("Web: http://127.0.0.1:19999/".into());
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::RiskLevel;

    #[test]
    fn adb_discovery_prefers_tls_then_tcp_and_defaults_to_5555() {
        assert_eq!(parsed_adb_port("37123\n5555\n5556"), 37123);
        assert_eq!(parsed_adb_port("-1\n4567\n"), 4567);
        for value in ["", "0\n-1", "invalid\n65536"] {
            assert_eq!(parsed_adb_port(value), 5555);
        }
    }

    #[tokio::test]
    async fn user_initiated_wizard_runs_without_further_confirmation_and_scopes_progress(
    ) -> Result<()> {
        assert!(begin(Config::default(), None, None).is_err());
        for risk in [RiskLevel::Mutating, RiskLevel::Dangerous] {
            let assessment = SecurityAssessment::from_policy(
                risk,
                vec![],
                false,
                "Tailcat shortcut".into(),
                true,
            );
            assert_eq!(
                UserInitiatedConfirmer
                    .confirm("explicitly selected Tailcat operation", &assessment)
                    .await?,
                ConfirmationDecision::Approve
            );
        }
        assert!(call(
            &Config::default(),
            "execute_shell_command",
            json!({"command":"id"})
        )
        .await
        .is_err());
        progress(10, Some(20));
        assert_eq!(snapshot()?.downloaded, 0);
        PROGRESS
            .scope(true, async {
                progress(10, Some(20));
            })
            .await;
        assert_eq!(snapshot()?.downloaded, 10);

        // The normal runtime still prepares and validates an explicitly started wizard.
        // A relative installation path fails locally, without any download or approval wait.
        let cfg = Config {
            tailcat_binary_path: std::path::PathBuf::from("relative-tailcat-install-target"),
            ..Config::default()
        };
        let original = cfg.tool_overrides.clone();
        begin(cfg.clone(), Some(9999), None)?;
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while snapshot()?.busy {
                tokio::task::yield_now().await;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await??;
        assert_eq!(cfg.tool_overrides, original);
        assert!(snapshot()?.status.contains("must be absolute"));
        assert!(!serde_json::to_value(snapshot()?)?
            .as_object()
            .context("missing wizard snapshot")?
            .contains_key("pending"));
        Ok(())
    }
}
