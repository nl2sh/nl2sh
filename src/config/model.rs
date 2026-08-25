use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use url::Url;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
/// Supported OpenAI-compatible wire protocol.
pub enum ApiType {
    #[default]
    /// Negotiates the protocol on the first request and caches the result.
    Auto,
    /// `/chat/completions` messages protocol.
    ChatCompletions,
    /// `/responses` item protocol.
    Responses,
}

impl ApiType {
    fn is_auto(&self) -> bool {
        *self == Self::Auto
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
/// General user confirmation preference, subordinate to tool policy.
pub enum ConfirmPolicy {
    /// Confirm every command.
    Always,
    #[default]
    /// Confirm commands according to assessed risk.
    RiskOnly,
    /// Skip general confirmation where mandatory safety policy permits.
    Never,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
/// Overall safety posture.
pub enum SecurityLevel {
    /// Confirm every command.
    Strict,
    #[default]
    /// Auto-run reads and confirm state changes.
    Balanced,
    /// Auto-run ordinary changes but retain dangerous confirmation.
    Unsafe,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
/// User identity used for command execution.
pub enum ExecuteUserMode {
    #[default]
    /// Elevate only when local policy says root is required.
    Auto,
    /// Never invoke `su`.
    Normal,
    /// Require UID zero or `su` for every command.
    Root,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
/// Language used by the terminal interface.
pub enum UiLanguage {
    #[default]
    /// Simplified Chinese interface.
    ZhCn,
    /// English interface.
    En,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
/// Preset task budget profile. Explicit limit fields remain authoritative.
pub enum AgentMode {
    /// Short diagnostics and simple changes.
    Fast,
    #[default]
    /// General Android and development work.
    Normal,
    /// Long debugging and build/fix/test loops.
    Deep,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
/// Explicit outbound proxy protocol.
pub enum ProxyType {
    #[default]
    /// HTTP proxy, including CONNECT for HTTPS destinations.
    Http,
    /// SOCKS5 with local DNS resolution.
    Socks5,
    /// SOCKS5 with proxy-side DNS resolution.
    Socks5h,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
/// Fully defaulted and validated runtime configuration.
pub struct Config {
    /// Optional bearer token; empty is valid for non-OpenAI local services.
    pub api_key: String,
    /// Enables the independent read-only Tencent ima connector.
    pub ima_enabled: bool,
    /// ima OpenAPI Client ID; never sent to the model or logs.
    pub ima_client_id: String,
    /// ima OpenAPI API key; never sent to the model or logs.
    pub ima_api_key: String,
    /// Optional default knowledge-base ID used instead of account discovery.
    pub ima_knowledge_base_id: Option<String>,
    /// Provider model identifier.
    pub model: String,
    /// Optional user/provider context-window override in tokens.
    pub model_context_window: Option<u64>,
    /// Optional user/provider maximum output-token override.
    pub model_max_output_tokens: Option<u64>,
    /// API base URL, normally ending in `/v1`.
    pub endpoint: String,
    /// Selected API wire protocol.
    #[serde(skip_serializing_if = "ApiType::is_auto")]
    pub api_type: ApiType,
    /// Master proxy switch. Disabling it preserves all proxy fields.
    pub proxy_enabled: bool,
    /// Proxy transport selected by the TUI.
    pub proxy_type: ProxyType,
    /// Proxy host and port without credentials or scheme.
    pub proxy_address: String,
    /// Optional proxy authentication username.
    pub proxy_username: String,
    /// Optional proxy authentication password.
    pub proxy_password: String,
    /// Comma-separated hosts which bypass the proxy.
    pub proxy_bypass: String,
    /// Release version suppressed by the user in the update prompt.
    pub skipped_update_version: Option<String>,
    /// Maximum complete text interaction units retained.
    pub max_context_turns: usize,
    /// Maximum model/tool iterations per request.
    pub max_agent_steps: usize,
    /// Named budget profile used as a user-facing hint.
    pub agent_mode: AgentMode,
    /// Maximum tool calls attempted during one task.
    pub max_tool_calls: usize,
    /// Maximum active wall-clock seconds, excluding confirmation waits.
    pub max_task_execution_time_secs: u64,
    /// Consecutive stalled steps before forcing a strategy change.
    pub replan_after_stalled_steps: usize,
    /// Consecutive stalled steps before ending the task.
    pub abort_after_stalled_steps: usize,
    /// Maximum executions of the same command with the same result.
    pub max_same_action_retries: usize,
    /// Built-in absolute step ceiling applied after user configuration.
    pub hard_max_agent_steps: usize,
    /// Number of retries after the initial LLM attempt.
    pub llm_retry_count: u32,
    /// Initial exponential retry delay.
    pub llm_retry_base_delay_ms: u64,
    /// HTTP request timeout.
    pub llm_request_timeout_secs: u64,
    /// Ordinary command timeout.
    pub execute_timeout_secs: u64,
    /// Interactive timeout; zero disables it.
    pub interactive_execute_timeout_secs: u64,
    /// General confirmation preference.
    pub execute_confirm_policy: ConfirmPolicy,
    /// Safety posture.
    pub security_level: SecurityLevel,
    /// Command execution identity mode.
    pub execute_user_mode: ExecuteUserMode,
    /// Enables real PTY execution instead of pipeline fallback.
    pub enable_pty: bool,
    /// Replaces Emoji labels with ASCII labels.
    pub ascii_symbols: bool,
    /// Shows the Buddha ASCII art in startup and help content.
    pub show_buddha_ascii_art: bool,
    /// Plays the one-shot ASCII train animation on startup.
    pub show_train_ascii_art: bool,
    /// Terminal interface language; Simplified Chinese is the default.
    pub ui_language: UiLanguage,
    /// JSON Lines interaction log, relative to the configuration directory by default.
    pub history_log_file: PathBuf,
    /// Maximum bytes retained from live command output in the TUI.
    pub ui_live_output_max_bytes: usize,
    /// Maximum bytes captured for one command result.
    pub tool_output_max_bytes: usize,
    /// Maximum bytes from one tool result sent back to the model.
    pub model_tool_output_max_bytes: usize,
    /// Maximum unencoded message bytes in one history event.
    pub history_log_event_max_bytes: usize,
    /// Maximum bytes written to one history log file per process run.
    pub history_log_max_bytes: u64,
    /// Additional rules that can only raise risk.
    pub security_rules: Vec<SecurityRuleConfig>,
    #[serde(skip)]
    /// File that produced this configuration.
    pub source: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// User-supplied regular-expression security rule.
pub struct SecurityRuleConfig {
    /// Stable identifier shown in assessments.
    pub id: String,
    /// Rust regular expression matched against normalized command text.
    pub pattern: String,
    /// `read_only`, `mutating`, `dangerous`, or `critical`.
    pub risk: String,
    /// User-facing explanation.
    pub message: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            ima_enabled: false,
            ima_client_id: String::new(),
            ima_api_key: String::new(),
            ima_knowledge_base_id: None,
            model: "gpt-4o-mini".into(),
            model_context_window: None,
            model_max_output_tokens: None,
            endpoint: "https://api.openai.com/v1".into(),
            api_type: ApiType::Auto,
            proxy_enabled: false,
            proxy_type: ProxyType::Http,
            proxy_address: String::new(),
            proxy_username: String::new(),
            proxy_password: String::new(),
            proxy_bypass: "localhost,127.0.0.1,::1".into(),
            skipped_update_version: None,
            max_context_turns: 16,
            max_agent_steps: 50,
            agent_mode: AgentMode::Normal,
            max_tool_calls: 100,
            max_task_execution_time_secs: 1800,
            replan_after_stalled_steps: 6,
            abort_after_stalled_steps: 12,
            max_same_action_retries: 3,
            hard_max_agent_steps: 200,
            llm_retry_count: 3,
            llm_retry_base_delay_ms: 500,
            llm_request_timeout_secs: 60,
            execute_timeout_secs: 30,
            interactive_execute_timeout_secs: 0,
            execute_confirm_policy: ConfirmPolicy::RiskOnly,
            security_level: SecurityLevel::Balanced,
            execute_user_mode: ExecuteUserMode::Auto,
            enable_pty: true,
            ascii_symbols: false,
            show_buddha_ascii_art: true,
            show_train_ascii_art: true,
            ui_language: UiLanguage::ZhCn,
            history_log_file: PathBuf::from("nl2sh.log"),
            ui_live_output_max_bytes: 256 * 1024,
            tool_output_max_bytes: 1024 * 1024,
            model_tool_output_max_bytes: 128 * 1024,
            history_log_event_max_bytes: 256 * 1024,
            history_log_max_bytes: 10 * 1024 * 1024,
            security_rules: Vec::new(),
            source: None,
        }
    }
}

impl Config {
    /// Applies the standard Fast, Normal, or Deep task budget as one atomic preset.
    pub fn apply_agent_mode(&mut self, mode: AgentMode) {
        self.agent_mode = mode;
        let (steps, tools, seconds) = match mode {
            AgentMode::Fast => (20, 40, 600),
            AgentMode::Normal => (50, 100, 1800),
            AgentMode::Deep => (100, 200, 3600),
        };
        self.max_agent_steps = steps;
        self.max_tool_calls = tools;
        self.max_task_execution_time_secs = seconds;
    }

    /// Validates URLs, bounds, enum-like rule values, and provider key needs.
    pub fn validate(&self) -> Result<()> {
        self.validate_runtime()?;
        if !self.provider_is_configured() {
            bail!("api_key is required for api.openai.com")
        }
        Ok(())
    }

    /// Validates runtime settings while allowing provider credentials to be
    /// completed later from the TUI.
    pub fn validate_runtime(&self) -> Result<()> {
        let url = Url::parse(&self.endpoint).context("endpoint is not a valid URL")?;
        if !matches!(url.scheme(), "http" | "https") {
            bail!("endpoint must use http or https")
        }
        if self.proxy_enabled {
            if self.proxy_address.trim().is_empty()
                || self.proxy_address.chars().any(char::is_whitespace)
            {
                bail!("enabled proxy requires a host:port address without whitespace")
            }
            Url::parse(&self.proxy_url()).context("proxy address is not valid")?;
        }
        if self.model.trim().is_empty() {
            bail!("model must not be empty")
        }
        if self.ima_enabled && !self.ima_is_configured() {
            bail!("enabled ima integration requires ima_client_id and ima_api_key")
        }
        if self.max_context_turns == 0
            || self.max_agent_steps == 0
            || self.max_tool_calls == 0
            || self.max_task_execution_time_secs == 0
            || self.replan_after_stalled_steps == 0
            || self.abort_after_stalled_steps == 0
            || self.max_same_action_retries == 0
            || self.hard_max_agent_steps == 0
        {
            bail!("context, step, tool, time, stall, retry, and hard limits must be positive")
        }
        if self.replan_after_stalled_steps >= self.abort_after_stalled_steps {
            bail!("replan_after_stalled_steps must be less than abort_after_stalled_steps")
        }
        if self.hard_max_agent_steps > crate::agent::SYSTEM_HARD_MAX_AGENT_STEPS {
            bail!("hard_max_agent_steps cannot exceed the system hard limit")
        }
        if self.model_context_window == Some(0) || self.model_max_output_tokens == Some(0) {
            bail!("model token limits must be positive when configured")
        }
        if self.llm_request_timeout_secs == 0 || self.execute_timeout_secs == 0 {
            bail!("request and execution timeouts must be positive")
        }
        if self.history_log_file.as_os_str().is_empty() {
            bail!("history_log_file must not be empty")
        }
        if self.ui_live_output_max_bytes < 256
            || self.tool_output_max_bytes < 256
            || self.model_tool_output_max_bytes < 256
            || self.history_log_event_max_bytes < 256
            || self.history_log_max_bytes < 512
        {
            bail!("output limits must be at least 256 bytes and history_log_max_bytes at least 512 bytes")
        }
        for rule in &self.security_rules {
            if rule.id.trim().is_empty() || rule.message.trim().is_empty() {
                bail!("custom security rule id and message must not be empty")
            }
            if !matches!(
                rule.risk.as_str(),
                "read_only" | "readonly" | "mutating" | "dangerous" | "critical"
            ) {
                bail!("invalid risk for security rule {}: {}", rule.id, rule.risk)
            }
            regex::Regex::new(&rule.pattern)
                .with_context(|| format!("invalid security rule {}", rule.id))?;
        }
        Ok(())
    }

    /// Returns the configured override or a conservative built-in model value.
    pub fn effective_context_window(&self) -> Option<u64> {
        self.model_context_window
            .or_else(|| crate::provider_metadata::known_context_window(&self.model))
    }

    /// Returns the input-token watermark used to preserve output headroom.
    pub fn effective_input_token_budget(&self) -> Option<u64> {
        self.effective_context_window().map(|window| {
            let safety_watermark = window.saturating_mul(85) / 100;
            let output_watermark = self
                .model_max_output_tokens
                .map_or(window, |output| window.saturating_sub(output));
            safety_watermark.min(output_watermark).max(1)
        })
    }

    /// Returns the credential-free proxy URL assembled from configured fields.
    pub fn proxy_url(&self) -> String {
        let scheme = match self.proxy_type {
            ProxyType::Http => "http",
            ProxyType::Socks5 => "socks5",
            ProxyType::Socks5h => "socks5h",
        };
        format!("{scheme}://{}", self.proxy_address.trim())
    }

    /// Reports whether the current endpoint has the credentials required by
    /// its built-in provider policy.
    pub fn provider_is_configured(&self) -> bool {
        Url::parse(&self.endpoint).is_ok_and(|url| {
            url.host_str() != Some("api.openai.com") || !self.api_key.trim().is_empty()
        }) && !self.model.trim().is_empty()
    }

    /// Reports whether both ima OpenAPI credentials are present.
    pub fn ima_is_configured(&self) -> bool {
        !self.ima_client_id.trim().is_empty() && !self.ima_api_key.trim().is_empty()
    }
}
