//! Interactive, model-free device regression driver for managed background shells.
//! Feed one {"tool": ..., "arguments": ...} object per line in a real terminal.
//! The production StdioConfirmer remains authoritative; EOF cleans all owned children.
use anyhow::{bail, Context, Result};
use nl2sh::{
    agent::StdioConfirmer,
    config::{Config, ExecuteUserMode},
    shell::{shutdown_background, ShellExecutor},
    tools::runtime::invoke,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Call {
    tool: String,
    arguments: Value,
}
#[tokio::main]
async fn main() -> Result<()> {
    let result = run().await;
    result.and(shutdown_background().await)
}
async fn run() -> Result<()> {
    let config = Config {
        execute_user_mode: ExecuteUserMode::Normal,
        ..Config::default()
    };
    let executor = ShellExecutor::new(config.clone());
    loop {
        println!("BACKGROUND_CHECK_READY");
        let line = tokio::task::spawn_blocking(|| -> std::io::Result<String> {
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            Ok(line)
        })
        .await
        .context("input worker failed")??;
        if line.trim().is_empty() || line.trim() == "exit" {
            return Ok(());
        }
        let call: Call = serde_json::from_str(&line).context("invalid regression call JSON")?;
        if !matches!(
            call.tool.as_str(),
            "execute_shell_command" | "read_output" | "kill"
        ) {
            bail!("only managed shell regression tools are supported")
        }
        let result = invoke(
            &config,
            &executor,
            &StdioConfirmer,
            &call.tool,
            call.arguments,
        )
        .await;
        match result {
            Ok(result) => println!(
                "BACKGROUND_CHECK_RESULT {}",
                serde_json::to_string(&result)?
            ),
            Err(error) => println!(
                "BACKGROUND_CHECK_RESULT {}",
                serde_json::json!({"success":false,"error":format!("{error:#}")})
            ),
        }
    }
}
