use super::{AgentMode, Config};
use anyhow::{Context, Result};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// Resolves `config.toml` beside the canonical executable path.
pub fn default_config_path() -> Result<PathBuf> {
    let exe = env::current_exe().context("cannot locate nl2sh executable")?;
    let canonical = fs::canonicalize(&exe).unwrap_or(exe);
    let parent = canonical
        .parent()
        .context("executable has no parent directory")?;
    Ok(parent.join("config.toml"))
}

/// Loads an explicit path or the executable-relative default.
pub fn load(explicit: Option<&Path>) -> Result<Config> {
    let config = load_unvalidated(explicit)?;
    config.validate()?;
    Ok(config)
}

/// Loads and overlays configuration without validation so CLI overrides can
/// be applied before the single authoritative validation pass.
pub fn load_unvalidated(explicit: Option<&Path>) -> Result<Config> {
    let path = match explicit {
        Some(p) => p.to_path_buf(),
        None => default_config_path()?,
    };
    load_from_unvalidated(&path)
}

/// Loads a configuration when present, or returns defaults associated with
/// the requested path so the TUI can start before provider setup.
pub fn load_or_default_unvalidated(path: &Path) -> Result<Config> {
    if path.exists() {
        return load_from_unvalidated(path);
    }
    let mut config = Config {
        source: Some(path.to_path_buf()),
        ..Config::default()
    };
    if let Ok(key) = env::var("NL2SH_API_KEY") {
        config.api_key = key;
    }
    apply_ima_environment(&mut config);
    Ok(config)
}

/// Loads, overlays the API-key environment variable, and validates TOML.
pub fn load_from(path: &Path) -> Result<Config> {
    let config = load_from_unvalidated(path)?;
    config.validate()?;
    Ok(config)
}

fn load_from_unvalidated(path: &Path) -> Result<Config> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("cannot read config {}", path.display()))?;
    let mut config: Config =
        toml::from_str(&text).with_context(|| format!("invalid config {}", path.display()))?;
    let document: toml::Value =
        toml::from_str(&text).with_context(|| format!("invalid config {}", path.display()))?;
    if document.get("agent_mode").is_some() {
        let mode: AgentMode = config.agent_mode;
        let explicit_steps = config.max_agent_steps;
        let explicit_tools = config.max_tool_calls;
        let explicit_time = config.max_task_execution_time_secs;
        config.apply_agent_mode(mode);
        if document.get("max_agent_steps").is_some() {
            config.max_agent_steps = explicit_steps;
        }
        if document.get("max_tool_calls").is_some() {
            config.max_tool_calls = explicit_tools;
        }
        if document.get("max_task_execution_time_secs").is_some() {
            config.max_task_execution_time_secs = explicit_time;
        }
    }
    if let Ok(key) = env::var("NL2SH_API_KEY") {
        config.api_key = key;
    }
    apply_ima_environment(&mut config);
    config.source = Some(path.to_path_buf());
    Ok(config)
}

fn apply_ima_environment(config: &mut Config) {
    let mut overridden = false;
    if let Ok(client_id) = env::var("NL2SH_IMA_CLIENT_ID") {
        config.ima_client_id = client_id;
        overridden = true;
    }
    if let Ok(api_key) = env::var("NL2SH_IMA_API_KEY") {
        config.ima_api_key = api_key;
        overridden = true;
    }
    if overridden && config.ima_is_configured() {
        config.ima_enabled = true;
    }
}
