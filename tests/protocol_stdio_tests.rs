//! Exercise the shipped CLI process: MCP stdout isolation, EOF and removed interfaces.
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn native_stdio_process_handles_shell_output_and_eof() -> Result<()> {
    let root = tempfile::tempdir()?;
    let config = root.path().join("config.toml");
    std::fs::write(&config, "")?;
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_nl2sh"))
        .arg("--config")
        .arg(&config)
        .args(["protocol", "stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut input = child.stdin.take().context("stdin missing")?;
    let mut output = BufReader::new(child.stdout.take().context("stdout missing")?).lines();
    input.write_all(format!("{}\n",json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"process-test","version":"1"}}})).as_bytes()).await?;
    let line = tokio::time::timeout(std::time::Duration::from_secs(15), output.next_line())
        .await??
        .context("missing initialize")?;
    assert_eq!(
        serde_json::from_str::<Value>(&line)?["result"]["protocolVersion"],
        "2025-11-25"
    );
    input.write_all(format!("{}\n{}\n",json!({"jsonrpc":"2.0","method":"notifications/initialized"}),json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"nl2sh_invoke","arguments":{"tool":"execute_shell_command","arguments":{"command":"printf protocol-output","reason":"verify captured shell output"}}}})).as_bytes()).await?;
    let line = tokio::time::timeout(std::time::Duration::from_secs(15), output.next_line())
        .await??
        .context("missing tools/call")?;
    let response: Value = serde_json::from_str(&line)?;
    assert_eq!(
        response["result"]["structuredContent"]["success"], true,
        "{response}"
    );
    assert!(response["result"]["structuredContent"]["output"]
        .as_str()
        .is_some_and(|text| text.contains("protocol-output")));
    drop(input);
    let status = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait()).await??;
    assert!(status.success());
    assert!(
        output.next_line().await?.is_none(),
        "stdout contains non-protocol output"
    );
    Ok(())
}
#[test]
fn legacy_gateway_cli_and_config_are_rejected() -> Result<()> {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_nl2sh"))
        .args(["bridge", "tools"])
        .output()?;
    assert!(!output.status.success());
    assert!(toml::from_str::<nl2sh::config::Config>("bridge_auto_approve=true").is_err());
    Ok(())
}
