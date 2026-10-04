//! Approval-bound wireless ADB setup and narrowly scoped multi-port sharing.

use super::{listener, start};
use crate::{
    shell::CommandExecutor,
    tools::{
        android::{automation, companion},
        ui::domain::{inspect_android_ui, InspectAndroidUiArgs},
        PreparedExecution, PreparedToolCall, ToolContext, ToolOutput,
    },
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{net::IpAddr, path::PathBuf, time::Duration};

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Action {
    Setup,
    Share,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Args {
    /// setup opens Settings and the pairing dialog; share approves current ports and starts Tailcat.
    action: Action,
    /// Optional existing localhost Web service to share as well, e.g. 9999. Omitted by default.
    #[serde(default)]
    web_port: Option<u16>,
    /// Peer-local pairing listener used in the generated command. Default 13701.
    #[serde(default = "pair_local")]
    local_pair_port: u16,
    /// Peer-local ADB connection listener used in the generated command. Default 13702.
    #[serde(default = "connect_local")]
    local_connect_port: u16,
    /// Peer-local optional Web listener used in the generated command. Default 19999.
    #[serde(default = "web_local")]
    local_web_port: u16,
}
fn pair_local() -> u16 {
    13701
}
fn connect_local() -> u16 {
    13702
}
fn web_local() -> u16 {
    19999
}

impl Args {
    fn validate(&self) -> Result<()> {
        if matches!(self.action, Action::Setup) && self.web_port.is_some() {
            bail!("web_port is only accepted with action=share")
        }
        let mut ports = vec![self.local_pair_port, self.local_connect_port];
        if self.web_port.is_some() {
            ports.push(self.local_web_port);
        }
        if ports.iter().any(|p| *p == 0 || *p == 5037) || self.web_port == Some(0) {
            bail!("ports must be nonzero; peer listeners cannot use ADB server port 5037")
        }
        ports.sort_unstable();
        if ports.windows(2).any(|p| p[0] == p[1]) {
            bail!("peer-local listener ports must be distinct")
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Device {
    developer: bool,
    wireless: bool,
    connect_port: Option<u16>,
}

async fn command(executor: &dyn CommandExecutor, source: &str) -> Result<String> {
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        executor.execute_machine(source, false),
    )
    .await
    .context("wireless ADB command timed out")??;
    if result.exit_code != Some(0) || result.timed_out || result.interrupted {
        // Do not include raw stderr or UI output: it may contain a pairing code.
        bail!("wireless ADB command failed or was interrupted")
    }
    Ok(result.stdout)
}

async fn probe(executor: &dyn CommandExecutor) -> Result<Device> {
    let output = command(executor, "getprop ro.build.version.sdk; id -u; settings get global development_settings_enabled; settings get global adb_wifi_enabled; getprop service.adb.tls.port").await?;
    let values: Vec<_> = output.lines().map(str::trim).collect();
    let api = values
        .first()
        .context("Android API level unavailable")?
        .parse::<u32>()
        .context("tailcat_adb_pair requires Android 11+; host execution is unsupported")?;
    if api < 30 {
        bail!("wireless ADB pairing requires Android 11 / API 30 or newer")
    }
    if !matches!(values.get(1), Some(&"0") | Some(&"2000")) {
        bail!("tailcat_adb_pair requires Android shell or root UID; ordinary Termux UID is unsupported")
    }
    Ok(Device {
        developer: values.get(2) == Some(&"1"),
        wireless: values.get(3) == Some(&"1"),
        connect_port: values
            .get(4)
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|p| *p > 0),
    })
}

async fn snapshot(executor: &dyn CommandExecutor) -> Result<Value> {
    let value = match companion::screen_dump(executor).await {
        Ok(value) => value,
        Err(_) => serde_json::from_str(
            &inspect_android_ui(executor, &InspectAndroidUiArgs { full: true }).await?,
        )
        .context("wireless ADB UI snapshot could not be decoded")?,
    };
    trusted_nodes(&value)?;
    Ok(value)
}

const NETWORK_DIALOG: &[&str] = &[
    "Allow wireless debugging on this network?",
    "是否允许在此网络上进行无线调试？",
    "允许在此网络上进行无线调试？",
];

fn complete_nodes(value: &Value) -> Result<&Vec<Value>> {
    if value["status"] != "complete" || value["mode"] == "partial" || value["truncated"] == true {
        bail!("wireless ADB requires a complete current UI tree")
    }
    let nodes = value["nodes"]
        .as_array()
        .context("wireless ADB UI nodes unavailable")?;
    if nodes.is_empty() {
        bail!("wireless ADB UI tree is empty")
    }
    Ok(nodes)
}

fn settings_nodes(value: &Value) -> Result<&Vec<Value>> {
    let nodes = complete_nodes(value)?;
    if nodes.iter().any(|n| n["package"] != "com.android.settings") {
        bail!("wireless ADB pairing UI must belong exclusively to com.android.settings")
    }
    Ok(nodes)
}

fn trusted_nodes(value: &Value) -> Result<&Vec<Value>> {
    let nodes = complete_nodes(value)?;
    if nodes.iter().all(|n| n["package"] == "com.android.settings")
        || (nodes.iter().all(|n| n["package"] == "com.android.systemui")
            && has_text(value, NETWORK_DIALOG))
    {
        return Ok(nodes);
    }
    bail!("wireless ADB navigation requires Settings or the recognized SystemUI network approval dialog")
}

fn has_text(value: &Value, labels: &[&str]) -> bool {
    value["nodes"].as_array().is_some_and(|nodes| {
        nodes
            .iter()
            .any(|n| n["text"].as_str().is_some_and(|t| labels.contains(&t)))
    })
}

/// Pairing codes are returned intentionally, but never included in Debug or approval previews.
#[derive(Clone, PartialEq, Eq)]
struct Pairing {
    ip: IpAddr,
    port: u16,
    connect_port: u16,
    code_hash: [u8; 32],
    code: String,
}

fn pairing(value: &Value, device: &Device) -> Result<Pairing> {
    if !device.developer || !device.wireless {
        bail!("enable Developer options and Wireless debugging before sharing")
    }
    let connect_port = device
        .connect_port
        .context("wireless ADB connection port is not ready")?;
    let nodes = settings_nodes(value)?;
    if !has_text(
        value,
        &[
            "Pair with device",
            "与设备配对",
            "使用配对码配对设备",
            "Pairing code",
            "Wi-Fi pairing code",
            "Wi‑Fi pairing code",
            "WLAN 配对码",
            "Wi-Fi 配对码",
            "无线网络配对码",
        ],
    ) {
        bail!("open the current Wireless debugging pairing-code dialog before action=share")
    }
    let texts: Vec<_> = nodes.iter().filter_map(|n| n["text"].as_str()).collect();
    let codes: Vec<_> = texts
        .iter()
        .filter(|t| t.len() == 6 && t.bytes().all(|b| b.is_ascii_digit()))
        .collect();
    let endpoints: Vec<_> = texts
        .iter()
        .filter_map(|t| {
            let (host, port) = t.rsplit_once(':')?;
            let ip = host.trim_matches(['[', ']']).parse::<IpAddr>().ok()?;
            let port = port.parse::<u16>().ok()?;
            (!ip.is_unspecified() && !ip.is_multicast() && port > 0).then_some((ip, port))
        })
        .collect();
    if codes.len() != 1 || endpoints.len() != 1 {
        bail!("pairing dialog must contain exactly one six-digit code and one IP:port; unsupported or ambiguous UI")
    }
    let (ip, port) = endpoints[0];
    if port == connect_port {
        bail!("pairing and connection ports must be different")
    }
    Ok(Pairing {
        ip,
        port,
        connect_port,
        code_hash: Sha256::digest(codes[0].as_bytes()).into(),
        code: (*codes[0]).to_owned(),
    })
}

async fn read_pairing(executor: &dyn CommandExecutor, attempts: usize) -> Result<Pairing> {
    for attempt in 0..attempts {
        let device = probe(executor).await?;
        if let Ok(value) = snapshot(executor).await {
            if let Ok(pair) = pairing(&value, &device) {
                return Ok(pair);
            }
        }
        if attempt + 1 < attempts {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    bail!("current wireless pairing dialog could not be verified; keep it open and retry")
}

pub(super) async fn prepare(ctx: &ToolContext<'_>, args: Args) -> Result<PreparedToolCall> {
    args.validate()?;
    let executor = ctx.executor.context("Android executor unavailable")?;
    let device = probe(executor).await?;
    let binary = ctx
        .config
        .context("Tailcat configuration unavailable")?
        .tailcat_binary_path
        .clone();
    let approved = if matches!(args.action, Action::Share) {
        let mut guard = listener().lock().await;
        if let Some(active) = guard.as_mut() {
            if active.child.try_wait()?.is_none() {
                bail!("a managed Tailcat listener already exists; explicitly approve tailcat_stop first")
            }
        }
        let pair = read_pairing(executor, 3).await?;
        if args
            .web_port
            .is_some_and(|p| p == pair.port || p == pair.connect_port)
        {
            bail!("web_port must differ from both wireless ADB ports")
        }
        Some(pair)
    } else {
        None
    };
    let preview = match &approved {
        Some(p) => format!("Share Android wireless ADB pairing TCP {} and authenticated TLS connection TCP {}{} through {}. Peers can pair using the code displayed on this device and then obtain ADB shell access. Peer-local mappings: {}:{}, {}:{}{}. Recheck the current pairing dialog and ports after approval. Keep the pairing dialog open; the current pairing code will be returned to the model and conversation. Existing listeners are not stopped.", p.port, p.connect_port,
            args.web_port.map(|port| format!(" and Web TCP {port} (Web has no login)")).unwrap_or_default(), binary.display(), args.local_pair_port, p.port, args.local_connect_port, p.connect_port,
            args.web_port.map(|port| format!(", {}:{port}",args.local_web_port)).unwrap_or_default()),
        None => "Open Android Settings to guide Developer options and Wireless debugging. If Developer options are disabled, open About device and ask the user to enable them. Otherwise navigate supported English/Chinese Settings UI, enable Wireless debugging, approve this network once (never Always allow), and open Pair device with pairing code. UI targets are rechecked before each tap. No shell settings writes, credential entry, installation, remote pairing, or port sharing in this phase. Unsupported UI returns user instructions; settings changed before an interruption remain changed.".into(),
    };
    Ok(PreparedToolCall::operation(
        preview,
        Box::new(Operation {
            args,
            binary,
            device,
            approved,
        }),
    ))
}

struct Operation {
    args: Args,
    binary: PathBuf,
    device: Device,
    approved: Option<Pairing>,
}

// If a task is cancelled while post-start validation is awaiting UI, only its
// own new listener is stopped. A replacement from another task is preserved.
struct StartedListener {
    address: String,
    committed: bool,
}

async fn stop_started(address: &str) {
    let mut guard = listener().lock().await;
    if guard
        .as_ref()
        .is_some_and(|active| active.address == address)
    {
        if let Some(mut active) = guard.take() {
            let _ = active.child.start_kill();
            let _ = active.child.wait().await;
        }
    }
}

impl Drop for StartedListener {
    fn drop(&mut self) {
        if !self.committed {
            let address = self.address.clone();
            tokio::spawn(async move {
                stop_started(&address).await;
            });
        }
    }
}

#[async_trait]
impl PreparedExecution for Operation {
    async fn execute(self: Box<Self>, ctx: &mut ToolContext<'_>) -> Result<ToolOutput> {
        let executor = ctx.executor.context("Android executor unavailable")?;
        let now = probe(executor).await?;
        match self.args.action {
            Action::Setup => {
                if now != self.device {
                    bail!("wireless debugging state changed after approval; request setup again")
                }
                setup(executor, &now).await
            }
            Action::Share => {
                let expected = self
                    .approved
                    .context("approved wireless ADB ports missing")?;
                let current = pairing(&snapshot(executor).await?, &now)?;
                if current != expected {
                    bail!("pairing code, address, or ADB ports changed after approval; reopen pairing and request share again")
                }
                let mut ports = vec![current.port, current.connect_port];
                if let Some(port) = self.args.web_port {
                    ports.push(port);
                }
                // Verify localhost reachability without sending any pairing/ADB protocol bytes.
                for port in &ports {
                    tokio::time::timeout(
                        Duration::from_secs(2),
                        tokio::net::TcpStream::connect(("127.0.0.1", *port)),
                    )
                    .await
                    .with_context(|| format!("localhost service port {port} probe timed out; no Tailcat listener started"))?
                    .with_context(|| format!("shared service port {port} is not reachable on localhost; no Tailcat listener started"))?;
                }
                // Network probes and approval may outlive the dialog. Never start from stale evidence.
                if pairing(&snapshot(executor).await?, &probe(executor).await?)? != expected {
                    bail!("pairing state changed during localhost probes; no Tailcat listener started")
                }
                let port_list = ports
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                let content = start(
                    &self.binary,
                    vec!["serve".into(), port_list.clone()],
                    format!(
                        "wireless ADB pairing={} connect={}{}",
                        current.port,
                        current.connect_port,
                        self.args
                            .web_port
                            .map(|p| format!(" web={p}"))
                            .unwrap_or_default()
                    ),
                    None,
                )
                .await?;
                let address = content
                    .lines()
                    .find_map(|l| l.strip_prefix("address="))
                    .context("Tailcat address unavailable")?;
                let mut started = StartedListener {
                    address: address.into(),
                    committed: false,
                };
                let still_current = match probe(executor).await {
                    Ok(device) => match snapshot(executor).await {
                        Ok(value) => pairing(&value, &device).is_ok_and(|p| p == expected),
                        Err(_) => false,
                    },
                    Err(_) => false,
                };
                if !still_current {
                    stop_started(address).await;
                    started.committed = true;
                    bail!("pairing state changed while Tailcat started; the new listener was stopped; request setup/share again")
                }
                let output =
                    serde_json::to_string_pretty(&commands(&self.args, &current, address))?;
                started.committed = true;
                Ok(ToolOutput::success(output))
            }
        }
    }
}

fn commands(args: &Args, p: &Pairing, address: &str) -> Value {
    let mut forward = format!(
        "tailcat forward {address} {}:{} {}:{}",
        args.local_pair_port, p.port, args.local_connect_port, p.connect_port
    );
    if let Some(port) = args.web_port {
        forward.push_str(&format!(" {}:{port}", args.local_web_port));
    }
    json!({
        "status":"sharing", "address":address,
        "pairing_code":p.code, "pairing_port":p.port, "connect_port":p.connect_port, "web_port":args.web_port,
        "peer_commands": {"forward":forward,
            "pair":format!("adb pair 127.0.0.1:{}",args.local_pair_port),
            "connect":format!("adb connect 127.0.0.1:{}",args.local_connect_port),
            "verify":format!("adb -s 127.0.0.1:{} shell getprop ro.build.version.sdk",args.local_connect_port)},
        "web_url": args.web_port.map(|_| format!("http://127.0.0.1:{}/",args.local_web_port)),
        "instructions":"On the peer, install Tailcat and Android SDK Platform Tools supporting adb pair. Keep forward running; execute the other commands in another terminal. Enter the returned current six-digit pairing code at the adb pair prompt; the code is also visible on the device screen. This result contains pairing credentials and is sent to the model/conversation. Keep the pairing dialog open until pairing completes. Pairing does not automatically establish a tunneled ADB connection: run connect explicitly. Dynamic ports may change when debugging or Wi-Fi restarts; rerun setup/share with fresh approval. tailcat_status inspects and tailcat_stop stops this listener; stopping it does not disable wireless debugging or revoke already paired computers. Only give this address to the intended peer."
    })
}

fn guidance(step: &str, instructions: &str, changed: bool) -> Result<ToolOutput> {
    Ok(ToolOutput::success(serde_json::to_string_pretty(
        &json!({"status":"needs_user_action", "step":step, "instructions":instructions, "settings_may_have_changed":changed, "next_action":"setup", "ports_shared":false}),
    )?))
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(400)).await;
}

async fn tap(executor: &dyn CommandExecutor, value: &Value, labels: &[&str]) -> Result<bool> {
    let nodes = settings_nodes(value)?;
    // Settings may repeat a row's label on its switch. Select the TextView label only.
    let candidates: Vec<_> = nodes
        .iter()
        .filter(|n| {
            n["class"] == "android.widget.TextView"
                && n["text"].as_str().is_some_and(|t| labels.contains(&t))
        })
        .collect();
    if candidates.is_empty() {
        return Ok(false);
    }
    if candidates.len() != 1 {
        bail!("Settings target is ambiguous; use the device UI manually")
    }
    let expected = candidates[0];
    execute_tap(executor, expected).await?;
    Ok(true)
}

async fn execute_tap(executor: &dyn CommandExecutor, expected: &Value) -> Result<()> {
    let bounds = expected["bounds"]
        .as_str()
        .context("Settings target bounds unavailable")?;
    let current = automation::find_unique_node(executor, None, Some(bounds)).await?;
    for field in [
        "package",
        "class",
        "resource_id",
        "text",
        "content_description",
        "text_hash",
        "description_hash",
        "bounds",
    ] {
        if current[field] != expected[field] {
            bail!("Settings target changed before tap")
        }
    }
    let args = automation::UiArgs {
        bounds: Some(bounds.into()),
        ..Default::default()
    };
    let (source, verified) = automation::prepare_tap("android.tap_node", &args, executor).await?;
    if verified != current {
        bail!("Settings target changed before execution")
    }
    command(executor, &source).await?;
    settle().await;
    Ok(())
}

async fn setup(executor: &dyn CommandExecutor, device: &Device) -> Result<ToolOutput> {
    if !device.developer {
        command(
            executor,
            "am start -W -a android.settings.DEVICE_INFO_SETTINGS -p com.android.settings",
        )
        .await?;
        return guidance("developer_options", "On the device, find Build number in About phone/device and tap it seven times. Enter your device credential yourself if requested. Then call tailcat_adb_pair with action=setup again. No debugging ports have been shared.", false);
    }
    // An already open pairing dialog must not be dismissed or regenerate its code.
    if device.wireless {
        if let Ok(value) = snapshot(executor).await {
            if let Ok(p) = pairing(&value, device) {
                return ready(&p);
            }
        }
    }
    command(
        executor,
        "am start -W -a android.settings.APPLICATION_DEVELOPMENT_SETTINGS -p com.android.settings",
    )
    .await?;
    settle().await;
    match navigate(executor).await {
        Ok(output) => Ok(output),
        // Never expose raw hierarchy/errors, which can include the pairing secret.
        Err(_) => guidance("wireless_debugging", "Settings navigation could not be verified. On the device, open Developer options > Wireless debugging, enable it and approve the current Wi-Fi network, then choose Pair device with pairing code. Keep the dialog open and call action=share. You can inspect the device screen yourself. Setup may already have enabled wireless debugging; no ports have been shared.", true),
    }
}

async fn navigate(executor: &dyn CommandExecutor) -> Result<ToolOutput> {
    let wifi = ["Wireless debugging", "无线调试"];
    for index in 0..=6 {
        let value = snapshot(executor).await?;
        if has_text(&value, NETWORK_DIALOG) {
            if !tap_button(executor, &value, &["Allow", "允许"]).await? {
                bail!("network approval unavailable")
            }
            continue;
        }
        if has_text(
            &value,
            &[
                "Use wireless debugging",
                "使用无线调试",
                "Pair device with pairing code",
                "使用配对码配对设备",
            ],
        ) {
            settings_nodes(&value)?;
            break;
        }
        if tap(executor, &value, &wifi).await? {
            break;
        }
        if index == 6 {
            bail!("wireless debugging row not found")
        }
        let source =
            automation::prepare("android.scroll", &automation::UiArgs::default(), executor).await?;
        // Scrolling is restricted to a freshly verified Settings tree.
        settings_nodes(&snapshot(executor).await?)?;
        command(executor, &source).await?;
        settle().await;
    }
    let mut device = probe(executor).await?;
    if !device.wireless {
        let value = snapshot(executor).await?;
        if !tap(
            executor,
            &value,
            &["Use wireless debugging", "使用无线调试", "无线调试"],
        )
        .await?
        {
            bail!("wireless debugging toggle not found")
        }
        let value = snapshot(executor).await?;
        if has_text(&value, NETWORK_DIALOG) {
            // Button targets are separately checked; never set Always allow on this network.
            if !tap_button(executor, &value, &["Allow", "允许"]).await? {
                bail!("network approval unavailable")
            }
        }
        device = probe(executor).await?;
        if !device.wireless {
            bail!("wireless debugging has not enabled")
        }
    }
    let value = snapshot(executor).await?;
    if !tap(
        executor,
        &value,
        &[
            "Pair device with pairing code",
            "使用配对码配对设备",
            "使用配对码配对",
        ],
    )
    .await?
    {
        bail!("pairing code row unavailable")
    }
    ready(&read_pairing(executor, 3).await?)
}

async fn tap_button(
    executor: &dyn CommandExecutor,
    value: &Value,
    labels: &[&str],
) -> Result<bool> {
    let candidates: Vec<_> = trusted_nodes(value)?
        .iter()
        .filter(|node| {
            node["class"] == "android.widget.Button"
                && node["text"]
                    .as_str()
                    .is_some_and(|text| labels.contains(&text))
        })
        .collect();
    if candidates.is_empty() {
        return Ok(false);
    }
    if candidates.len() != 1 {
        bail!("network approval target is ambiguous")
    }
    execute_tap(executor, candidates[0]).await?;
    Ok(true)
}

fn ready(p: &Pairing) -> Result<ToolOutput> {
    Ok(ToolOutput::success(serde_json::to_string_pretty(
        &json!({"status":"ready_to_share", "pairing_code":p.code, "pairing_port":p.port, "connect_port":p.connect_port,
        "ports_shared":false, "next_action":"share", "instructions":"Keep the pairing-code dialog open. The six-digit pairing code is returned to the model and conversation. Call tailcat_adb_pair with action=share; optionally set web_port=9999 only if that service exists. Strong confirmation will show the actual ports and local mappings. Setup may have enabled wireless debugging; it will remain enabled until you turn it off."}),
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::{ConfirmationDecision, Confirmer},
        config::Config,
        security::{RiskLevel, SecurityAssessment},
        shell::ExecutionResult,
        tools::runtime::invoke,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };

    fn device() -> Device {
        Device {
            developer: true,
            wireless: true,
            connect_port: Some(42817),
        }
    }
    fn tree() -> Value {
        json!({"status":"complete", "mode":"full", "truncated":false, "nodes":[
            {"package":"com.android.settings", "text":"Pair with device", "class":"android.widget.TextView"},
            {"package":"com.android.settings", "text":"123456", "class":"android.widget.TextView"},
            {"package":"com.android.settings", "text":"10.0.2.16:37123", "class":"android.widget.TextView"}
        ]})
    }
    fn args(value: Value) -> Result<Args> {
        Ok(serde_json::from_value(value)?)
    }

    #[test]
    fn rejects_ambiguous_partial_foreign_and_disabled_pairing() -> Result<()> {
        let mut value = tree();
        assert_eq!(pairing(&value, &device())?.port, 37123);
        value["mode"] = json!("partial");
        assert!(pairing(&value, &device()).is_err());
        value = tree();
        value["nodes"][1]["package"] = json!("com.example.fake");
        assert!(pairing(&value, &device()).is_err());
        value = tree();
        value["nodes"]
            .as_array_mut()
            .context("nodes")?
            .push(json!({"package":"com.android.settings","text":"654321"}));
        assert!(pairing(&value, &device()).is_err());
        let mut off = device();
        off.wireless = false;
        assert!(pairing(&tree(), &off).is_err());
        let mut same = device();
        same.connect_port = Some(37123);
        assert!(pairing(&tree(), &same).is_err());
        value = tree();
        value["nodes"][2]["text"] = json!("10.0.2.16:0");
        assert!(pairing(&value, &device()).is_err());
        Ok(())
    }

    #[test]
    fn only_recognized_network_dialog_can_use_systemui() -> Result<()> {
        let mut value = tree();
        for node in value["nodes"].as_array_mut().context("nodes")? {
            node["package"] = json!("com.android.systemui");
        }
        assert!(trusted_nodes(&value).is_err());
        value["nodes"][0]["text"] = json!(NETWORK_DIALOG[0]);
        assert!(trusted_nodes(&value).is_ok());
        assert!(pairing(&value, &device()).is_err());
        value["nodes"][1]["package"] = json!("com.example.fake");
        assert!(trusted_nodes(&value).is_err());
        Ok(())
    }

    #[test]
    fn approval_binding_detects_rotated_code_and_ports() -> Result<()> {
        let original = pairing(&tree(), &device())?;
        let mut changed = tree();
        changed["nodes"][1]["text"] = json!("654321");
        assert!(pairing(&changed, &device())? != original);
        changed = tree();
        changed["nodes"][2]["text"] = json!("10.0.2.16:37124");
        assert!(pairing(&changed, &device())? != original);
        let mut connection = device();
        connection.connect_port = Some(42818);
        assert!(pairing(&tree(), &connection)? != original);
        Ok(())
    }

    #[test]
    fn mappings_return_code_without_putting_it_in_command_arguments() -> Result<()> {
        let args = args(json!({"action":"share","web_port":9999}))?;
        args.validate()?;
        let p = pairing(&tree(), &device())?;
        let out = commands(&args, &p, "tcEXAMPLEADDRESS012345");
        assert_eq!(out["pairing_code"], "123456");
        assert_eq!(
            out["peer_commands"]["forward"],
            "tailcat forward tcEXAMPLEADDRESS012345 13701:37123 13702:42817 19999:9999"
        );
        assert_eq!(out["peer_commands"]["pair"], "adb pair 127.0.0.1:13701");
        assert!(!out["peer_commands"].to_string().contains("123456"));
        assert_eq!(out["web_url"], "http://127.0.0.1:19999/");
        assert_eq!(
            serde_json::from_str::<Value>(&ready(&p)?.content)?["pairing_code"],
            "123456"
        );
        for bad in [
            json!({"action":"share","local_pair_port":0}),
            json!({"action":"share","local_pair_port":5037}),
            json!({"action":"share","local_connect_port":13701}),
            json!({"action":"setup","web_port":9999}),
        ] {
            assert!(args_for_validation(bad)?.validate().is_err());
        }
        assert!(args_for_validation(json!({"action":"share","pairing_port":5555})).is_err());
        Ok(())
    }
    fn args_for_validation(v: Value) -> Result<Args> {
        args(v)
    }

    struct Fixture {
        state: Mutex<String>,
        calls: Mutex<Vec<String>>,
    }
    impl Fixture {
        fn new() -> Self {
            Self {
                state: Mutex::new("35\n2000\n1\n1\n42817\n".into()),
                calls: Mutex::new(vec![]),
            }
        }
    }
    #[async_trait]
    impl CommandExecutor for Fixture {
        async fn execute(&self, source: &str, _: bool, _: bool) -> Result<ExecutionResult> {
            self.calls
                .lock()
                .map_err(|_| anyhow::anyhow!("calls lock"))?
                .push(source.into());
            let stdout = if source.starts_with("getprop ro.build.version.sdk;") {
                self.state
                    .lock()
                    .map_err(|_| anyhow::anyhow!("state lock"))?
                    .clone()
            } else if source.starts_with("p=/data/local/tmp/.nl2sh-ui-") {
                "<hierarchy><node package=\"com.android.settings\" class=\"android.widget.TextView\" text=\"Pair with device\"/><node package=\"com.android.settings\" class=\"android.widget.TextView\" text=\"123456\"/><node package=\"com.android.settings\" class=\"android.widget.TextView\" text=\"10.0.2.16:37123\"/></hierarchy>\n---NL2SH_WINDOW---\nSettings\n---NL2SH_DISPLAY---\n1080x2400".into()
            } else if source.starts_with("content call") {
                return Ok(ExecutionResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: Some(1),
                    timed_out: false,
                    interrupted: false,
                });
            } else {
                bail!("unexpected write or probe: {source}")
            };
            Ok(ExecutionResult {
                stdout,
                stderr: String::new(),
                exit_code: Some(0),
                timed_out: false,
                interrupted: false,
            })
        }
    }
    struct Reject(AtomicUsize);
    #[async_trait]
    impl Confirmer for Reject {
        async fn confirm(
            &self,
            preview: &str,
            assessment: &SecurityAssessment,
        ) -> Result<ConfirmationDecision> {
            assert_eq!(assessment.risk_level, RiskLevel::Dangerous);
            assert!(!preview.contains("123456"));
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ConfirmationDecision::Reject)
        }
    }
    fn config() -> Config {
        let mut c = Config::default();
        c.tool_groups.insert("tailcat".into(), true);
        c
    }

    #[tokio::test]
    async fn rejecting_setup_or_share_executes_no_actions() -> Result<()> {
        let f = Fixture::new();
        let reject = Reject(AtomicUsize::new(0));
        for action in ["setup", "share"] {
            let result = invoke(
                &config(),
                &f,
                &reject,
                "tailcat_adb_pair",
                json!({"action":action}),
            )
            .await?;
            assert!(!result.success);
        }
        assert_eq!(reject.0.load(Ordering::SeqCst), 2);
        assert!(f
            .calls
            .lock()
            .map_err(|_| anyhow::anyhow!("calls lock"))?
            .iter()
            .all(|s| s.starts_with("getprop")
                || s.starts_with("content call")
                || s.starts_with("p=/data/local/tmp/.nl2sh-ui-")));
        Ok(())
    }

    struct Rotate<'a>(&'a Fixture);
    #[async_trait]
    impl Confirmer for Rotate<'_> {
        async fn confirm(&self, _: &str, _: &SecurityAssessment) -> Result<ConfirmationDecision> {
            *self
                .0
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("state lock"))? = "35\n2000\n1\n1\n42818\n".into();
            Ok(ConfirmationDecision::Approve)
        }
    }
    #[tokio::test]
    async fn changed_connection_after_approval_never_starts_listener() -> Result<()> {
        let f = Fixture::new();
        let result = invoke(
            &config(),
            &f,
            &Rotate(&f),
            "tailcat_adb_pair",
            json!({"action":"share"}),
        )
        .await;
        assert!(result.is_err_and(|e| e.to_string().contains("changed after approval")));
        Ok(())
    }

    #[tokio::test]
    async fn unsupported_api_or_uid_never_reaches_confirmation() -> Result<()> {
        let f = Fixture::new();
        let reject = Reject(AtomicUsize::new(0));
        for state in ["29\n2000\n1\n0\n\n", "35\n10123\n1\n0\n\n"] {
            *f.state.lock().map_err(|_| anyhow::anyhow!("state lock"))? = state.into();
            assert!(invoke(
                &config(),
                &f,
                &reject,
                "tailcat_adb_pair",
                json!({"action":"setup"})
            )
            .await
            .is_err());
        }
        assert_eq!(reject.0.load(Ordering::SeqCst), 0);
        Ok(())
    }
}
