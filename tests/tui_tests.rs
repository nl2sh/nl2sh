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
async fn settings_tools_save_and_return_to_tui() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        "endpoint='http://127.0.0.1:9999/v1'\nui_language='en'\nshow_train_ascii_art=false\nshow_buddha_ascii_art=false\n",
    )?;
    let mut process = spawn_tui(&config)?;
    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;
    process.master.write_all(b"/config\r")?;
    wait_for_text(&mut process.master, "Ctrl+S", Duration::from_secs(3)).await?;
    process.master.write_all(b"\t\t\t\t\t\t")?;
    wait_for_text(&mut process.master, "APK/JADX", Duration::from_secs(3)).await?;
    process.master.write_all(b" ")?;
    process.master.write_all(&[0x13])?;
    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;
    let loaded = nl2sh::config::load_from(&config)?;
    assert_eq!(loaded.tool_groups.get("jadx"), Some(&true));
    assert!(nl2sh::tools::tool_enabled(&loaded, "inspect_apk"));
    assert!(!nl2sh::tools::tool_enabled(&loaded, "tailcat_check"));
    process.master.write_all(b"/config\r")?;
    wait_for_text(&mut process.master, "Ctrl+S", Duration::from_secs(3)).await?;
    process.master.write_all(b"\t\t\t\t\t\t")?;
    wait_for_text(
        &mut process.master,
        "APK/JADX: true",
        Duration::from_secs(3),
    )
    .await?;
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

#[tokio::test]
async fn ctrl_q_cancels_pending_model_request_and_restores_terminal() -> anyhow::Result<()> {
    pending_model_cancellation(false).await
}

#[tokio::test]
async fn ctrl_c_cancels_pending_model_request_and_allows_another_task() -> anyhow::Result<()> {
    pending_model_cancellation(true).await
}

async fn pending_model_cancellation(cancel_then_retry: bool) -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)))
        .mount(&server)
        .await;
    let directory = tempdir()?;
    let config = directory.path().join("config.toml");
    std::fs::write(&config, format!(
        "api_key=''\nmodel='test'\nendpoint='{}/v1'\napi_type='responses'\nui_language='en'\nshow_buddha_ascii_art=false\nshow_train_ascii_art=false\n",
        server.uri()
    ))?;
    let mut process = spawn_tui(&config)?;
    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;
    process.master.write_all(b"pending request\r")?;
    wait_for_model_requests(&server, 1).await?;
    if cancel_then_retry {
        process.master.write_all(b"\x03")?;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            // Drain redraws so a full PTY buffer cannot block the TUI before cancellation.
            let mut redraw = [0u8; 8192];
            loop {
                match process.master.read(&mut redraw) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                    Err(error) => return Err(error.into()),
                }
            }
            let log =
                std::fs::read_to_string(directory.path().join("nl2sh.log")).unwrap_or_default();
            // The task watch channel and the LLM SIGINT handler can finish first.
            // Both paths must leave the TUI alive and admit the next request.
            if log.contains("task cancelled by user")
                || log.contains("LLM request cancelled by user")
                || log.contains("LLM response cancelled by user")
                || log.contains("LLM retry cancelled by user")
            {
                break;
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "task cancellation was not logged: {log}"
            );
            sleep(Duration::from_millis(20)).await;
        }
        anyhow::ensure!(process.child.try_wait()?.is_none(), "Ctrl+C exited the TUI");
        process.master.write_all(b"second request\r")?;
        wait_for_model_requests(&server, 2).await?;
    }
    process.master.write_all(b"\x11")?;
    let status = timeout(Duration::from_secs(3), process.child.wait()).await??;
    anyhow::ensure!(status.success(), "TUI did not exit cleanly: {status}");
    wait_for_text(&mut process.master, "\x1b[?1049l", Duration::from_secs(3)).await?;
    Ok(())
}

async fn wait_for_model_requests(server: &MockServer, count: usize) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if server.received_requests().await.unwrap_or_default().len() >= count {
            return Ok(());
        }
        anyhow::ensure!(Instant::now() < deadline, "model request was not received");
        sleep(Duration::from_millis(20)).await;
    }
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

#[tokio::test]
async fn tailcat_dialog_opens_without_provider_and_closes_back_to_tui() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        "enable_pty=false\nshow_buddha_ascii_art=false\nshow_train_ascii_art=false\n",
    )?;
    let mut process = spawn_tui(&config)?;
    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;
    process.master.write_all(b"/tailcat\r")?;
    wait_for_text(&mut process.master, "P:", Duration::from_secs(3)).await?;
    process.master.write_all(&[0x1b])?;
    sleep(Duration::from_millis(100)).await;
    assert!(process.child.try_wait()?.is_none());
    process.master.write_all(&[0x11])?;
    assert!(timeout(Duration::from_secs(3), process.child.wait())
        .await??
        .success());
    let log = std::fs::read_to_string(directory.path().join("nl2sh.log"))?;
    assert!(log.contains("local_command"));
    assert!(log.contains("/tailcat"));
    assert!(!log.contains("agent_user_input"));
    assert!(!log.contains("llm_request"));
    Ok(())
}

