//! Read-only Tencent ima knowledge-base integration.

use crate::config::Config;
use anyhow::{bail, Context, Result};
use reqwest::{header::HeaderMap, redirect::Policy, Client, ClientBuilder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashSet, time::Duration};
use url::Url;

const IMA_BASE_URL: &str = "https://ima.qq.com";
const MAX_API_BYTES: usize = 4 * 1024 * 1024;
const MAX_ORIGINAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_BASES: usize = 20;
const MAX_SEARCH_BASES: usize = 10;

#[derive(Clone)]
/// Credential-isolated, no-proxy client for read-only ima operations.
pub struct ImaClient {
    client: Client,
    client_id: String,
    api_key: String,
    base_url: String,
    default_knowledge_base_id: Option<String>,
}

#[derive(Debug, Deserialize)]
/// Arguments accepted by the ima knowledge-base search tool.
pub struct ImaSearchArgs {
    /// Natural-language or keyword query.
    pub query: String,
    /// Optional ima knowledge-base ID. Defaults to configured ID or bounded discovery.
    pub knowledge_base_id: Option<String>,
}

#[derive(Debug, Deserialize)]
/// Arguments accepted by the ima original-content reader.
pub struct ImaReadArgs {
    /// Media ID returned by `ima_search`.
    pub media_id: String,
}

impl ImaClient {
    /// Builds a client only when all ima credentials are configured.
    pub fn from_config(config: &Config) -> Result<Option<Self>> {
        if !config.ima_enabled {
            return Ok(None);
        }
        if !config.ima_is_configured() {
            bail!("ima is enabled but client ID or API key is missing")
        }
        Self::new(
            &config.ima_client_id,
            &config.ima_api_key,
            config.llm_request_timeout_secs,
            config.ima_knowledge_base_id.clone(),
        )
        .map(Some)
    }

    fn new(
        client_id: &str,
        api_key: &str,
        timeout_secs: u64,
        default_knowledge_base_id: Option<String>,
    ) -> Result<Self> {
        Self::with_base_url(
            client_id,
            api_key,
            timeout_secs,
            default_knowledge_base_id,
            IMA_BASE_URL,
        )
    }

    fn with_base_url(
        client_id: &str,
        api_key: &str,
        timeout_secs: u64,
        default_knowledge_base_id: Option<String>,
        base_url: &str,
    ) -> Result<Self> {
        let client = ClientBuilder::new()
            .no_proxy()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .context("failed to build ima HTTP client")?;
        Ok(Self {
            client,
            client_id: client_id.to_owned(),
            api_key: api_key.to_owned(),
            base_url: base_url.trim_end_matches('/').to_owned(),
            default_knowledge_base_id: default_knowledge_base_id
                .filter(|value| !value.trim().is_empty()),
        })
    }

    /// Lists a bounded first page of knowledge bases visible to the account.
    pub async fn list_knowledge_bases(&self) -> Result<String> {
        let data = self
            .post_wiki(
                "search_knowledge_base",
                json!({"query": "", "cursor": "", "limit": MAX_BASES}),
            )
            .await?;
        let bases = data
            .get("info_list")
            .and_then(Value::as_array)
            .context("ima knowledge-base response omitted info_list")?;
        let mut output = String::new();
        for base in bases.iter().take(MAX_BASES) {
            let id = required_alias(base, "id", "kb_id", "ima knowledge base")?;
            let name = required_alias(base, "name", "kb_name", "ima knowledge base")?;
            output.push_str(&format!("- name={name}\n  knowledge_base_id={id}\n"));
        }
        if output.is_empty() {
            output.push_str("No accessible ima knowledge bases were returned.");
        }
        Ok(output)
    }

