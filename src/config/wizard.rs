use super::{ApiType, Config, ConfirmPolicy, ExecuteUserMode, UiLanguage};
use crate::provider_account::build_account_client;
use crate::provider_metadata::{build_metadata_client, known_context_window, ModelMetadata};
use anyhow::{bail, Context, Result};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
};

const PROVIDERS: &[ProviderPreset] = &[
    ProviderPreset::new("OpenAI", "https://api.openai.com/v1"),
    ProviderPreset::new("DeepSeek", "https://api.deepseek.com"),
    ProviderPreset::new("Moonshot / Kimi", "https://api.moonshot.cn/v1"),
    ProviderPreset::new("SiliconFlow", "https://api.siliconflow.cn/v1"),
    ProviderPreset::new("Ollama (local)", "http://127.0.0.1:11434/v1"),
    ProviderPreset::custom("Custom", "自定义"),
];

struct ProviderPreset {
    name_en: &'static str,
    name_zh_cn: &'static str,
    endpoint: Option<&'static str>,
}

impl ProviderPreset {
    const fn new(name: &'static str, endpoint: &'static str) -> Self {
        Self {
            name_en: name,
            name_zh_cn: name,
            endpoint: Some(endpoint),
        }
    }

    const fn custom(name_en: &'static str, name_zh_cn: &'static str) -> Self {
        Self {
            name_en,
            name_zh_cn,
            endpoint: None,
        }
    }

    fn name(&self, language: UiLanguage) -> &'static str {
        match language {
            UiLanguage::ZhCn => self.name_zh_cn,
            UiLanguage::En => self.name_en,
        }
    }
}

struct RawModeGuard;

impl RawModeGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode().context("failed to enable raw mode for provider selection")?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

/// Interactively creates a new configuration without overwriting an existing file.
pub fn run_wizard(path: &Path) -> Result<()> {
    if path.exists() {
        bail!(
            "configuration already exists at {}; refusing to overwrite",
            path.display()
        )
    }
    println!("正在创建配置 / Creating configuration: {}", path.display());
    let language = prompt("界面语言 / UI language (zh_cn/en)", "zh_cn")?;
    let ui_language = parse_language(&language)?;
    let endpoint = select_endpoint(ui_language, "https://api.openai.com/v1")?;
    let api_key = prompt(
        label(
            ui_language,
            "API Key（可见输入，本地服务可留空）",
            "API Key (visible input; empty for local service)",
        ),
        "",
    )?;
    let model = prompt(label(ui_language, "模型", "Model"), "gpt-4o-mini")?;
    let api = prompt(
        label(
            ui_language,
            "API 类型 (responses/chat_completions)",
            "API type (responses/chat_completions)",
        ),
        "responses",
    )?;
    let confirm = prompt(
        label(
            ui_language,
            "确认策略 (always/risk_only/never)",
            "Confirmation policy (always/risk_only/never)",
        ),
        "risk_only",
    )?;
    let root = prompt(
        label(
            ui_language,
            "执行用户 (auto/normal/root)",
            "Execution user (auto/normal/root)",
        ),
        "auto",
    )?;
    let api_type = parse(
        &api,
        &[
            ("responses", ApiType::Responses),
            ("chat_completions", ApiType::ChatCompletions),
        ],
    )?;
    let execute_confirm_policy = parse(
        &confirm,
        &[
            ("always", ConfirmPolicy::Always),
            ("risk_only", ConfirmPolicy::RiskOnly),
            ("never", ConfirmPolicy::Never),
        ],
    )?;
    let execute_user_mode = parse(
        &root,
        &[
            ("auto", ExecuteUserMode::Auto),
            ("normal", ExecuteUserMode::Normal),
            ("root", ExecuteUserMode::Root),
        ],
    )?;
    let cfg = Config {
        api_key,
        endpoint,
        model,
        api_type,
        execute_confirm_policy,
        execute_user_mode,
        ui_language,
        ..Config::default()
    };
    validate_with_environment_key(&cfg)?;
    write_new(path, &cfg)
}

