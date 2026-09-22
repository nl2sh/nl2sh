mod cli;
use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Command, Mode};
use nl2sh::{
    agent::{
        android_shell_constraints, AgentRunner, ConfirmationDecision, Confirmer, StdioConfirmer,
    },
    config::{self},
    history::HistoryLog,
    llm::{build_client, ConversationMessage, LlmClient, LlmRequest, Role},
    security::assess,
    shell::{ConsoleOutput, OutputSink, ShellExecutor, SystemRootProbe},
    tui,
};
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let path = match &cli.config {
        Some(p) => p.clone(),
        None => config::default_config_path()?,
    };
    let mut cfg = load_runtime_config(&path, &cli)?;
    if matches!(cli.command, Some(Command::Update)) {
        return run_update(&cfg).await;
    }
    let mut provider_configured = cfg.provider_is_configured();
    if cli.instruction.is_some() && !provider_configured {
        anyhow::bail!(
            "model provider is not configured; start nl2sh without an instruction and use /config or /setting"
        )
    }
    let history_log = HistoryLog::open_with_limits(
        &path,
        &cfg.history_log_file,
        cfg.history_log_event_max_bytes,
        cfg.history_log_max_bytes,
    )?;
    let mut llm = build_client(&cfg)?;
    let probe = SystemRootProbe;
    let root = format!("{:?}", probe.status());
    if let Some(instruction) = cli.instruction.as_deref() {
        run_once(
            &cfg,
            &llm,
            cli.mode,
            instruction,
            cli.dry_run,
            &[],
            std::sync::Arc::new(ConsoleOutput),
        )
        .await?;
        return Ok(());
    }
    let web = nl2sh::web_ui::start(path.clone()).await?;
    nl2sh::web_ui::set_welcome_url(web.url().to_owned());
    if matches!(cli.mode, Mode::Agent) {
        loop {
            match tui::run_agent_session(
                &cfg,
                &llm,
                root.clone(),
                history_log.clone(),
                provider_configured,
            )
            .await?
            {
                tui::SessionExit::Quit => return Ok(()),
                tui::SessionExit::Update(release) => {
                    nl2sh::update::install(&cfg, &release).await?;
                    println!("已更新到 v{}，请重新启动 nl2sh。", release.version);
                    return Ok(());
                }
                tui::SessionExit::Configure(target) => {
                    run_configure_target(&path, target).await?;
                    cfg = load_runtime_config(&path, &cli)?;
                    provider_configured = cfg.provider_is_configured();
                    llm = build_client(&cfg)?;
                }
            }
        }
    }
    let mut model_history = Vec::new();
    let ui_history = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    loop {
        let snapshot = ui_history
            .lock()
            .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
            .clone();
        let instruction = match tui::run(
            tui::TuiOptions {
                model: cfg.model.clone(),
                root: root.clone(),
                ascii: cfg.ascii_symbols,
                show_buddha_ascii_art: cfg.show_buddha_ascii_art,
                show_train_ascii_art: cfg.show_train_ascii_art,
                language: cfg.ui_language,
                api_type: format!("{:?}", cfg.api_type),
                mode: match (cfg.ui_language, cli.mode) {
                    (config::UiLanguage::ZhCn, Mode::Agent) => "智能体".into(),
                    (config::UiLanguage::ZhCn, Mode::Command) => "命令".into(),
                    (config::UiLanguage::En, mode) => format!("{mode:?}"),
                },
                turn: model_history.len(),
                max_context: cfg.max_context_turns,
            },
            snapshot,
        )? {
            Some(value) => value,
            None => return Ok(()),
        };
        if let Some(target) = config_target(instruction.trim()) {
            run_configure_target(&path, target).await?;
            cfg = load_runtime_config(&path, &cli)?;
            provider_configured = cfg.provider_is_configured();
            llm = build_client(&cfg)?;
            ui_history
                .lock()
                .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
                .push(match cfg.ui_language {
                    config::UiLanguage::ZhCn => "[CONFIG] 模型服务配置已重新加载。".into(),
                    config::UiLanguage::En => "[CONFIG] Provider configuration reloaded.".into(),
                });
            continue;
        }
        if instruction.trim() == "/help" {
            ui_history
                .lock()
                .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
                .extend(tui::help_history(
                    cfg.ui_language,
                    cfg.ascii_symbols,
                    cfg.show_buddha_ascii_art,
                ));
            continue;
        }
        if instruction.trim() == "/clear" {
            ui_history
                .lock()
                .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
                .clear();
            model_history.clear();
            continue;
        }
        if instruction.trim_start().starts_with('/') {
            ui_history
                .lock()
                .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
                .push(match cfg.ui_language {
                    config::UiLanguage::ZhCn => format!(
                        "⚠️ 未知本地命令：{}。输入 /help 查看可用命令。",
                        instruction.trim()
                    ),
                    config::UiLanguage::En => format!(
                        "[WARN] Unknown local command: {}. Use /help to list commands.",
                        instruction.trim()
                    ),
                });
            continue;
        }
        if !provider_configured {
            ui_history
                .lock()
                .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
                .push(match cfg.ui_language {
                    config::UiLanguage::ZhCn => "⚠️ 尚未配置模型服务，请使用 /config 或 /setting 打开设置面板。".into(),
                    config::UiLanguage::En => "[WARN] Provider is not configured. Use /config or /setting to open Settings.".into(),
                });
            continue;
        }
        ui_history
            .lock()
            .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
            .push(format!("> {instruction}"));
        let output = std::sync::Arc::new(UiOutput {
            history: ui_history.clone(),
            ascii: cfg.ascii_symbols,
            max_bytes: cfg.ui_live_output_max_bytes,
            bytes: std::sync::Mutex::new(0),
            truncated: std::sync::atomic::AtomicBool::new(false),
        });
        match run_once(
            &cfg,
            &llm,
            cli.mode,
            &instruction,
            cli.dry_run,
            &model_history,
            output,
        )
        .await
        {
            Ok(Some(outcome)) => {
                let mut visible = ui_history
                    .lock()
                    .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?;
                append_tool_history(&mut visible, &outcome.transcript, cfg.ascii_symbols);
                visible.push(format!(
                    "{} {}",
                    if cfg.ascii_symbols { "[AGENT]" } else { "🤖" },
                    outcome.final_text
                ));
                model_history.push(outcome.transcript);
            }
            Ok(None) => ui_history
                .lock()
                .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
                .push(match (cfg.ui_language, cfg.ascii_symbols) {
                    (config::UiLanguage::ZhCn, true) => "[OK] 命令执行完成。".into(),
                    (config::UiLanguage::ZhCn, false) => "✅ 命令执行完成。".into(),
                    (config::UiLanguage::En, true) => "[OK] Command completed.".into(),
                    (config::UiLanguage::En, false) => "✅ Command completed.".into(),
                }),
            Err(error) => ui_history
                .lock()
                .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
                .push(format!(
                    "{} {error:#}",
                    if cfg.ascii_symbols { "[ERROR]" } else { "❌" }
                )),
        }
    }
}

