//! Shared outbound HTTP client construction with credential-safe proxy policy.

use crate::config::Config;
use anyhow::{Context, Result};
use reqwest::{Client, ClientBuilder, NoProxy, Proxy};
use std::time::Duration;

/// Builds a rustls client used by every Provider-facing request.
pub fn build_http_client(config: &Config) -> Result<Client> {
    let mut builder = ClientBuilder::new()
        .no_proxy()
        .timeout(Duration::from_secs(config.llm_request_timeout_secs));
    if config.proxy_enabled {
        let mut proxy = Proxy::all(config.proxy_url()).context("invalid proxy configuration")?;
        if !config.proxy_username.is_empty() {
            proxy = proxy.basic_auth(&config.proxy_username, &config.proxy_password);
        }
        proxy = proxy.no_proxy(NoProxy::from_string(&config.proxy_bypass));
        builder = builder.proxy(proxy);
    }
    builder.build().context("failed to build HTTP client")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProxyType;

    #[test]
    fn disabled_proxy_ignores_but_preserves_incomplete_settings() -> Result<()> {
        let config = Config {
            proxy_enabled: false,
            proxy_address: "saved-for-later".into(),
            proxy_password: "secret".into(),
            ..Config::default()
        };
        build_http_client(&config)?;
        assert_eq!(config.proxy_address, "saved-for-later");
        assert_eq!(config.proxy_password, "secret");
        Ok(())
    }

    #[test]
    fn all_supported_proxy_protocols_build() -> Result<()> {
        for proxy_type in [ProxyType::Http, ProxyType::Socks5, ProxyType::Socks5h] {
            let config = Config {
                proxy_enabled: true,
                proxy_type,
                proxy_address: "127.0.0.1:1080".into(),
                ..Config::default()
            };
            config.validate_runtime()?;
            build_http_client(&config)?;
        }
        Ok(())
    }
}
