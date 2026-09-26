#![cfg(target_os = "linux")]

use nix::{
    fcntl::{fcntl, FcntlArg, OFlag},
    pty::{openpty, Winsize},
};
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
    matchers::{body_string_contains, method, path},
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
        .and(body_string_contains("Generate a concise title"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "output": [{"type": "message", "content": [{"type": "output_text", "text": "TUI 自动标题"}]}]
        })))
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(concat!(
                    "data: {\"type\":\"response.output_text.delta\",\"delta\":\"tui-e2e-done\"}\n\n",
                    "data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"tui-e2e-done\"}]}]}}\n\n"
                )),
        )
        .with_priority(10)
        .mount(&server)
        .await;

    let directory = tempdir()?;
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "api_key=''\nmodel='test'\nendpoint='{}/v1'\napi_type='responses'\nshow_buddha_ascii_art=false\nshow_train_ascii_art=false\n",
            server.uri()
        ),
    )?;

    let mut process = spawn_tui(&config)?;

    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;
    process.master.write_all(b"show status\r")?;
    wait_for_text(&mut process.master, "tui-e2e-done", Duration::from_secs(5)).await?;
    let store = nl2sh::sessions::SessionStore::open(&config)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if store
            .list()?
            .iter()
            .any(|session| session.title == "TUI 自动标题")
        {
            break;
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "TUI session title was not generated"
        );
        sleep(Duration::from_millis(50)).await;
    }
    process.master.write_all(b"show status again\r")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if store
            .list()?
            .iter()
            .any(|session| session.turns == 2 && session.title == "TUI 自动标题")
        {
            break;
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "TUI title was not kept after another turn"
        );
        sleep(Duration::from_millis(50)).await;
    }
    assert!(
        process.child.try_wait()?.is_none(),
        "TUI exited after one Agent response"
    );

    process.master.write_all(&[0x11])?;
    let status = timeout(Duration::from_secs(3), process.child.wait()).await??;
    assert!(status.success());
    let log = std::fs::read_to_string(directory.path().join("nl2sh.log"))?;
    assert!(log.contains("show status"));
    assert!(log.contains("tui-e2e-done"));
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
    process.master.write_all(b"/config\r")?;
    wait_for_text(&mut process.master, "Ctrl+S", Duration::from_secs(3)).await?;
    process.master.write_all(&[0x13])?;
    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;

    let loaded = nl2sh::config::load_unvalidated(Some(&config))?;
    assert_eq!(loaded.max_agent_steps, 50);
    assert_eq!(loaded.max_context_turns, 16);
    assert!(process.child.try_wait()?.is_none());
    process.master.write_all(b"/exit\r")?;
    assert!(timeout(Duration::from_secs(3), process.child.wait())
        .await??
        .success());
    assert!(std::fs::read_to_string(directory.path().join("nl2sh.log"))?.contains("/exit"));
    Ok(())
}

#[tokio::test]
async fn slash_shell_runs_commands_and_exit_restores_tui() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let config = directory.path().join("missing.toml");
    std::fs::write(&config, "enable_pty=false\n")?;
    let mut process = spawn_tui(&config)?;

    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;
    process.master.write_all(b"/shell\r")?;
    sleep(Duration::from_millis(150)).await;
    process
        .master
        .write_all(b"printf 'shell-mode-ok\\n'\rexit\r")?;
    wait_for_text(&mut process.master, "shell-mode-ok", Duration::from_secs(3)).await?;
    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;

    assert!(process.child.try_wait()?.is_none());
    let log = std::fs::read_to_string(directory.path().join("nl2sh.log"))?;
    assert!(log.contains("/shell"));
    assert!(!log.contains("shell-mode-ok"));
    process.master.write_all(&[0x11])?;
    assert!(timeout(Duration::from_secs(3), process.child.wait())
        .await??
        .success());
    Ok(())
}

#[tokio::test]
async fn bang_command_runs_without_a_configured_provider_and_stays_in_tui() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let config = directory.path().join("missing.toml");
    std::fs::write(&config, "enable_pty=false\n")?;
    let mut process = spawn_tui(&config)?;

    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;
    process.master.write_all(b"!printf 'bang-direct-ok\\n'\r")?;
    wait_for_text(
        &mut process.master,
        "bang-direct-ok",
        Duration::from_secs(3),
    )
    .await?;
    sleep(Duration::from_millis(200)).await;

    assert!(process.child.try_wait()?.is_none());
    process.master.write_all(&[0x11])?;
    assert!(timeout(Duration::from_secs(3), process.child.wait())
        .await??
        .success());
    let log = std::fs::read_to_string(directory.path().join("nl2sh.log"))?;
    assert!(log.contains("direct_command_requested"));
    assert!(log.contains("direct_command_result"));
    assert!(log.contains("exit=Some(0)"));
    assert!(log.contains("bang-direct-ok"));
    assert!(!log.contains("local_rejection"));
    Ok(())
}

#[tokio::test]
async fn setting_alias_can_create_partial_config_without_startup_wizard() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let config = directory.path().join("model-only.toml");
    let mut process = spawn_tui(&config)?;

    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;
    process.master.write_all(b"/setting\r")?;
    process.master.write_all(b"\t")?;
    wait_for_text(&mut process.master, "Ctrl+S", Duration::from_secs(3)).await?;
    process
        .master
        .write_all(&vec![0x7f; nl2sh::config::Config::default().model.len()])?;
    process.master.write_all(b"model-from-tui")?;
    process.master.write_all(&[0x13])?;
    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;

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
    wait_for_text(&mut process.master, "Ctrl+S", Duration::from_secs(3)).await?;
    process.master.write_all(b"\t")?;
    process.master.write_all(&[0x7f; 13])?;
    process.master.write_all(b"reconfigured-model")?;
    process.master.write_all(&[0x13])?;
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
    process.master.write_all(b"/not-a-command\r")?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let log = std::fs::read_to_string(directory.path().join("nl2sh.log"))?;
    assert!(log.contains("local_command"));
    assert!(log.contains("unknown_local_command"));
    assert!(!log
        .lines()
        .any(|line| line.contains("\"kind\":\"user\"") && line.contains("/config")));
    assert!(!log
        .lines()
        .any(|line| line.contains("\"kind\":\"user\"") && line.contains("/not-a-command")));
    assert!(process.child.try_wait()?.is_none());
    process.master.write_all(&[0x11])?;
    assert!(timeout(Duration::from_secs(3), process.child.wait())
        .await??
        .success());
    Ok(())
}

#[tokio::test]
async fn new_session_and_local_command_typo_stay_local() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        "show_buddha_ascii_art=false\nshow_train_ascii_art=false\n",
    )?;
    let mut process = spawn_tui(&config)?;
    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;

    process.master.write_all(b"/new\r")?;
    sleep(Duration::from_millis(200)).await;
    process.master.write_all(b"/confiig\r")?;
    sleep(Duration::from_millis(200)).await;

    let log = std::fs::read_to_string(directory.path().join("nl2sh.log"))?;
    assert!(log.contains("/new"));
    assert!(log.contains("unknown_local_command"));
    assert!(!log
        .lines()
        .any(|line| line.contains("\"kind\":\"user\"") && line.contains("/confiig")));
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
