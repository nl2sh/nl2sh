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
    "tailcat_serve",
    "tailcat_adb_pair",
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
        preview: &str,
        _assessment: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        update(|s| {
            s.messages
                .push(preview.replace("the model and conversation", "this local dialog (no LLM)"));
        });
        Ok(ConfirmationDecision::Approve)
    }
}
/// Begin a single wizard, explicitly enabling only its tools in a temporary config snapshot.
pub fn begin(mut cfg: Config, web_port: Option<u16>, adb: bool) -> Result<Snapshot> {
    if web_port.is_none() && !adb {
        bail!("Select at least one port")
    }
    if web_port == Some(0) {
        bail!("Invalid Web port")
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
        let result = run(&cfg, web_port, adb).await;
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
async fn run(cfg: &Config, web_port: Option<u16>, adb: bool) -> Result<()> {
    // Only absence triggers installation; a broken existing binary must be diagnosed.
    if !cfg.tailcat_binary_path.exists() {
        let output = call(cfg, "tailcat_install", json!({})).await?;
        update(|s| s.messages.push(output));
    }
    let checked = call(cfg, "tailcat_check", json!({})).await?;
    update(|s| s.messages.push(checked));
    if adb {
        let output = call(cfg, "tailcat_adb_pair", json!({"action":"setup"})).await?;
        let value: Value = serde_json::from_str(&output)?;
        update(|s| {
            if value["status"] == "ready_to_share" {
                s.messages.push(format!(
                    "ADB 配对端口 / Pairing port: {}；连接端口 / Connection port: {}",
                    value["pairing_port"], value["connect_port"]
                ));
            } else {
                s.messages.push(output);
            }
        });
        if value["status"] != "ready_to_share" {
            bail!("请按提示完成设备设置后重试 / Complete device setup and retry")
        }
        let output = call(
            cfg,
            "tailcat_adb_pair",
            json!({"action":"share","web_port":web_port}),
        )
        .await?;
        let value: Value = serde_json::from_str(&output)?;
        let commands = value["peer_commands"]
            .as_object()
            .context("Missing peer commands")?;
        update(|s| {
            for key in ["forward", "pair", "connect", "verify"] {
                if let Some(command) = commands.get(key).and_then(Value::as_str) {
                    s.commands.push(command.into());
                }
            }
            s.messages
                .push(format!("配对码 / Pairing code: {}", value["pairing_code"]));
            s.messages.push("保持设备配对窗口和对端 forward 命令运行；在另一终端执行 adb pair 并输入配对码，再执行 adb connect。关闭弹窗不会停止共享或关闭无线调试。 / Keep device pairing dialog and peer forward running; run adb pair with the code, then adb connect in another terminal. Closing this dialog does not stop sharing or wireless debugging.".into());
            if let Some(url) = value["web_url"].as_str() {
                s.messages.push(format!("Web: {url}"));
            }
        });
    } else if let Some(port) = web_port {
        let output = call(cfg, "tailcat_serve", json!({"port":port})).await?;
        let address = output
            .lines()
            .find_map(|l| l.strip_prefix("address="))
            .context("Missing Tailcat address")?;
        let command = format!("tailcat forward {address} 19999:{port}");
        update(|s| {
            s.commands.push(command);
            s.messages.push(output);
            s.messages.push("Web: http://127.0.0.1:19999/".into());
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::RiskLevel;

    #[tokio::test]
    async fn user_initiated_wizard_runs_without_further_confirmation_and_scopes_progress(
    ) -> Result<()> {
        assert!(begin(Config::default(), None, false).is_err());
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
        begin(cfg.clone(), Some(9999), false)?;
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