    /// Searches one configured base or a bounded set discovered from the account.
    pub async fn search(&self, args: &ImaSearchArgs) -> Result<String> {
        let query = args.query.trim();
        if query.is_empty() {
            bail!("ima search query must not be empty")
        }
        let base_ids = if let Some(id) = args
            .knowledge_base_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
            .or(self.default_knowledge_base_id.as_deref())
        {
            vec![(id.to_owned(), String::new())]
        } else {
            self.discover_bases().await?
        };
        let mut output = String::new();
        for (base_id, base_name) in base_ids.into_iter().take(MAX_SEARCH_BASES) {
            let data = self
                .post_wiki(
                    "search_knowledge",
                    json!({"query": query, "cursor": "", "knowledge_base_id": base_id}),
                )
                .await
                .with_context(|| format!("ima search failed for knowledge base {base_name}"))?;
            let items = data
                .get("info_list")
                .and_then(Value::as_array)
                .context("ima search response omitted info_list")?;
            for item in items.iter().take(20) {
                let Some(media_id) = item.get("media_id").and_then(Value::as_str) else {
                    continue;
                };
                let title = required_string(item, "title", "ima search item")?;
                let highlight = item
                    .get("highlight_content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                output.push_str(&format!(
                    "- knowledge_base={base_name}\n  knowledge_base_id={base_id}\n  title={title}\n  media_id={media_id}\n  highlight={highlight}\n"
                ));
            }
        }
        if output.is_empty() {
            output.push_str("No matching ima knowledge was found.");
        }
        Ok(output)
    }

    /// Reads original text for an ima media ID without exposing signed URLs or headers.
    pub async fn read(&self, args: &ImaReadArgs) -> Result<String> {
        let media_id = args.media_id.trim();
        if media_id.is_empty() {
            bail!("ima media_id must not be empty")
        }
        let data = self
            .post_wiki("get_media_info", json!({"media_id": media_id}))
            .await?;
        let media_type = data
            .get("media_type")
            .and_then(Value::as_i64)
            .context("ima media response omitted media_type")?;
        if media_type == 11 {
            let note_id = data
                .pointer("/notebook_ext_info/notebook_id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .context("ima note media response omitted notebook_id")?;
            let note = self
                .post_note(
                    "get_doc_content",
                    json!({"note_id": note_id, "target_content_format": 0}),
                )
                .await?;
            return note
                .get("content")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .context("ima note response omitted content");
        }
        let url = data
            .pointer("/url_info/url")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .context("ima media has no readable original URL; use the ima client to view it")?;
        validate_media_url(url)?;
        let mut request = self.client.get(url);
        if let Some(headers) = data.pointer("/url_info/headers").and_then(Value::as_object) {
            let mut safe_headers = HeaderMap::new();
            for (name, value) in headers {
                let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                    .context("ima media returned an invalid temporary header name")?;
                if matches!(
                    name.as_str(),
                    "host"
                        | "content-length"
                        | "transfer-encoding"
                        | "connection"
                        | "proxy-authorization"
                        | "cookie"
                ) {
                    bail!("ima media returned a forbidden transport header")
                }
                let value = value
                    .as_str()
                    .context("ima media returned a non-string temporary header")?
                    .parse()
                    .context("ima media returned an invalid temporary header value")?;
                safe_headers.insert(name, value);
            }
            request = request.headers(safe_headers);
        }
        let response = request
            .send()
            .await
            .context("ima original-content request failed")?;
        if !response.status().is_success() {
            bail!(
                "ima original-content request returned HTTP {}",
                response.status()
            )
        }
        let bytes = read_bounded(response, MAX_ORIGINAL_BYTES).await?;
        String::from_utf8(bytes).context("ima original content is not UTF-8 text")
    }

    async fn discover_bases(&self) -> Result<Vec<(String, String)>> {
        let data = self
            .post_wiki(
                "search_knowledge_base",
                json!({"query": "", "cursor": "", "limit": MAX_BASES}),
            )
            .await?;
        let items = data
            .get("info_list")
            .and_then(Value::as_array)
            .context("ima knowledge-base response omitted info_list")?;
        let mut seen = HashSet::new();
        let mut bases = Vec::new();
        for item in items.iter().take(MAX_SEARCH_BASES) {
            let id = required_alias(item, "id", "kb_id", "ima knowledge base")?.to_owned();
            if seen.insert(id.clone()) {
                bases.push((
                    id,
                    required_alias(item, "name", "kb_name", "ima knowledge base")?.to_owned(),
                ));
            }
        }
        if bases.is_empty() {
            bail!("no accessible ima knowledge bases were returned")
        }
        Ok(bases)
    }

    async fn post_wiki(&self, endpoint: &str, body: Value) -> Result<Value> {
        self.post("wiki", endpoint, body).await
    }

    async fn post_note(&self, endpoint: &str, body: Value) -> Result<Value> {
        self.post("note", endpoint, body).await
    }

    async fn post(&self, family: &str, endpoint: &str, body: Value) -> Result<Value> {
        let url = format!("{}/openapi/{family}/v1/{endpoint}", self.base_url);
        let response = self
            .client
            .post(url)
            .header("ima-openapi-clientid", &self.client_id)
            .header("ima-openapi-apikey", &self.api_key)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("ima {endpoint} request failed"))?;
        if !response.status().is_success() {
            bail!("ima {endpoint} returned HTTP {}", response.status())
        }
        let bytes = read_bounded(response, MAX_API_BYTES).await?;
        let envelope: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("ima {endpoint} returned invalid JSON"))?;
        let code_value = envelope
            .get("code")
            .or_else(|| envelope.get("retcode"))
            .context("ima response omitted status code")?;
        let code = code_value
            .as_i64()
            .or_else(|| code_value.as_str().and_then(|value| value.parse().ok()))
            .context("ima response returned an invalid status code")?;
        if code != 0 {
            let message = envelope
                .get("msg")
                .or_else(|| envelope.get("errmsg"))
                .and_then(Value::as_str)
                .unwrap_or("request rejected");
            bail!("ima {endpoint} failed with code {code}: {message}")
        }
        envelope
            .get("data")
            .cloned()
            .filter(Value::is_object)
            .context("ima response omitted object data")
    }
}

