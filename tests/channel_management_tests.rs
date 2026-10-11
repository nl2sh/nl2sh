//! Real Web-process channel ownership and same-UID management from a second UI.
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{process::Stdio, time::Duration};
use tokio::{
    process::Command,
    time::{sleep, timeout},
};

#[tokio::test]
async fn two_web_processes_share_channel_management_without_taking_over_owner() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        "show_train_ascii_art=false\nshow_buddha_ascii_art=false\n",
    )?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_nl2sh"))
        .args(["--config"])
        .arg(&config)
        .arg("--web-only")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut lines = BufReader::new(child.stdout.take().context("stdout missing")?).lines();
    let announced = timeout(Duration::from_secs(10), lines.next_line())
        .await??
        .context("web startup output missing")?;
    let url = announced
        .strip_prefix("Web UI: ")
        .context("unexpected startup output")?;
    let port = url::Url::parse(url)?.port().context("web port missing")?;
    let endpoint = format!("http://127.0.0.1:{port}/api/channels");
    let client = reqwest::Client::new();
    let state = client.get(&endpoint).send().await?.error_for_status()?;
    assert_eq!(
        state
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("no-store")
    );
    let state: Value = state.json().await?;
    assert_eq!(state["channels"].as_array().map(Vec::len), Some(2));
    assert!(state["channels"]
        .as_array()
        .context("channels missing")?
        .iter()
        .all(|c| c["state"] == "stopped" && c.get("secret").is_none()));
    let second = nl2sh::web_ui::start_on_port(config.clone(), 0, true).await?;
    let second_endpoint = format!("http://127.0.0.1:{}/api/channels", second.port());
    let state: Value = client
        .get(&second_endpoint)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(state["channels"].as_array().map(Vec::len), Some(2));
    let result = client
        .post(&second_endpoint)
        .json(&json!({"action":"start","kind":"qq"}))
        .send()
        .await?;
    assert_eq!(result.status(), 400);
    client
        .post(&second_endpoint)
        .json(&json!({"action":"stop","kind":"weixin"}))
        .send()
        .await?
        .error_for_status()?;
    second.shutdown().await?;
    assert!(child.try_wait()?.is_none());
    assert!(client.get(&endpoint).send().await?.status().is_success());
    let pid = child.id().context("child PID missing")?;
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid as i32),
        nix::sys::signal::Signal::SIGINT,
    )?;
    timeout(Duration::from_secs(5), child.wait()).await??;
    // Once the original owner exits, another process can claim the state.
    sleep(Duration::from_millis(50)).await;
    let fresh = nl2sh::web_ui::start_on_port(config, 0, true).await?;
    fresh.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn configured_qq_invitation_renders_locally_without_exposing_credentials() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        "show_train_ascii_art=false\nshow_buddha_ascii_art=false\n",
    )?;
    let root = nl2sh::config::state_dir(&config)?.join("channels");
    std::fs::create_dir_all(&root)?;
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    let accounts = root.join("accounts.json");
    std::fs::write(
        &accounts,
        serde_json::to_vec(&json!({
            "qq":{"id":"102719228","secret":"never-expose-this-secret","base_url":"https://api.bot.qq.com","enabled":false},
            "qq_uin":"3889625032"
        }))?,
    )?;
    std::fs::set_permissions(&accounts, std::fs::Permissions::from_mode(0o600))?;
    let web = nl2sh::web_ui::start_on_port(config, 0, true).await?;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", web.port());
    let status = client
        .get(format!("{base}/api/channels"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    assert!(!status.contains("never-expose-this-secret"));
    assert!(status.contains("robot_appid=102719228&robot_uin=3889625032"));
    let qr = client
        .get(format!("{base}/api/channels/qq/qr.svg"))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(
        qr.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("image/svg+xml")
    );
    assert_eq!(
        qr.headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("no-store")
    );
    let svg = qr.text().await?;
    assert!(svg.starts_with("<svg ") && svg.contains("fill=\"#fff\""));
    assert!(!svg.contains("never-expose-this-secret") && !svg.contains("https://"));
    web.shutdown().await?;
    Ok(())
}
