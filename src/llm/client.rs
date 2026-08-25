use super::{chat_completions, responses, retry, LlmRequest, LlmResponse};
use crate::config::{ApiType, Config};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use std::sync::atomic::{AtomicU8, Ordering};

const DIALECT_UNKNOWN: u8 = 0;
const DIALECT_RESPONSES: u8 = 1;
const DIALECT_CHAT_COMPLETIONS: u8 = 2;

/// Receives model-authored text as it arrives from a streaming response.
pub trait TextDeltaSink: Send + Sync {
    /// Starts a new provider generation.
    fn begin(&self) {}
    /// Appends one text fragment in provider order.
    fn delta(&self, text: &str);
    /// Finishes the current generation. `completed` is false on failure/cancellation.
    fn end(&self, _completed: bool) {}
}

#[async_trait]
/// Provider-neutral asynchronous language-model client.
pub trait LlmClient: Send + Sync {
    /// Completes one request and converts the provider response to internal types.
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse>;

    /// Completes one request while forwarding model text incrementally.
    async fn complete_stream(
        &self,
        request: LlmRequest,
        sink: &dyn TextDeltaSink,
    ) -> Result<LlmResponse> {
        sink.begin();
        let result = self.complete(request).await;
        if let Ok(response) = &result {
            if let Some(text) = &response.text {
                sink.delta(text);
            }
        }
        sink.end(result.is_ok());
        result
    }
}
/// OpenAI-compatible HTTP client configured for one concrete API dialect.
pub struct HttpLlmClient {
    client: Client,
    endpoint: String,
    key: String,
    api_type: ApiType,
    negotiated_api_type: AtomicU8,
    retries: u32,
    base_delay: u64,
}
/// Builds a rustls-backed HTTP client from validated configuration.
pub fn build_client(cfg: &Config) -> Result<HttpLlmClient> {
    Ok(HttpLlmClient {
        client: crate::network::build_http_client(cfg)?,
        endpoint: cfg.endpoint.trim_end_matches('/').into(),
        key: cfg.api_key.clone(),
        api_type: cfg.api_type,
        negotiated_api_type: AtomicU8::new(match cfg.api_type {
            ApiType::Auto => DIALECT_UNKNOWN,
            ApiType::Responses => DIALECT_RESPONSES,
            ApiType::ChatCompletions => DIALECT_CHAT_COMPLETIONS,
        }),
        retries: cfg.llm_retry_count,
        base_delay: cfg.llm_retry_base_delay_ms,
    })
}

#[async_trait]
impl LlmClient for HttpLlmClient {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
        let api_type = self.selected_api_type();
        if api_type != ApiType::Auto {
            return self.complete_for(request, api_type).await;
        }
        match self.complete_for(request.clone(), ApiType::Responses).await {
            Ok(response) => {
                self.cache_api_type(ApiType::Responses);
                Ok(response)
            }
            Err(error) if is_protocol_mismatch(&error) => {
                let response = self.complete_for(request, ApiType::ChatCompletions).await?;
                self.cache_api_type(ApiType::ChatCompletions);
                Ok(response)
            }
            Err(error) => Err(error),
        }
    }

    async fn complete_stream(
        &self,
        request: LlmRequest,
        sink: &dyn TextDeltaSink,
    ) -> Result<LlmResponse> {
        sink.begin();
        let tracking_sink = TrackingSink::new(sink);
        let api_type = self.selected_api_type();
        let result = if api_type != ApiType::Auto {
            self.complete_stream_for(request, &tracking_sink, api_type)
                .await
        } else {
            match self
                .complete_stream_for(request.clone(), &tracking_sink, ApiType::Responses)
                .await
            {
                Ok(response) => {
                    self.cache_api_type(ApiType::Responses);
                    Ok(response)
                }
                Err(error) if !tracking_sink.emitted() && is_protocol_mismatch(&error) => {
                    let fallback = self
                        .complete_stream_for(request, &tracking_sink, ApiType::ChatCompletions)
                        .await;
                    if fallback.is_ok() {
                        self.cache_api_type(ApiType::ChatCompletions);
                    }
                    fallback
                }
                Err(error) => Err(error),
            }
        };
        sink.end(result.is_ok());
        result
    }
}

impl HttpLlmClient {
    fn selected_api_type(&self) -> ApiType {
        match self.negotiated_api_type.load(Ordering::Acquire) {
            DIALECT_RESPONSES => ApiType::Responses,
            DIALECT_CHAT_COMPLETIONS => ApiType::ChatCompletions,
            _ => ApiType::Auto,
        }
    }

    fn cache_api_type(&self, api_type: ApiType) {
        if self.api_type != ApiType::Auto {
            return;
        }
        let value = match api_type {
            ApiType::Responses => DIALECT_RESPONSES,
            ApiType::ChatCompletions => DIALECT_CHAT_COMPLETIONS,
            ApiType::Auto => DIALECT_UNKNOWN,
        };
        self.negotiated_api_type.store(value, Ordering::Release);
    }