async fn read_bounded(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        bail!("ima response exceeds the {limit}-byte limit")
    }
    let mut output = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("failed to read ima response")?
    {
        if output.len().saturating_add(chunk.len()) > limit {
            bail!("ima response exceeds the {limit}-byte limit")
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}

fn required_string<'a>(value: &'a Value, field: &str, kind: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .with_context(|| format!("{kind} omitted {field}"))
}

fn required_alias<'a>(value: &'a Value, primary: &str, alias: &str, kind: &str) -> Result<&'a str> {
    let primary_value = value.get(primary).and_then(Value::as_str);
    let alias_value = value.get(alias).and_then(Value::as_str);
    match (primary_value, alias_value) {
        (Some(left), Some(right)) if left != right => {
            bail!("{kind} returned conflicting {primary} and {alias}")
        }
        (Some(value), _) | (_, Some(value)) if !value.is_empty() => Ok(value),
        _ => bail!("{kind} omitted {primary}"),
    }
}

fn validate_media_url(raw: &str) -> Result<()> {
    let url = Url::parse(raw).context("ima returned an invalid original-content URL")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        bail!("ima original-content URL violates HTTPS origin policy")
    }
    let host = url
        .host_str()
        .context("ima original-content URL has no host")?;
    if host != "ima.qq.com" && host != "mp.weixin.qq.com" && !host.ends_with(".myqcloud.com") {
        bail!("ima original-content URL host is not allowed")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{body_json, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    async fn searches_and_reads_note_without_exposing_credentials() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/openapi/wiki/v1/search_knowledge"))
            .and(header("ima-openapi-clientid", "client-test"))
            .and(header("ima-openapi-apikey", "key-test"))
            .and(body_json(json!({"query":"android","cursor":"","knowledge_base_id":"kb-test"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"code":0,"msg":"ok","data":{"info_list":[{"media_id":"media-test","title":"Guide","highlight_content":"match"}]}})))
            .mount(&server).await;
        Mock::given(method("POST")).and(path("/openapi/wiki/v1/get_media_info"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"code":0,"data":{"media_type":11,"notebook_ext_info":{"notebook_id":"note-test"}}}))).mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/openapi/note/v1/get_doc_content"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"code":0,"data":{"content":"正文"}})),
            )
            .mount(&server)
            .await;
        let client = ImaClient::with_base_url(
            "client-test",
            "key-test",
            5,
            Some("kb-test".into()),
            &server.uri(),
        )?;
        let searched = client
            .search(&ImaSearchArgs {
                query: "android".into(),
                knowledge_base_id: None,
            })
            .await?;
        assert!(searched.contains("media_id=media-test"));
        assert_eq!(
            client
                .read(&ImaReadArgs {
                    media_id: "media-test".into()
                })
                .await?,
            "正文"
        );
        assert!(!searched.contains("key-test"));
        Ok(())
    }

    #[test]
    fn rejects_untrusted_media_origins() {
        assert!(validate_media_url("https://example.com/file").is_err());
        assert!(validate_media_url("http://ima.qq.com/file").is_err());
        assert!(validate_media_url("https://bucket.cos.ap.myqcloud.com/file").is_ok());
    }

    #[tokio::test]
    #[ignore = "requires explicit NL2SH_IMA_CLIENT_ID and NL2SH_IMA_API_KEY"]
    async fn live_readonly_smoke() -> Result<()> {
        let client_id =
            std::env::var("NL2SH_IMA_CLIENT_ID").context("NL2SH_IMA_CLIENT_ID is required")?;
        let api_key =
            std::env::var("NL2SH_IMA_API_KEY").context("NL2SH_IMA_API_KEY is required")?;
        let client = ImaClient::new(&client_id, &api_key, 30, None)?;
        let bases = client.list_knowledge_bases().await?;
        if bases.starts_with("No accessible") {
            bail!("live ima account returned no accessible knowledge bases")
        }
        let _ = client
            .search(&ImaSearchArgs {
                query: "Android".into(),
                knowledge_base_id: None,
            })
            .await?;
        Ok(())
    }
}