/// Creates or reconfigures all provider and model settings.
pub fn run_configure(path: &Path) -> Result<()> {
    if !path.exists() {
        return run_wizard(path);
    }
    // Read the stored value directly: an NL2SH_API_KEY environment override
    // must never be copied into config.toml merely by opening `/config`.
    let stored = fs::read_to_string(path)
        .with_context(|| format!("cannot read config {}", path.display()))?;
    let mut cfg: Config =
        toml::from_str(&stored).with_context(|| format!("invalid config {}", path.display()))?;
    cfg.validate_runtime()?;
    let default_language = match cfg.ui_language {
        UiLanguage::ZhCn => "zh_cn",
        UiLanguage::En => "en",
    };
    let language = prompt("界面语言 / UI language (zh_cn/en)", default_language)?;
    cfg.ui_language = parse_language(&language)?;
    println!(
        "{} {}",
        label(
            cfg.ui_language,
            "正在重新配置：",
            "Reconfiguring provider at"
        ),
        path.display()
    );
    cfg.endpoint = select_endpoint(cfg.ui_language, &cfg.endpoint)?;
    let api_key = prompt(
        label(
            cfg.ui_language,
            "API Key（可见输入，留空保持当前值）",
            "API Key (visible input; empty keeps current)",
        ),
        "",
    )?;
    if !api_key.is_empty() {
        cfg.api_key = api_key;
    }
    cfg.model = prompt(label(cfg.ui_language, "模型", "Model"), &cfg.model)?;
    let default_api = match cfg.api_type {
        ApiType::Responses => "responses",
        ApiType::ChatCompletions => "chat_completions",
    };
    let api = prompt(
        label(
            cfg.ui_language,
            "API 类型 (responses/chat_completions)",
            "API type (responses/chat_completions)",
        ),
        default_api,
    )?;
    cfg.api_type = parse(
        &api,
        &[
            ("responses", ApiType::Responses),
            ("chat_completions", ApiType::ChatCompletions),
        ],
    )?;
    validate_with_environment_key(&cfg)?;
    write_replace(path, &cfg)
}

/// Creates or updates endpoint, API key, and API protocol settings.
pub fn run_provider_configure(path: &Path) -> Result<()> {
    let mut cfg = load_stored_or_default(path)?;
    println!(
        "{} {}",
        label(cfg.ui_language, "正在配置 API：", "Configuring API at"),
        path.display()
    );
    cfg.endpoint = select_endpoint(cfg.ui_language, &cfg.endpoint)?;
    let api_key = prompt(
        label(
            cfg.ui_language,
            "API Key（可见输入，留空保持当前值）",
            "API Key (visible input; empty keeps current)",
        ),
        "",
    )?;
    if !api_key.is_empty() {
        cfg.api_key = api_key;
    }
    let default_api = match cfg.api_type {
        ApiType::Responses => "responses",
        ApiType::ChatCompletions => "chat_completions",
    };
    let api = prompt(
        label(
            cfg.ui_language,
            "API 类型 (responses/chat_completions)",
            "API type (responses/chat_completions)",
        ),
        default_api,
    )?;
    cfg.api_type = parse(
        &api,
        &[
            ("responses", ApiType::Responses),
            ("chat_completions", ApiType::ChatCompletions),
        ],
    )?;
    validate_with_environment_key(&cfg)?;
    write_upsert(path, &cfg)
}

/// Creates or updates only the model identifier.
pub fn run_model_configure(path: &Path) -> Result<()> {
    let mut cfg = load_stored_or_default(path)?;
    cfg.model = prompt(label(cfg.ui_language, "模型", "Model"), &cfg.model)?;
    let current = cfg
        .model_context_window
        .map_or_else(String::new, |value| value.to_string());
    let context = prompt(
        label(
            cfg.ui_language,
            "上下文窗口 Token（留空自动识别）",
            "Context-window tokens (empty for automatic detection)",
        ),
        &current,
    )?;
    cfg.model_context_window = parse_optional_positive(&context)?;
    let current_output = cfg
        .model_max_output_tokens
        .map_or_else(String::new, |value| value.to_string());
    let output = prompt(
        label(
            cfg.ui_language,
            "最大输出 Token（留空自动识别）",
            "Maximum output tokens (empty for automatic detection)",
        ),
        &current_output,
    )?;
    cfg.model_max_output_tokens = parse_optional_positive(&output)?;
    cfg.validate_runtime()?;
    write_upsert(path, &cfg)
}

fn parse_optional_positive(value: &str) -> Result<Option<u64>> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    let value = value
        .trim()
        .parse::<u64>()
        .context("invalid positive token limit")?;
    if value == 0 {
        bail!("token limit must be positive")
    }
    Ok(Some(value))
}

