use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Mode {
    Agent,
    Command,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum ApiTypeArg {
    Auto,
    ChatCompletions,
    Responses,
}
#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Check for and install the latest compatible GitHub Release.
    Update,
    /// Manage the native background Web service for this configuration.
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
    /// Machine-readable, non-interactive interface for a trusted local bridge.
    Bridge {
        #[command(subcommand)]
        command: BridgeCommand,
    },
}
#[derive(Debug, Clone, Subcommand)]
pub enum ServiceCommand {
    /// Start or return the healthy existing service.
    Start {
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 9999)]
        port: u16,
        #[arg(long)]
        port_strict: bool,
    },
    /// Stop only the service owned by this configuration.
    Stop {
        #[arg(long)]
        json: bool,
    },
    /// Stop and start the service, preserving configuration and sessions.
    Restart {
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 9999)]
        port: u16,
        #[arg(long)]
        port_strict: bool,
    },
    /// Report verified process identity and HTTP readiness.
    Status {
        #[arg(long)]
        json: bool,
    },
    #[command(hide = true)]
    Run {
        #[arg(long)]
        port: u16,
        #[arg(long)]
        port_strict: bool,
    },
}
#[derive(Debug, Clone, Subcommand)]
pub enum BridgeCommand {
    /// Return bounded, read-only Android environment facts.
    Inspect,
    /// Return the configured model-facing tool catalog.
    Tools,
    /// Run the Agent with a bounded base64url JSON request and stored history.
    Ask {
        /// Base64url without padding, containing {session,message} JSON.
        #[arg(long)]
        payload_base64: String,
    },
    /// Invoke one registered tool directly with base64url {tool,arguments} JSON.
    Invoke {
        /// Base64url without padding, containing {tool,arguments} JSON.
        #[arg(long)]
        payload_base64: String,
    },
    /// List pending direct-tool approvals on this device.
    Approvals,
    /// Approve or reject one pending direct-tool request from an interactive terminal.
    Approve {
        /// Request identifier printed by `bridge approvals`.
        id: String,
    },
}
impl From<ApiTypeArg> for nl2sh::config::ApiType {
    fn from(value: ApiTypeArg) -> Self {
        match value {
            ApiTypeArg::Auto => Self::Auto,
            ApiTypeArg::ChatCompletions => Self::ChatCompletions,
            ApiTypeArg::Responses => Self::Responses,
        }
    }
}
#[derive(Debug, Parser)]
#[command(version, about = "Natural Language to Shell for Android")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    pub instruction: Option<String>,
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "agent")]
    pub mode: Mode,
    #[arg(long)]
    pub endpoint: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long, value_enum)]
    pub api_type: Option<ApiTypeArg>,
    #[arg(long)]
    pub no_pty: bool,
    #[arg(long)]
    pub ascii: bool,
    #[arg(long)]
    pub dry_run: bool,
    /// Run only the browser UI without initializing a terminal interface.
    #[arg(long)]
    pub web_only: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_provider_overrides() {
        let cli = Cli::try_parse_from([
            "nl2sh",
            "--endpoint",
            "http://localhost:11434/v1",
            "--model",
            "local",
            "--api-type",
            "chat_completions",
            "task",
        ])
        .expect("valid CLI should parse");
        assert_eq!(cli.endpoint.as_deref(), Some("http://localhost:11434/v1"));
        assert_eq!(cli.model.as_deref(), Some("local"));
        assert!(matches!(cli.api_type, Some(ApiTypeArg::ChatCompletions)));
    }

    #[test]
    fn parses_update_command() {
        let cli = Cli::try_parse_from(["nl2sh", "update"]).expect("update command should parse");
        assert!(matches!(cli.command, Some(Command::Update)));
    }

    #[test]
    fn parses_web_only_mode() {
        let cli = Cli::try_parse_from(["nl2sh", "--web-only"]).expect("valid CLI should parse");
        assert!(cli.web_only);
        assert!(cli.instruction.is_none());
        assert!(cli.command.is_none());
    }
}
