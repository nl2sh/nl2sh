use super::{ApiType, Config};
use anyhow::{Context, Result};
use reqwest::Client;
use std::{path::Path, time::Duration};

const KONKA_COMPLETIONS_URL: &str = "http://10.48.22.11/internal/v1/chat/completions";
const KONKA_BASE_URL: &str = "http://10.48.22.11/internal/v1";
const KONKA_CREDENTIAL: &str = "KK-FREE-TEST";
const KONKA_MODEL: &str = "deepseek-v4-flash-0731";
const CONNECTIVITY_TIMEOUT: Duration = Duration::from_secs(2);

/// Creates the KONKA convenience configuration when the internal service is
/// reachable and no configuration file already exists.
pub async fn bootstrap_konka_provider_if_available(path: &Path) -> Result<bool> {
    bootstrap_with_url(path, KONKA_COMPLETIONS_URL).await
}

async fn bootstrap_with_url(path: &Path, probe_url: &str) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }

    let client = Client::builder()
        .connect_timeout(CONNECTIVITY_TIMEOUT)
        .timeout(CONNECTIVITY_TIMEOUT)
        .build()
        .context("failed to build KONKA provider connectivity probe")?;
    if client.get(probe_url).send().await.is_err() {
        return Ok(false);
    }
    if path.exists() {
        return Ok(false);
    }

    let config = Config {
        api_key: KONKA_CREDENTIAL.into(),
        model: KONKA_MODEL.into(),
        endpoint: KONKA_BASE_URL.into(),
        api_type: ApiType::Responses,
        source: Some(path.to_path_buf()),
        ..Config::default()
    };
    config.validate()?;
    super::wizard::write_new(path, &config)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load_from;
    use std::fs;
    use tempfile::tempdir;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn reachable_internal_service_creates_responses_config() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(wiremock::matchers::method("GET"))
            .respond_with(ResponseTemplate::new(405))
            .mount(&server)
            .await;
        let dir = tempdir()?;
        let path = dir.path().join("config.toml");

        assert!(bootstrap_with_url(&path, &server.uri()).await?);
        let config = load_from(&path)?;
        assert_eq!(config.endpoint, KONKA_BASE_URL);
        assert_eq!(config.api_key, KONKA_CREDENTIAL);
        assert_eq!(config.model, KONKA_MODEL);
        assert_eq!(config.api_type, ApiType::Responses);
        Ok(())
    }

    #[tokio::test]
    async fn existing_config_is_never_changed() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("config.toml");
        let original = "model='existing'\nendpoint='http://127.0.0.1/v1'\n";
        fs::write(&path, original)?;

        assert!(!bootstrap_with_url(&path, "not a valid URL").await?);
        assert_eq!(fs::read_to_string(path)?, original);
        Ok(())
    }

    #[tokio::test]
    async fn unreachable_service_does_not_create_config() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("config.toml");

        assert!(!bootstrap_with_url(&path, "http://127.0.0.1:1").await?);
        assert!(!path.exists());
        Ok(())
    }
}
