//! Export public defaults, CLI help, and the complete tool schema for documentation.
#[path = "../src/cli.rs"]
#[allow(dead_code)]
mod cli;

use anyhow::Result;
use clap::CommandFactory;
use nl2sh::{config::Config, tools::configured_tools};

fn main() -> Result<()> {
    let defaults = Config::default();
    let mut catalog_config = defaults.clone();
    catalog_config.ima_enabled = true;
    catalog_config.tool_groups.insert("jadx".into(), true);
    catalog_config.tool_groups.insert("tailcat".into(), true);
    let mut command = cli::Cli::command();
    let mut help = vec![command.render_long_help().to_string()];
    for child in command.get_subcommands_mut() {
        help.push(child.render_long_help().to_string());
        for nested in child.get_subcommands_mut() {
            help.push(nested.render_long_help().to_string());
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "defaults": defaults,
            "cli": help,
            "tools": configured_tools(&catalog_config),
        }))?
    );
    Ok(())
}
