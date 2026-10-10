//! Interactive device verifier for the managed wireless ADB tool, without a model.
//! Build for Android and run in an interactive shell. Approvals remain explicit.
use anyhow::{Context, Result};
use async_trait::async_trait;
use nl2sh::{
    agent::{ConfirmationDecision, Confirmer},
    config::Config,
    security::SecurityAssessment,
    shell::ShellExecutor,
    tools::runtime::invoke,
};
use serde_json::{json, Value};

async fn input() -> Result<String> {
    tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        Ok::<_, std::io::Error>(line.trim().to_owned())
    })
    .await
    .context("input worker failed")?
    .map_err(Into::into)
}
struct LocalApproval;
#[async_trait]
impl Confirmer for LocalApproval {
    async fn confirm(
        &self,
        preview: &nl2sh::agent::ConfirmationRequest<'_>,
        assessment: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        let preview = preview.preview;
        eprintln!(
            "{preview}\nRisk: {:?}. Type CONFIRM to approve:",
            assessment.risk_level
        );
        Ok(if input().await? == "CONFIRM" {
            ConfirmationDecision::Approve
        } else {
            ConfirmationDecision::Reject
        })
    }
}
#[tokio::main]
async fn main() -> Result<()> {
    let mut config = Config::default();
    config.tool_groups.insert("tailcat".into(), true);
    let executor = ShellExecutor::new(config.clone());
    eprintln!("Enter tool JSON (setup/share), or stop. This process owns its listener. Share results contain the pairing code.");
    loop {
        let line = input().await?;
        if line.is_empty() || line == "stop" {
            break;
        }
        let args: Value = serde_json::from_str(&line).context("invalid tool JSON")?;
        match invoke(&config, &executor, &LocalApproval, "tailcat_adb_pair", args).await {
            Ok(result) => println!("{}", serde_json::to_string(&result)?),
            Err(error) => println!("{}", json!({"success":false,"error":error.to_string()})),
        }
    }
    let result = invoke(
        &config,
        &executor,
        &LocalApproval,
        "tailcat_stop",
        json!({}),
    )
    .await?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