async fn run_update(cfg: &config::Config) -> Result<()> {
    if !nl2sh::update::self_update_enabled() {
        println!("{}", nl2sh::update::package_update_message());
        return Ok(());
    }
    println!("正在检查更新 / Checking for updates…");
    let Some(release) = nl2sh::update::check(cfg).await? else {
        println!(
            "已是最新版本 / Already up to date: {}",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    };
    nl2sh::update::install(cfg, &release).await?;
    println!("已更新到 v{}，请重新启动 nl2sh。", release.version);
    Ok(())
}

fn load_runtime_config(path: &std::path::Path, cli: &Cli) -> Result<config::Config> {
    let mut cfg = config::load_or_default_unvalidated(path)?;
    if let Some(endpoint) = cli.endpoint.clone() {
        cfg.endpoint = endpoint;
    }
    if let Some(model) = cli.model.clone() {
        cfg.model = model;
    }
    if let Some(api_type) = cli.api_type {
        cfg.api_type = api_type.into();
    }
    if cli.no_pty {
        cfg.enable_pty = false
    }
    if cli.ascii {
        cfg.ascii_symbols = true
    }
    cfg.validate_runtime()?;
    Ok(cfg)
}

fn config_target(input: &str) -> Option<tui::ConfigTarget> {
    match input {
        "/config" | "/setting" => Some(tui::ConfigTarget::All),
        "/balance" => Some(tui::ConfigTarget::Balance),
        _ => None,
    }
}

async fn run_configure_target(path: &std::path::Path, target: tui::ConfigTarget) -> Result<()> {
    match target {
        tui::ConfigTarget::All => config::run_configure(path),
        tui::ConfigTarget::Provider => config::run_provider_configure(path),
        tui::ConfigTarget::Model => config::run_model_configure(path),
        tui::ConfigTarget::Models => config::run_models_configure(path).await,
        tui::ConfigTarget::Balance => config::run_balance_query(path).await,
        tui::ConfigTarget::Proxy => Ok(()),
    }
}

async fn run_once(
    cfg: &config::Config,
    llm: &dyn LlmClient,
    mode: Mode,
    instruction: &str,
    dry_run: bool,
    history: &[Vec<nl2sh::llm::ConversationItem>],
    output: std::sync::Arc<dyn OutputSink>,
) -> Result<Option<nl2sh::agent::AgentOutcome>> {
    let instruction = nl2sh::file_references::augment_file_references(instruction);
    match mode {
        Mode::Agent => {
            let executor = ShellExecutor::new(cfg.clone()).with_output(output);
            let outcome = AgentRunner {
                config: cfg,
                llm,
                executor: &executor,
                confirmer: &StdioConfirmer,
            }
            .run_with_history(&instruction, history)
            .await?;
            println!("{}", outcome.final_text);
            Ok(Some(outcome))
        }
        Mode::Command => {
            run_command(cfg, llm, &instruction, dry_run, output).await?;
            Ok(None)
        }
    }
}

struct UiOutput {
    history: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ascii: bool,
    max_bytes: usize,
    bytes: std::sync::Mutex<usize>,
    truncated: std::sync::atomic::AtomicBool,
}

impl OutputSink for UiOutput {
    fn stdout(&self, text: &str) {
        ConsoleOutput.stdout(text);
        self.push(if self.ascii { "[OUT]" } else { "✅" }, text);
    }
    fn stderr(&self, text: &str) {
        ConsoleOutput.stderr(text);
        self.push(if self.ascii { "[ERR]" } else { "❌" }, text);
    }
}

impl UiOutput {
    fn push(&self, prefix: &str, text: &str) {
        let Ok(mut used) = self.bytes.lock() else {
            return;
        };
        if *used >= self.max_bytes {
            if !self
                .truncated
                .swap(true, std::sync::atomic::Ordering::AcqRel)
            {
                if let Ok(mut history) = self.history.lock() {
                    history.push(format!(
                        "{prefix} [... NL2SH OUTPUT TRUNCATED: live UI limit {} bytes reached; later live output omitted ...]",
                        self.max_bytes
                    ));
                }
            }
            return;
        }
        let text = nl2sh::limits::truncate_text(text, self.max_bytes - *used);
        *used = used.saturating_add(text.len());
        if let Ok(mut history) = self.history.lock() {
            for line in text.lines() {
                history.push(format!("{prefix} {line}"));
            }
        }
    }
}

fn append_tool_history(
    history: &mut Vec<String>,
    transcript: &[nl2sh::llm::ConversationItem],
    ascii: bool,
) {
    for item in transcript {
        if let nl2sh::llm::ConversationItem::Tools(round) = item {
            for call in &round.calls {
                history.push(format!(
                    "{} {}",
                    if ascii { "[TOOL]" } else { "🔧" },
                    call.name
                ));
                if let Some(command) = call
                    .arguments
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                {
                    history.push(format!("{} {command}", if ascii { "[CMD]" } else { "💻" }));
                }
            }
            for result in &round.results {
                let prefix = if result.success {
                    if ascii {
                        "[OK]"
                    } else {
                        "✅"
                    }
                } else if ascii {
                    "[ERROR]"
                } else {
                    "❌"
                };
                history.push(format!("{prefix} {}", result.output));
            }
        }
    }
}

async fn run_command(
    cfg: &config::Config,
    llm: &dyn LlmClient,
    input: &str,
    dry: bool,
    output: std::sync::Arc<dyn OutputSink>,
) -> Result<()> {
    let system = format!(
        "You are an Android device shell command generator. Generate one executable command for the detected Android shell runtime. Output only the command. No Markdown, explanation, prefix, or alternatives. {} If a non-baseline runtime would be required and cannot be safely probed with an appropriate runtime fallback in the same command, return NL2SH_UNABLE_TO_GENERATE. If unsafe or unreliable return NL2SH_UNABLE_TO_GENERATE.",
        android_shell_constraints()
    );
    let r = llm
        .complete(LlmRequest {
            model: cfg.model.clone(),
            items: vec![
                nl2sh::llm::ConversationItem::Message(ConversationMessage::new(
                    Role::System,
                    system,
                )),
                nl2sh::llm::ConversationItem::Message(ConversationMessage::new(Role::User, input)),
            ],
            tools: vec![],
        })
        .await?;
    let raw = r.text.context("model returned no command")?;
    let mut command = clean(&raw);
    if command == "NL2SH_UNABLE_TO_GENERATE" {
        anyhow::bail!("model could not safely generate a command")
    }
    let mut a = assess(&command, cfg);
    println!("Command: {command}\nRisk: {:?}", a.risk_level);
    if dry {
        return Ok(());
    }
    let confirmer = StdioConfirmer;
    let mut interactive_override = None;
    while a.requires_confirmation {
        match confirmer.confirm(&command, &a).await? {
            ConfirmationDecision::Approve => break,
            ConfirmationDecision::ApproveForTask | ConfirmationDecision::ApproveForRun => break,
            ConfirmationDecision::ApproveInteractive => {
                interactive_override = Some(true);
                break;
            }
            ConfirmationDecision::ApproveCaptured => {
                interactive_override = Some(false);
                break;
            }
            ConfirmationDecision::Reject => {
                anyhow::bail!("command was not executed: confirmation refused or unavailable")
            }
            ConfirmationDecision::Edit(edited) => {
                command = edited;
                a = assess(&command, cfg);
                println!("Edited command: {command}\nRisk: {:?}", a.risk_level);
            }
        }
    }
    let executor = ShellExecutor::new(cfg.clone()).with_output(output);
    let result = nl2sh::shell::CommandExecutor::execute(
        &executor,
        &command,
        a.requires_root,
        interactive_override.unwrap_or_else(|| nl2sh::shell::is_interactive(&command, false)),
    )
    .await?;
    if result.timed_out {
        anyhow::bail!("command timed out")
    }
    if result.interrupted {
        anyhow::bail!("command interrupted")
    }
    if result.exit_code != Some(0) {
        anyhow::bail!("command exited with status {:?}", result.exit_code)
    }
    Ok(())
}
fn clean(s: &str) -> String {
    let trimmed = s.trim();
    let mut x = if trimmed.starts_with("```") {
        let mut lines = trimmed.lines();
        let _language = lines.next();
        lines
            .take_while(|line| line.trim() != "```")
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_owned()
    } else {
        trimmed.to_owned()
    };
    for p in ["Command:", "command:"] {
        if let Some(v) = x.strip_prefix(p) {
            x = v.trim().into()
        }
    }
    x.lines().next().unwrap_or("").trim().into()
}

#[cfg(test)]
mod tests {
    use super::clean;
    #[test]
    fn cleans_only_known_command_wrappers() {
        assert_eq!(clean("```sh\nid\n```"), "id");
        assert_eq!(clean("Command: pwd"), "pwd");
        assert_eq!(clean("id\npwd"), "id");
    }
}