/// Fetches models from the configured OpenAI-compatible endpoint and lets the user select one.
pub async fn run_models_configure(path: &Path) -> Result<()> {
    let mut cfg = load_stored_or_default(path)?;
    println!(
        "{}",
        label(
            cfg.ui_language,
            "正在从 Provider 网络拉取可用模型…",
            "Fetching available models from the provider…"
        )
    );
    let result = build_metadata_client(&cfg).list_models(&cfg).await;
    let models = match result {
        Ok(models) if !models.is_empty() => models,
        Ok(_) => {
            println!(
                "{}",
                label(
                    cfg.ui_language,
                    "Provider 未返回模型，改为手工输入。",
                    "The provider returned no models; switching to manual input."
                )
            );
            return run_model_configure(path);
        }
        Err(error) => {
            println!(
                "{} {error:#}",
                label(
                    cfg.ui_language,
                    "模型列表拉取失败，改为手工输入：",
                    "Model discovery failed; switching to manual input:"
                )
            );
            return run_model_configure(path);
        }
    };
    for (index, model) in models.iter().enumerate() {
        let context = model
            .context_window
            .map_or_else(|| "?".into(), |value| value.to_string());
        println!("{:>3}. {}  context={context}", index + 1, model.id);
    }
    let choice = prompt(
        label(
            cfg.ui_language,
            "选择编号或直接输入模型名称",
            "Select a number or enter a model identifier",
        ),
        &cfg.model,
    )?;
    let selected = choice
        .parse::<usize>()
        .ok()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| models.get(index).cloned())
        .unwrap_or_else(|| ModelMetadata {
            context_window: known_context_window(&choice),
            max_output_tokens: None,
            id: choice,
        });
    cfg.model = selected.id;
    if cfg.model_context_window.is_none() {
        cfg.model_context_window = selected.context_window;
    }
    cfg.validate_runtime()?;
    write_upsert(path, &cfg)
}

/// Queries and displays balances without persisting or logging account data.
pub async fn run_balance_query(path: &Path) -> Result<()> {
    let cfg = load_stored_or_default(path)?;
    println!(
        "{}",
        label(
            cfg.ui_language,
            "正在从 Provider 网络查询余额…",
            "Fetching provider balance…"
        )
    );
    match build_account_client(&cfg) {
        Ok(client) => match client.balances(&cfg).await {
            Ok(balances) if balances.is_empty() => println!(
                "{}",
                label(
                    cfg.ui_language,
                    "Provider 未返回余额。",
                    "The provider returned no balances."
                )
            ),
            Ok(balances) => {
                for balance in balances {
                    println!("{} {}", balance.currency, balance.amount);
                }
            }
            Err(error) => println!(
                "{} {error:#}",
                label(cfg.ui_language, "余额查询失败：", "Balance lookup failed:")
            ),
        },
        Err(error) => println!(
            "{} {error:#}",
            label(
                cfg.ui_language,
                "当前 Provider 不支持余额查询：",
                "Balance lookup is unsupported for this provider:"
            )
        ),
    }
    println!(
        "{}",
        label(
            cfg.ui_language,
            "余额仅显示在当前终端，不写入审计日志。按 Enter 返回。",
            "Balance is shown only in this terminal and is not audited. Press Enter to return."
        )
    );
    let mut ignored = String::new();
    io::stdin()
        .read_line(&mut ignored)
        .context("failed to wait for balance acknowledgement")?;
    Ok(())
}

fn load_stored_or_default(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config {
            source: Some(path.to_path_buf()),
            ..Config::default()
        });
    }
    let stored = fs::read_to_string(path)
        .with_context(|| format!("cannot read config {}", path.display()))?;
    let cfg: Config =
        toml::from_str(&stored).with_context(|| format!("invalid config {}", path.display()))?;
    cfg.validate_runtime()?;
    Ok(cfg)
}

fn validate_with_environment_key(cfg: &Config) -> Result<()> {
    let mut effective = cfg.clone();
    if effective.api_key.trim().is_empty() {
        if let Ok(key) = std::env::var("NL2SH_API_KEY") {
            effective.api_key = key;
        }
    }
    effective.validate()
}

