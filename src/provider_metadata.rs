//! Provider-specific model discovery normalized for configuration and TUI use.

use crate::config::Config;
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

/// Provider family selected from the configured endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// OpenAI's public API.
    OpenAi,
    /// DeepSeek's public API.
    DeepSeek,
    /// SiliconFlow's public API.
    SiliconFlow,
    /// A local Ollama server.
    Ollama,
    /// An unknown OpenAI-compatible endpoint.
    OpenAiCompatible,
}

/// Provider-neutral model information used by the configuration UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelMetadata {
    /// Provider model identifier.
    pub id: String,
    /// Maximum input context when the provider exposes it or nl2sh knows it.
    pub context_window: Option<u64>,
    /// Maximum output tokens when known.
    pub max_output_tokens: Option<u64>,
}

/// Read-only provider metadata operations. Implementations must not log credentials or responses.
#[async_trait]
pub trait ProviderMetadataClient: Send + Sync {
    /// Lists models visible to the configured credential.
    async fn list_models(&self, config: &Config) -> Result<Vec<ModelMetadata>>;
}

/// Returns the metadata adapter appropriate for the configured endpoint.
pub fn build_metadata_client(config: &Config) -> Box<dyn ProviderMetadataClient> {
    Box::new(HttpProviderMetadataClient {
        kind: provider_kind(&config.endpoint),
    })
}

/// Detects a built-in provider without sending a request.
pub fn provider_kind(endpoint: &str) -> ProviderKind {
    if endpoint.contains("api.openai.com") {
        ProviderKind::OpenAi
    } else if endpoint.contains("api.deepseek.com") {
        ProviderKind::DeepSeek
    } else if endpoint.contains("api.siliconflow.cn") {
        ProviderKind::SiliconFlow
    } else if endpoint.contains("11434") {
        ProviderKind::Ollama
    } else {
        ProviderKind::OpenAiCompatible
    }
}

/// Returns a conservative built-in context-window value when known.
pub fn known_context_window(model: &str) -> Option<u64> {
    if model.starts_with("gpt-4.1") {
        Some(1_047_576)
    } else if model.starts_with("gpt-4o") {
        Some(128_000)
    } else if model.starts_with("deepseek-") {
        Some(1_000_000)
    } else {
        None
    }
}

struct HttpProviderMetadataClient {
    kind: ProviderKind,
}

#[async_trait]
impl ProviderMetadataClient for HttpProviderMetadataClient {
    async fn list_models(&self, config: &Config) -> Result<Vec<ModelMetadata>> {
        match self.kind {
            ProviderKind::Ollama => list_ollama_models(config).await,
            ProviderKind::OpenAi
            | ProviderKind::DeepSeek
            | ProviderKind::SiliconFlow
            | ProviderKind::OpenAiCompatible => list_openai_models(config).await,
        }
    }
}

async fn list_openai_models(config: &Config) -> Result<Vec<ModelMetadata>> {
    let client = crate::network::build_http_client(config)?;
    let value = send_json(
        client.get(format!("{}/models", config.endpoint.trim_end_matches('/'))),
        config,
    )
    .await?;
    let mut models = value["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| model["id"].as_str())
        .map(|id| ModelMetadata {
            id: id.to_owned(),
            context_window: known_context_window(id),
            max_output_tokens: None,
        })
        .collect::<Vec<_>>();
    normalize_models(&mut models);
    Ok(models)
}

async fn list_ollama_models(config: &Config) -> Result<Vec<ModelMetadata>> {
    let client = crate::network::build_http_client(config)?;
    let root = config
        .endpoint
        .trim_end_matches('/')
        .trim_end_matches("/v1");
    let value = send_json(client.get(format!("{root}/api/tags")), config).await?;
    let ids = value["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| model["model"].as_str().or_else(|| model["name"].as_str()))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut models = Vec::with_capacity(ids.len());
    for id in ids {
        let details = send_json(
            client
                .post(format!("{root}/api/show"))
                .json(&json!({ "model": id })),
            config,
        )
        .await
        .ok();
        let context_window = details
            .as_ref()
            .and_then(|value| value["model_info"].as_object())
            .and_then(|info| {
                info.iter()
                    .find(|(key, _)| key.ends_with(".context_length"))
                    .and_then(|(_, value)| value.as_u64())
            });
        models.push(ModelMetadata {
            id,
            context_window,
            max_output_tokens: None,
        });
    }
    normalize_models(&mut models);
    Ok(models)
}

async fn send_json(builder: reqwest::RequestBuilder, config: &Config) -> Result<Value> {
    let key = effective_key(config);
    let builder = if key.trim().is_empty() {
        builder
    } else {
        builder.bearer_auth(key)
    };
    let response = builder
        .send()
        .await
        .context("provider metadata request failed")?;
    let status = response.status();
    if !status.is_success() {
        bail!("provider metadata returned HTTP {status}")
    }
    response
        .json()
        .await
        .context("invalid provider metadata JSON")
}

fn effective_key(config: &Config) -> String {
    if config.api_key.trim().is_empty() {
        std::env::var("NL2SH_API_KEY").unwrap_or_default()
    } else {
        config.api_key.clone()
    }
}

fn normalize_models(models: &mut Vec<ModelMetadata>) {
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_builtin_provider_families_and_contexts() {
        assert_eq!(
            provider_kind("https://api.openai.com/v1"),
            ProviderKind::OpenAi
        );
        assert_eq!(
            provider_kind("https://api.deepseek.com"),
            ProviderKind::DeepSeek
        );
        assert_eq!(
            provider_kind("https://api.siliconflow.cn/v1"),
            ProviderKind::SiliconFlow
        );
        assert_eq!(
            provider_kind("http://127.0.0.1:11434/v1"),
            ProviderKind::Ollama
        );
        assert_eq!(known_context_window("gpt-4o-mini"), Some(128_000));
    }
}
