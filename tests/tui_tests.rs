#![cfg(target_os = "linux")]

use nix::{
    fcntl::{fcntl, FcntlArg, OFlag},
    pty::{openpty, Winsize},
};
use serde_json::json;
use std::{
    fs::File,
    io::{ErrorKind, Read, Write},
    os::{
        fd::{FromRawFd, IntoRawFd},
        unix::process::CommandExt,
    },
    process::Stdio,
    time::Duration,
};
use tempfile::tempdir;
use tokio::{
    process::Command,
    time::{sleep, timeout, Instant},
};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

struct PtyChild {
    master: File,
    child: tokio::process::Child,
}

#[tokio::test]
async fn agent_reply_remains_in_live_tui_until_ctrl_q() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "output": [{
                "type": "message",
                "content": [{"type": "output_text", "text": "tui-e2e-done"}]
            }]
        })))
        .mount(&server)
        .await;

    let directory = tempdir()?;
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "api_key=''\nmodel='test'\nendpoint='{}/v1'\napi_type='responses'\n",
            server.uri()
        ),
    )?;

    let pair = openpty(
        Some(&Winsize {
            ws_row: 30,
            ws_col: 100,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }),
        None,
    )?;
    let raw_master = pair.master.into_raw_fd();
    let flags = OFlag::from_bits_truncate(fcntl(raw_master, FcntlArg::F_GETFL)?);
    fcntl(raw_master, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
    let mut master = unsafe { File::from_raw_fd(raw_master) };
    let slave = File::from(pair.slave);
    let stdin = slave.try_clone()?;
    let stdout = slave.try_clone()?;

    let mut child = Command::new(env!("CARGO_BIN_EXE_nl2sh"))
        .arg("--config")
        .arg(&config)
        .env("TERM", "xterm-256color")
        .env("NL2SH_TEST_DISABLE_KONKA_BOOTSTRAP", "1")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(slave))
        .spawn()?;

    wait_for_text(&mut master, "Ctrl+Q", Duration::from_secs(3)).await?;
    master.write_all(b"/help\r")?;
    sleep(Duration::from_millis(100)).await;
    master.write_all(b"/clear\r")?;
    sleep(Duration::from_millis(100)).await;
    master.write_all(b"show status\r")?;
    wait_for_text(&mut master, "tui-e2e-done", Duration::from_secs(5)).await?;
    assert!(
        child.try_wait()?.is_none(),
        "TUI exited after one Agent response"
    );

    master.write_all(&[0x11])?;
    let status = timeout(Duration::from_secs(3), child.wait()).await??;
    assert!(status.success());
    let log = std::fs::read_to_string(directory.path().join("nl2sh.log"))?;
    assert!(log.contains("show status"));
    assert!(log.contains("tui-e2e-done"));
    assert!(log.contains("local_command"));
    assert!(log.contains("/help"));
    assert!(log.contains("/clear"));
    Ok(())
}

#[tokio::test]
async fn missing_config_enters_tui_and_config_command_runs_setup() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let config = directory.path().join("missing.toml");
    let mut process = spawn_tui(&config)?;

    let initial =
        wait_for_text_capture(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;
    assert!(!initial.contains("界面语言"));
    assert!(!initial.contains("API Key"));
    process.master.write_all(b"diagnose device\r")?;
    sleep(Duration::from_millis(100)).await;
    process.master.write_all(b"/config\r")?;
    wait_for_text(&mut process.master, "界面语言", Duration::from_secs(3)).await?;
    process.master.write_all(b"\r")?;
    wait_for_text(
        &mut process.master,
        "选择 API 服务商",
        Duration::from_secs(3),
    )
    .await?;
    // Select the built-in Ollama endpoint.
    process.master.write_all(b"\x1b[B\x1b[B\x1b[B\x1b[B\r")?;
    wait_for_text(&mut process.master, "API Key", Duration::from_secs(3)).await?;
    // API Key uses normal echoed input, then accept the remaining defaults.
    process.master.write_all(b"visible-test-key\r")?;
    let echoed = wait_for_text_capture(
        &mut process.master,
        "visible-test-key",
        Duration::from_secs(3),
    )
    .await?;
    assert!(echoed.contains("visible-test-key"));
    process.master.write_all(b"\r\r\r\r")?;
    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;

    let loaded = nl2sh::config::load_from(&config)?;
    assert_eq!(loaded.endpoint, "http://127.0.0.1:11434/v1");
    assert_eq!(loaded.api_key, "visible-test-key");
    assert!(process.child.try_wait()?.is_none());
    process.master.write_all(b"/exit\r")?;
    assert!(timeout(Duration::from_secs(3), process.child.wait())
        .await??
        .success());
    assert!(std::fs::read_to_string(directory.path().join("nl2sh.log"))?.contains("/exit"));
    Ok(())
}

