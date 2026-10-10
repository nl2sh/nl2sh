//! Exercise the shipped CLI process: MCP stdout isolation, EOF and removed interfaces.
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn native_stdio_process_handles_shell_output_and_eof() -> Result<()> {
    let root = tempfile::tempdir()?;
    let config = root.path().join("config.toml");
    std::fs::write(&config, "bridge_auto_approve = true\n")?;
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

async fn generated_server(
    config: &std::path::Path,
) -> Result<(tokio::process::Child, String, String)> {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_nl2sh"))
        // Exercise the default configuration path override rather than --config.
        .env("NL2SH_CONFIG", config)
        .env_remove("NL2SH_PROTOCOL_TOKEN")
        .args(["protocol", "serve", "--port", "0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut lines = BufReader::new(child.stdout.take().context("stdout missing")?).lines();
    let mut url = None;
    let mut token = None;
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        while let Some(line) = lines.next_line().await? {
            if let Some(value) = line.strip_prefix("MCP (Streamable HTTP): ") {
                url = Some(value.to_owned());
            }
            if let Some(value) =
                line.strip_prefix("Token (generated for this run; changes after restart): ")
            {
                token = Some(value.to_owned());
            }
            if line.starts_with("HTTP transmits the token") {
                return anyhow::Ok(());
            }
        }
        anyhow::bail!("startup output incomplete")
    })
    .await??;
    Ok((
        child,
        url.context("missing MCP URL")?,
        token.context("missing generated token")?,
    ))
}

#[tokio::test]
async fn single_command_generates_usable_token_redacts_tasks_and_rotates_on_restart() -> Result<()>
{
    let root = tempfile::tempdir()?;
    let config = root.path().join("config.toml");
    std::fs::write(&config, "")?;
    let (mut child, url, token) = generated_server(&config).await?;
    assert_eq!(token.len(), 64);
    assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(!url.contains("0.0.0.0"));
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"single-command-test","version":"1"}}});
    let request = |token: &str, body: Value| {
        client
            .post(&url)
            .bearer_auth(token)
            .header("accept", "application/json, text/event-stream")
            .json(&body)
    };
    assert_eq!(
        request("wrong", init.clone()).send().await?.status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    let response: Value = request(&token, init)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
    let response: Value = request(&token, json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"nl2sh_invoke","arguments":{"tool":"execute_shell_command","arguments":{"command":format!("printf {token}"),"reason":"verify generated credential redaction"}}}})).send().await?.error_for_status()?.json().await?;
    assert_eq!(response["result"]["structuredContent"]["success"], true);
    assert!(!response.to_string().contains(&token));
    assert!(response.to_string().contains("[REDACTED]"));
    let connections = nl2sh::protocol::connection_info(&config).await;
    assert_eq!(connections.mcp_url.as_deref(), Some(url.as_str()));
    assert!(!serde_json::to_string(&connections)?.contains(&token));
    assert!(
        std::fs::read_to_string(root.path().join("protocol/connection.json"))?.contains(&token)
    );
    let card: Value = client
        .get(connections.agent_card_url.context("missing card URL")?)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(!card.to_string().contains(&token));
    child.kill().await?;
    let (mut restarted, new_url, new_token) = generated_server(&config).await?;
    assert_ne!(token, new_token);
    let response = client
        .post(&new_url)
        .bearer_auth(&token)
        .header("accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc":"2.0","id":3,"method":"tools/list","params":{}}))
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    let response = client
        .post(&new_url)
        .bearer_auth(&new_token)
        .header("accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc":"2.0","id":4,"method":"tools/list","params":{}}))
        .send()
        .await?;
    assert!(response.status().is_success());
    restarted.kill().await?;
    Ok(())
}

#[tokio::test]
async fn configured_fixed_token_authenticates_after_restart_and_environment_overrides() -> Result<()>
{
    let root = tempfile::tempdir()?;
    let config = root.path().join("config.toml");
    let fixed = "config-fixed-token-01234567890123456789";
    let override_token = "environment-token-01234567890123456789";
    std::fs::write(&config, format!("protocol_token = '{fixed}'\n"))?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    for override_env in [false, false, true] {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_nl2sh"));
        command
            .arg("--config")
            .arg(&config)
            .args(["protocol", "serve", "--host", "127.0.0.1", "--port", "0"])
            .env_remove("NL2SH_PROTOCOL_TOKEN")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if override_env {
            command.env("NL2SH_PROTOCOL_TOKEN", override_token);
        }
        let mut child = command.spawn()?;
        let mut lines = BufReader::new(child.stdout.take().context("stdout missing")?).lines();
        let mut url = None;
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            while let Some(line) = lines.next_line().await? {
                assert!(!line.contains(fixed));
                assert!(!line.contains(override_token));
                if let Some(value) = line.strip_prefix("MCP (Streamable HTTP): ") {
                    url = Some(value.to_owned());
                }
                if line.starts_with("HTTP transmits the token") {
                    return anyhow::Ok(());
                }
            }
            anyhow::bail!("startup output incomplete")
        })
        .await??;
        let url = url.context("missing MCP URL")?;
        let selected = if override_env { override_token } else { fixed };
        let body = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixed-token-test","version":"1"}}});
        let request = |token: &str| {
            client
                .post(&url)
                .bearer_auth(token)
                .header("accept", "application/json, text/event-stream")
                .json(&body)
        };
        request(selected).send().await?.error_for_status()?;
        assert_eq!(
            request(if override_env { fixed } else { "wrong-token" })
                .send()
                .await?
                .status(),
            reqwest::StatusCode::UNAUTHORIZED
        );
        let response: Value = client.post(&url).bearer_auth(selected)
            .header("accept", "application/json, text/event-stream")
            .json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"nl2sh_invoke","arguments":{"tool":"execute_shell_command","arguments":{"command":format!("printf {selected}"),"reason":"verify credential redaction"}}}}))
            .send().await?.error_for_status()?.json().await?;
        assert_eq!(response["result"]["structuredContent"]["success"], true);
        assert!(!response.to_string().contains(fixed));
        assert!(!response.to_string().contains(selected));
        assert!(response.to_string().contains("[REDACTED]"));
        let connections = nl2sh::protocol::connection_details(&config).await;
        assert_eq!(connections.token.as_deref(), Some(selected));
        child.kill().await?;
    }
    Ok(())
}
