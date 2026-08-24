//! Read-only account information for providers with documented bearer-token APIs.

use crate::{
    config::Config,
    provider_metadata::{provider_kind, ProviderKind},
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::Value;

/// One normalized available-balance amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountBalance {
    /// ISO-like currency label supplied by the provider.
    pub currency: String,
    /// Provider-formatted available amount, kept as text to avoid rounding money.
    pub amount: String,
}

/// Read-only balance lookup. Implementations never expose credentials or raw responses.
#[async_trait]
pub trait ProviderAccountClient: Send + Sync {
    /// Returns available balances visible to the configured API token.
    async fn balances(&self, config: &Config) -> Result<Vec<AccountBalance>>;
}

/// Creates a balance client for a provider with a documented endpoint.
pub fn build_account_client(config: &Config) -> Result<Box<dyn ProviderAccountClient>> {
    match provider_kind(&config.endpoint) {
        ProviderKind::DeepSeek => Ok(Box::new(DeepSeekAccountClient)),
        ProviderKind::SiliconFlow => Ok(Box::new(SiliconFlowAccountClient)),
        _ => bail!("this provider has no supported public bearer-token balance API"),
    }
}

struct DeepSeekAccountClient;
struct SiliconFlowAccountClient;

#[async_trait]
impl ProviderAccountClient for DeepSeekAccountClient {
    async fn balances(&self, config: &Config) -> Result<Vec<AccountBalance>> {
        let value = get(config, "user/balance").await?;
        Ok(value["balance_infos"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|balance| {
                Some(AccountBalance {
                    currency: balance["currency"].as_str()?.to_owned(),
                    amount: balance["total_balance"].as_str()?.to_owned(),
                })
            })
            .collect())
    }
}

#[async_trait]
impl ProviderAccountClient for SiliconFlowAccountClient {
    async fn balances(&self, config: &Config) -> Result<Vec<AccountBalance>> {
        let value = get(config, "user/info").await?;
        let data = &value["data"];
        let amount = ["totalBalance", "balance", "chargeBalance"]
            .iter()
            .find_map(|field| value_as_amount(&data[*field]))
            .context("SiliconFlow response did not contain a supported balance field")?;
        Ok(vec![AccountBalance {
            currency: "CNY".into(),
            amount,
        }])
    }
}

async fn get(config: &Config, path: &str) -> Result<Value> {
    let key = if config.api_key.trim().is_empty() {
        std::env::var("NL2SH_API_KEY").unwrap_or_default()
    } else {
        config.api_key.clone()
    };
    if key.trim().is_empty() {
        bail!("API token is required for balance lookup")
    }
    let response = crate::network::build_http_client(config)?
        .get(format!(
            "{}/{}",
            config.endpoint.trim_end_matches('/'),
            path
        ))
        .bearer_auth(key)
        .send()
        .await
        .context("balance request failed")?;
    let status = response.status();
    if !status.is_success() {
        bail!("provider balance endpoint returned HTTP {status}")
    }
    response
        .json()
        .await
        .context("invalid provider balance JSON")
}

fn value_as_amount(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_f64().map(|amount| amount.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_provider_fails_before_network_access() {
        let config = Config::default();
        assert!(build_account_client(&config).is_err());
    }
}