#[tokio::test]
async fn model_command_can_create_partial_config_without_startup_wizard() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let config = directory.path().join("model-only.toml");
    let mut process = spawn_tui(&config)?;

    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;
    process.master.write_all(b"/model\r")?;
    wait_for_text(&mut process.master, "模型", Duration::from_secs(3)).await?;
    process.master.write_all(b"model-from-tui\r")?;
    wait_for_text(
        &mut process.master,
        "model-from-tui",
        Duration::from_secs(3),
    )
    .await?;

    let loaded = nl2sh::config::load_unvalidated(Some(&config))?;
    assert_eq!(loaded.model, "model-from-tui");
    assert!(std::fs::read_to_string(&config)?.contains("api_key = \"\""));
    process.master.write_all(&[0x11])?;
    assert!(timeout(Duration::from_secs(3), process.child.wait())
        .await??
        .success());
    Ok(())
}

#[tokio::test]
async fn slash_config_reconfigures_and_returns_to_tui() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        "api_key='existing-key'\nmodel='before-config'\nendpoint='http://127.0.0.1:9999/v1'\napi_type='responses'\n",
    )?;
    let mut process = spawn_tui(&config)?;
    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;
    process.master.write_all(b"/config\r")?;
    wait_for_text(&mut process.master, "界面语言", Duration::from_secs(3)).await?;
    process.master.write_all(b"\r")?;
    wait_for_text(
        &mut process.master,
        "选择 API 服务商",
        Duration::from_secs(3),
    )
    .await?;
    process.master.write_all(b"\r")?;
    wait_for_text(
        &mut process.master,
        "自定义 API Base URL",
        Duration::from_secs(3),
    )
    .await?;
    process.master.write_all(b"\r")?;
    wait_for_text(&mut process.master, "API Key", Duration::from_secs(3)).await?;
    process.master.write_all(b"\r")?;
    wait_for_text(&mut process.master, "模型", Duration::from_secs(3)).await?;
    process.master.write_all(b"reconfigured-model\r")?;
    wait_for_text(&mut process.master, "API 类型", Duration::from_secs(3)).await?;
    process.master.write_all(b"\r")?;
    wait_for_text(
        &mut process.master,
        "reconfigured-model",
        Duration::from_secs(3),
    )
    .await?;

    let loaded = nl2sh::config::load_from(&config)?;
    assert_eq!(loaded.model, "reconfigured-model");
    assert_eq!(loaded.endpoint, "http://127.0.0.1:9999/v1");
    assert_eq!(loaded.api_key, "existing-key");
    assert!(process.child.try_wait()?.is_none());
    process.master.write_all(&[0x11])?;
    assert!(timeout(Duration::from_secs(3), process.child.wait())
        .await??
        .success());
    Ok(())
}

fn spawn_tui(config: &std::path::Path) -> anyhow::Result<PtyChild> {
    let pair = openpty(
        Some(&Winsize {
            ws_row: 30,
            ws_col: 100,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }),
        None,
    )?;
    let raw_master = pair.master.into_raw_fd();
    let flags = OFlag::from_bits_truncate(fcntl(raw_master, FcntlArg::F_GETFL)?);
    fcntl(raw_master, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
    let master = unsafe { File::from_raw_fd(raw_master) };
    let slave = File::from(pair.slave);
    let stdin = slave.try_clone()?;
    let stdout = slave.try_clone()?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_nl2sh"));
    command
        .arg("--config")
        .arg(config)
        .env("TERM", "xterm-256color")
        .env("NL2SH_TEST_DISABLE_KONKA_BOOTSTRAP", "1")
        .env_remove("NL2SH_API_KEY")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(slave));
    unsafe {
        command.as_std_mut().pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn()?;
    Ok(PtyChild { master, child })
}

async fn wait_for_text_capture(
    master: &mut File,
    needle: &str,
    limit: Duration,
) -> anyhow::Result<String> {
    let deadline = Instant::now() + limit;
    let mut captured = String::new();
    let mut buffer = [0_u8; 8192];
    loop {
        match master.read(&mut buffer) {
            Ok(0) => {}
            Ok(count) => captured.push_str(&String::from_utf8_lossy(&buffer[..count])),
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(error) if error.raw_os_error() == Some(libc::EIO) => {}
            Err(error) => return Err(error.into()),
        }
        if captured.contains(needle) {
            return Ok(captured);
        }
        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for {needle:?}; captured {captured:?}")
        }
        sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_for_text(master: &mut File, needle: &str, limit: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + limit;
    let mut captured = String::new();
    let mut buffer = [0_u8; 8192];
    loop {
        match master.read(&mut buffer) {
            Ok(0) => {}
            Ok(count) => captured.push_str(&String::from_utf8_lossy(&buffer[..count])),
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(error) if error.raw_os_error() == Some(libc::EIO) => {}
            Err(error) => return Err(error.into()),
        }
        if captured.contains(needle) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for {needle:?}; captured {captured:?}")
        }
        sleep(Duration::from_millis(20)).await;
    }
}
