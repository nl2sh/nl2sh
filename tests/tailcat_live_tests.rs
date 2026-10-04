//! Opt-in end-to-end test using an installed Tailcat binary and real transport.

use anyhow::{Context, Result};
use async_trait::async_trait;
use nl2sh::{
    agent::{ConfirmationDecision, Confirmer},
    config::Config,
    security::SecurityAssessment,
    shell::ShellExecutor,
    tools::runtime::invoke,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Approve;
#[async_trait]
impl Confirmer for Approve {
    async fn confirm(&self, _: &str, _: &SecurityAssessment) -> Result<ConfirmationDecision> {
        Ok(ConfirmationDecision::Approve)
    }
}

#[tokio::test]
#[ignore = "set NL2SH_TAILCAT_TEST_BINARY to an installed Tailcat executable"]
async fn raw_file_transfer_through_tailcat_tools() -> Result<()> {
    let binary = std::env::var("NL2SH_TAILCAT_TEST_BINARY")
        .context("NL2SH_TAILCAT_TEST_BINARY is required")?;
    let dir = tempfile::tempdir()?;
    let source = dir.path().join("source.bin");
    let target = dir.path().join("received.bin");
    std::fs::write(&source, b"nl2sh tailcat live transfer\n")?;
    let mut config = Config {
        tailcat_binary_path: binary.into(),
        ..Config::default()
    };
    config.tool_groups.insert("tailcat".into(), true);
    let executor = ShellExecutor::new(config.clone());
    let started = invoke(
        &config,
        &executor,
        &Approve,
        "tailcat_receive_stream",
        json!({"path":target}),
    )
    .await?;
    assert!(started.success);
    let address = started
        .output
        .lines()
        .find_map(|line| line.strip_prefix("address="))
        .context("listener address missing")?;
    let sent = invoke(
        &config,
        &executor,
        &Approve,
        "tailcat_send_file",
        json!({"path":source,"address":address,"mode":"stream"}),
    )
    .await;
    if sent.is_err() {
        let _ = invoke(&config, &executor, &Approve, "tailcat_stop", json!({})).await;
    }
    assert!(sent?.success);
    let mut raw_ok = false;
    for _ in 0..40 {
        if std::fs::read(&target)? == std::fs::read(&source)? {
            raw_ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    if !raw_ok {
        anyhow::bail!("Tailcat received bytes did not match source")
    }
    let inbox = dir.path().join("inbox");
    std::fs::create_dir(&inbox)?;
    let started = invoke(
        &config,
        &executor,
        &Approve,
        "tailcat_receive",
        json!({"directory":inbox}),
    )
    .await?;
    let address = started
        .output
        .lines()
        .find_map(|line| line.strip_prefix("address="))
        .context("file drop box address missing")?;
    let sent = invoke(
        &config,
        &executor,
        &Approve,
        "tailcat_send_file",
        json!({"path":source,"address":address,"mode":"copy"}),
    )
    .await;
    let stopped = invoke(&config, &executor, &Approve, "tailcat_stop", json!({})).await?;
    assert!(stopped.success);
    assert!(sent?.success);
    let received = std::fs::read_dir(&inbox)?
        .filter_map(|item| item.ok())
        .filter_map(|item| std::fs::read(item.path()).ok())
        .collect::<Vec<_>>();
    assert_eq!(received, vec![std::fs::read(source)?]);
    share_existing_service(&config, &executor).await
}

#[tokio::test]
#[ignore = "set NL2SH_TAILCAT_TEST_BINARY to an installed Tailcat executable"]
async fn occupied_service_port_is_forwarded_without_rebinding() -> Result<()> {
    let binary = std::env::var("NL2SH_TAILCAT_TEST_BINARY")
        .context("NL2SH_TAILCAT_TEST_BINARY is required")?;
    let mut config = Config {
        tailcat_binary_path: binary.into(),
        ..Config::default()
    };
    config.tool_groups.insert("tailcat".into(), true);
    let executor = ShellExecutor::new(config.clone());
    share_existing_service(&config, &executor).await
}

async fn share_existing_service(config: &Config, executor: &ShellExecutor) -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut request = [0_u8; 128];
        let _ = socket.read(&mut request).await?;
        socket
            .write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 4\r\n\r\npass")
            .await?;
        Ok::<(), anyhow::Error>(())
    });
    let started = invoke(
        config,
        executor,
        &Approve,
        "tailcat_serve",
        json!({"port":port}),
    )
    .await?;
    let address = started
        .output
        .lines()
        .find_map(|line| line.strip_prefix("address="))
        .context("port-sharing address missing")?;
    let mut client = tokio::process::Command::new(&config.tailcat_binary_path)
        .arg(address)
        .arg(port.to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    if let Some(mut input) = client.stdin.take() {
        input.write_all(b"GET / HTTP/1.0\r\n\r\n").await?;
    }
    let reply = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        client.wait_with_output(),
    )
    .await??;
    assert!(
        reply.status.success(),
        "{}",
        String::from_utf8_lossy(&reply.stderr)
    );
    assert!(String::from_utf8_lossy(&reply.stdout).contains("\r\n\r\npass"));
    server.await??;
    let stopped = invoke(config, executor, &Approve, "tailcat_stop", json!({})).await?;
    assert!(stopped.success);
    Ok(())
}