#[tokio::test]
async fn tailcat_user_selected_ports_share_after_one_enter_without_model_or_safety_prompts(
) -> anyhow::Result<()> {
    use anyhow::Context;
    use base64::Engine;
    use std::os::unix::fs::PermissionsExt;
    let server = MockServer::start().await;
    let directory = tempdir()?;
    let config = directory.path().join("config.toml");
    let binary = directory.path().join("tailcat");
    std::fs::write(&binary, "#!/bin/sh\nif [ \"$1\" = version ]; then printf 'tailcat v0.7.0\\n'; exit 0; fi\nprintf 'tc0123456789abcdef\\n'\nexec sleep 30\n")?;
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))?;
    std::fs::write(&config, format!("api_key=''\nendpoint='{}/v1'\ntailcat_binary_path={:?}\nenable_pty=false\nshow_buddha_ascii_art=false\nshow_train_ascii_art=false\n", server.uri(), binary))?;
    let mut process = spawn_tui(&config)?;
    wait_for_text(&mut process.master, "Ctrl+Q", Duration::from_secs(3)).await?;
    process.master.write_all(b"/tailcat\r")?;
    wait_for_text(&mut process.master, "P:", Duration::from_secs(3)).await?;
    // Deselect ADB and confirm the Web port once. No more input is sent until completion.
    process.master.write_all(b"a\r")?;
    let captured =
        wait_for_text_capture(&mut process.master, "Completed", Duration::from_secs(5)).await?;
    // Copy the result to inspect its exact bytes, independent of ratatui ANSI diffs.
    process.master.write_all(b"C")?;
    let clipboard =
        wait_for_text_capture(&mut process.master, "\x07", Duration::from_secs(3)).await?;
    let encoded = clipboard
        .split("\x1b]52;c;")
        .nth(1)
        .and_then(|s| s.split('\x07').next())
        .context("missing clipboard sequence")?;
    let command = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(encoded)?)?;
    assert!(command.starts_with("tailcat forward tc0123456789abcdef 19999:"));
    // A fragmented arrow escape must not dismiss the completed dialog.
    process.master.write_all(&[0x1b])?;
    sleep(Duration::from_millis(5)).await;
    process.master.write_all(b"[")?;
    sleep(Duration::from_millis(5)).await;
    process.master.write_all(b"A")?;
    process.master.write_all(b"r")?;
    wait_for_text(&mut process.master, "P:", Duration::from_secs(3)).await?;
    process.master.write_all(b"\r")?;
    let retried =
        wait_for_text_capture(&mut process.master, "Completed", Duration::from_secs(5)).await?;
    assert!(!retried.contains("stop it first"));
    // Copy after retry proves the dialog still accepts input after completion.
    sleep(Duration::from_millis(100)).await;
    process.master.write_all(b"C")?;
    let copied_again =
        wait_for_text_capture(&mut process.master, "\x07", Duration::from_secs(3)).await?;
    let encoded_again = copied_again
        .split("\x1b]52;c;")
        .nth(1)
        .and_then(|s| s.split('\x07').next())
        .context("missing retry clipboard sequence")?;
    assert_eq!(
        base64::engine::general_purpose::STANDARD.decode(encoded_again)?,
        command.as_bytes()
    );

    // Failure must leave the wizard open too, ready for another retry.
    std::fs::write(
        &binary,
        "#!/bin/sh\nprintf 'unavailable fixture\\n' >&2\nexit 1\n",
    )?;
    process.master.write_all(b"r")?;
    wait_for_text(&mut process.master, "P:", Duration::from_secs(3)).await?;
    process.master.write_all(b"\r")?;
    wait_for_text(&mut process.master, "Incomplete", Duration::from_secs(5)).await?;
    process.master.write_all(b"r")?;
    wait_for_text(&mut process.master, "P:", Duration::from_secs(3)).await?;
    assert!(!captured.contains("Approve"));
    assert!(!captured.contains("Review"));
    assert!(server
        .received_requests()
        .await
        .is_some_and(|requests| requests.is_empty()));
    assert!(process.child.try_wait()?.is_none());
    process.master.write_all(&[0x1b])?;
    sleep(Duration::from_millis(100)).await;
    process.master.write_all(&[0x11])?;
    assert!(timeout(Duration::from_secs(3), process.child.wait())
        .await??
        .success());
    let log = std::fs::read_to_string(directory.path().join("nl2sh.log"))?;
    assert!(!log.contains("llm_request"));
    Ok(())
}