    async fn complete_for(&self, request: LlmRequest, api_type: ApiType) -> Result<LlmResponse> {
        let suffix = match api_type {
            ApiType::ChatCompletions => "chat/completions",
            ApiType::Responses => "responses",
            ApiType::Auto => return Err(anyhow!("automatic API negotiation was not resolved")),
        };
        let url = format!("{}/{}", self.endpoint, suffix);
        let body = match api_type {
            ApiType::ChatCompletions => chat_completions::request(&request),
            ApiType::Responses => responses::request(&request),
            ApiType::Auto => return Err(anyhow!("automatic API negotiation was not resolved")),
        };
        for attempt in 0..=self.retries {
            let mut builder = self.client.post(&url).json(&body);
            if !self.key.is_empty() {
                builder = builder.bearer_auth(&self.key)
            }
            let sent = tokio::select! {
                response = builder.send() => response,
                signal = tokio::signal::ctrl_c() => {
                    signal?;
                    return Err(anyhow!("LLM request cancelled by user"));
                }
            };
            match sent {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        let response = retry::checked(resp).await?;
                        let value = tokio::select! {
                            value = response.json() => value.context("invalid LLM JSON")?,
                            signal = tokio::signal::ctrl_c() => {
                                signal?;
                                return Err(anyhow!("LLM response cancelled by user"));
                            }
                        };
                        return match api_type {
                            ApiType::ChatCompletions => chat_completions::response(value),
                            ApiType::Responses => responses::response(value),
                            ApiType::Auto => {
                                Err(anyhow!("automatic API negotiation was not resolved"))
                            }
                        };
                    }
                    if !retry::retryable_status(status) || attempt == self.retries {
                        return Err(http_error(status, resp, &self.key).await);
                    }
                }
                Err(e) => {
                    if attempt == self.retries {
                        return Err(e).context("LLM request failed");
                    }
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(retry::delay(self.base_delay, attempt)) => {},
                signal = tokio::signal::ctrl_c() => {
                    signal?;
                    return Err(anyhow!("LLM retry cancelled by user"));
                }
            }
        }
        Err(anyhow!("retry loop ended unexpectedly"))
    }

    async fn complete_stream_for(
        &self,
        request: LlmRequest,
        sink: &dyn TextDeltaSink,
        api_type: ApiType,
    ) -> Result<LlmResponse> {
        let suffix = match api_type {
            ApiType::ChatCompletions => "chat/completions",
            ApiType::Responses => "responses",
            ApiType::Auto => return Err(anyhow!("automatic API negotiation was not resolved")),
        };
        let url = format!("{}/{}", self.endpoint, suffix);
        let mut body = match api_type {
            ApiType::ChatCompletions => chat_completions::request(&request),
            ApiType::Responses => responses::request(&request),
            ApiType::Auto => return Err(anyhow!("automatic API negotiation was not resolved")),
        };
        body["stream"] = serde_json::Value::Bool(true);
        for attempt in 0..=self.retries {
            let mut builder = self.client.post(&url).json(&body);
            if !self.key.is_empty() {
                builder = builder.bearer_auth(&self.key);
            }
            let sent = tokio::select! {
                response = builder.send() => response,
                signal = tokio::signal::ctrl_c() => {
                    signal?;
                    return Err(anyhow!("LLM request cancelled by user"));
                }
            };
            match sent {
                Ok(resp) if resp.status().is_success() => {
                    return super::streaming::parse(resp, api_type, sink).await;
                }
                Ok(resp) => {
                    let status = resp.status();
                    if !retry::retryable_status(status) || attempt == self.retries {
                        return Err(http_error(status, resp, &self.key).await);
                    }
                }
                Err(error) if attempt == self.retries => {
                    return Err(error).context("LLM request failed");
                }
                Err(_) => {}
            }
            tokio::select! {
                _ = tokio::time::sleep(retry::delay(self.base_delay, attempt)) => {},
                signal = tokio::signal::ctrl_c() => {
                    signal?;
                    return Err(anyhow!("LLM retry cancelled by user"));
                }
            }
        }
        Err(anyhow!("retry loop ended unexpectedly"))
    }
}

struct TrackingSink<'a> {
    inner: &'a dyn TextDeltaSink,
    emitted: AtomicU8,
}

impl<'a> TrackingSink<'a> {
    fn new(inner: &'a dyn TextDeltaSink) -> Self {
        Self {
            inner,
            emitted: AtomicU8::new(0),
        }
    }

    fn emitted(&self) -> bool {
        self.emitted.load(Ordering::Acquire) != 0
    }
}

impl TextDeltaSink for TrackingSink<'_> {
    fn delta(&self, text: &str) {
        if !text.is_empty() {
            self.emitted.store(1, Ordering::Release);
        }
        self.inner.delta(text);
    }
}

fn is_protocol_mismatch(error: &anyhow::Error) -> bool {
    let message = format!("{error:#}").to_ascii_lowercase();
    message.contains("empty responses output")
        || message.contains("responses stream ended before completion")
        || message.contains("invalid llm json")
        || message.contains("llm http 404")
        || message.contains("llm http 405")
        || ((message.contains("unsupported") || message.contains("not supported"))
            && (message.contains("responses") || message.contains("endpoint")))
}
async fn http_error(
    status: StatusCode,
    response: reqwest::Response,
    api_key: &str,
) -> anyhow::Error {
    let mut text = response.text().await.unwrap_or_default();
    if !api_key.is_empty() {
        text = text.replace(api_key, "[REDACTED]");
    }
    anyhow!(
        "LLM HTTP {status}: {}",
        text.chars().take(512).collect::<String>()
    )
}