fn write_upsert(path: &Path, cfg: &Config) -> Result<()> {
    if path.exists() {
        write_replace(path, cfg)
    } else {
        write_new(path, cfg)
    }
}

/// Atomically persists a validated configuration from an in-TUI editor.
pub fn save_config(path: &Path, cfg: &Config) -> Result<()> {
    cfg.validate_runtime()?;
    write_upsert(path, cfg)
}

fn parse_language(value: &str) -> Result<UiLanguage> {
    parse(
        value,
        &[("zh_cn", UiLanguage::ZhCn), ("en", UiLanguage::En)],
    )
}

fn label(language: UiLanguage, zh_cn: &'static str, en: &'static str) -> &'static str {
    match language {
        UiLanguage::ZhCn => zh_cn,
        UiLanguage::En => en,
    }
}

fn select_endpoint(language: UiLanguage, current: &str) -> Result<String> {
    let custom_index = PROVIDERS.len() - 1;
    let mut selected = PROVIDERS
        .iter()
        .position(|provider| {
            provider.endpoint.is_some_and(|endpoint| {
                endpoint.trim_end_matches('/') == current.trim_end_matches('/')
            })
        })
        .unwrap_or(custom_index);
    let mut stdout = io::stdout();
    let _raw_mode = RawModeGuard::enter()?;
    let rendered_lines = PROVIDERS.len() + 1;
    let mut first_frame = true;

    loop {
        if !first_frame {
            execute!(stdout, cursor::MoveUp(rendered_lines as u16))?;
        }
        first_frame = false;
        execute!(stdout, terminal::Clear(ClearType::FromCursorDown))?;
        write!(
            stdout,
            "{}\r\n",
            label(
                language,
                "选择 API 服务商（↑/↓ 或 j/k，Enter 确认）",
                "Select API provider (Up/Down or j/k, Enter to confirm)"
            )
        )?;
        for (index, provider) in PROVIDERS.iter().enumerate() {
            let marker = if index == selected { ">" } else { " " };
            let endpoint = provider.endpoint.unwrap_or(current);
            write!(
                stdout,
                "{marker} {}  {}\r\n",
                provider.name(language),
                endpoint
            )?;
        }
        stdout.flush()?;

        let Event::Key(key) = event::read().context("failed to read provider selection")? else {
            continue;
        };
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            continue;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                selected = selected.checked_sub(1).unwrap_or(PROVIDERS.len() - 1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                selected = (selected + 1) % PROVIDERS.len();
            }
            KeyCode::Home => selected = 0,
            KeyCode::End => selected = PROVIDERS.len() - 1,
            KeyCode::Enter => break,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                bail!("provider selection cancelled")
            }
            _ => {}
        }
    }
    drop(_raw_mode);

    match PROVIDERS[selected].endpoint {
        Some(endpoint) => Ok(endpoint.into()),
        None => prompt(
            label(language, "自定义 API Base URL", "Custom API Base URL"),
            current,
        ),
    }
}

fn serialize(cfg: &Config) -> Result<String> {
    toml::to_string_pretty(cfg).context("failed to serialize configuration")
}

pub(super) fn write_new(path: &Path, cfg: &Config) -> Result<()> {
    let text = serialize(cfg)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("cannot create {}", path.display()))?;
    file.write_all(text.as_bytes())
        .context("cannot write configuration")?;
    Ok(())
}

fn write_replace(path: &Path, cfg: &Config) -> Result<()> {
    let text = serialize(cfg)?;
    let temporary = path.with_extension("toml.nl2sh-new");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("cannot create temporary config {}", temporary.display()))?;
    if let Err(error) = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
    {
        let _ = fs::remove_file(&temporary);
        return Err(error).context("cannot write replacement configuration");
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error).context("cannot replace configuration");
    }
    Ok(())
}

fn prompt(label: &str, default: &str) -> Result<String> {
    print!("{label} [{default}]: ");
    io::stdout().flush()?;
    let mut s = String::new();
    io::stdin().read_line(&mut s)?;
    let s = s.trim();
    Ok(if s.is_empty() {
        default.into()
    } else {
        s.into()
    })
}
fn parse<T: Copy>(value: &str, choices: &[(&str, T)]) -> Result<T> {
    choices
        .iter()
        .find(|x| x.0 == value)
        .map(|x| x.1)
        .with_context(|| format!("invalid choice: {value}"))
}
