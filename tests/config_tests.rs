use nl2sh::config::{
    load_from, load_or_default_unvalidated, load_unvalidated, AgentMode, ApiType, Config,
    UiLanguage,
};
use std::fs;
use tempfile::tempdir;

#[test]
fn normal_toml_and_defaults() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        "model='local'\nendpoint='http://127.0.0.1:11434/v1'\napi_key=''\n",
    )?;
    let cfg = load_from(&path)?;
    assert_eq!(cfg.model, "local");
    assert_eq!(cfg.api_type, ApiType::Auto);
    assert_eq!(cfg.max_agent_steps, 50);
    assert_eq!(cfg.max_tool_calls, 100);
    assert_eq!(cfg.max_context_turns, 16);
    assert_eq!(cfg.model_tool_output_max_bytes, 128 * 1024);
    assert_eq!(cfg.history_log_max_bytes, 10 * 1024 * 1024);
    assert_eq!(cfg.ui_language, UiLanguage::ZhCn);
    assert!(cfg.show_buddha_ascii_art);
    assert!(cfg.show_train_ascii_art);
    Ok(())
}

#[test]
fn agent_mode_applies_a_preset_unless_limits_are_explicit() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("mode.toml");
    fs::write(
        &path,
        "model='local'\nendpoint='http://localhost/v1'\nagent_mode='fast'\n",
    )?;
    let fast = load_from(&path)?;
    assert_eq!(fast.agent_mode, AgentMode::Fast);
    assert_eq!((fast.max_agent_steps, fast.max_tool_calls), (20, 40));

    fs::write(
        &path,
        "model='local'\nendpoint='http://localhost/v1'\nagent_mode='deep'\nmax_agent_steps=80\n",
    )?;
    let custom = load_from(&path)?;
    assert_eq!(custom.max_agent_steps, 80);
    assert_eq!(custom.max_tool_calls, 200);
    Ok(())
}

#[test]
fn automatic_api_type_is_defaulted_and_omitted_when_saved() -> anyhow::Result<()> {
    let encoded = toml::to_string(&Config::default())?;
    assert!(!encoded.lines().any(|line| line.starts_with("api_type")));
    let decoded: Config = toml::from_str(&encoded)?;
    assert_eq!(decoded.api_type, ApiType::Auto);

    let forced = toml::to_string(&Config {
        api_type: ApiType::ChatCompletions,
        ..Config::default()
    })?;
    assert!(forced.contains("api_type = \"chat_completions\""));
    Ok(())
}

#[test]
fn missing_file_and_invalid_values_fail() -> anyhow::Result<()> {
    let dir = tempdir()?;
    assert!(load_from(&dir.path().join("missing")).is_err());
    let path = dir.path().join("bad.toml");
    fs::write(&path, "api_type='bogus'")?;
    assert!(load_from(&path).is_err());
    fs::write(
        &path,
        "endpoint='http://localhost'\nmodel='x'\nexecute_timeout_secs=0",
    )?;
    assert!(load_from(&path).is_err());
    Ok(())
}

#[test]
fn missing_file_can_supply_unconfigured_tui_defaults_without_writing() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("missing.toml");
    let cfg = load_or_default_unvalidated(&path)?;
    assert!(cfg.validate_runtime().is_ok());
    let key_configured = std::env::var("NL2SH_API_KEY").is_ok_and(|key| !key.trim().is_empty());
    assert_eq!(cfg.provider_is_configured(), key_configured);
    assert!(!path.exists());
    assert_eq!(cfg.source.as_deref(), Some(path.as_path()));
    Ok(())
}

#[test]
fn empty_key_is_valid_for_local_endpoint() {
    let cfg = Config {
        endpoint: "http://127.0.0.1:8080/v1".into(),
        api_key: String::new(),
        ..Config::default()
    };
    assert!(cfg.validate().is_ok());
}

#[test]
fn runtime_validation_allows_tui_before_openai_credentials_exist() {
    let cfg = Config::default();
    assert!(cfg.validate_runtime().is_ok());
    assert!(!cfg.provider_is_configured());
    assert!(cfg.validate().is_err());
}

#[test]
fn cli_layer_can_override_before_validation() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("override.toml");
    fs::write(&path, "model=''\nendpoint='not-a-url'\n")?;
    let mut cfg = load_unvalidated(Some(&path))?;
    assert!(cfg.validate().is_err());
    cfg.model = "local".into();
    cfg.endpoint = "http://127.0.0.1:11434/v1".into();
    assert!(cfg.validate().is_ok());
    Ok(())
}
