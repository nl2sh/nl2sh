use crate::{config::Config, network::build_tool_http_client};
use anyhow::{bail, Context, Result};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::IpAddr,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use url::{Host, Url};

const DEFAULT_MAX_BYTES: usize = 256 * 1024;
const HARD_MAX_BYTES: usize = 2 * 1024 * 1024;
const POST_MAX_BODY_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HttpRequestArgs {
    pub url: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub max_bytes: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DownloadUrlArgs {
    pub url: String,
    pub path: String,
    #[serde(default)]
    pub max_bytes: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HttpPostArgs {
    pub url: String,
    pub body: serde_json::Value,
    #[serde(default)]
    pub max_bytes: Option<usize>,
}

pub fn post_summary(args: &HttpPostArgs) -> Result<String> {
    let bytes = serde_json::to_vec(&args.body).context("cannot encode JSON request body")?;
    if bytes.len() > POST_MAX_BODY_BYTES {
        bail!("JSON request body exceeds the hard byte limit")
    }
    Ok(format!(
        "POST JSON to: {}\nRequest bytes: {}\nResponse limit: {} bytes\nExact JSON body:\n{}",
        args.url,
        bytes.len(),
        args.max_bytes
            .unwrap_or(DEFAULT_MAX_BYTES)
            .clamp(1, HARD_MAX_BYTES),
        serde_json::to_string_pretty(&args.body).context("cannot render JSON request body")?
    ))
}

pub async fn http_post(config: &Config, args: &HttpPostArgs) -> Result<String> {
    let url = validate_public_url(&args.url).await?;
    let max_bytes = args
        .max_bytes
        .unwrap_or(DEFAULT_MAX_BYTES)
        .clamp(1, HARD_MAX_BYTES);
    let body = serde_json::to_vec(&args.body).context("cannot encode JSON request body")?;
    if body.len() > POST_MAX_BODY_BYTES {
        bail!("JSON request body exceeds the hard byte limit")
    }
    let client = build_tool_http_client(config)?;
    let mut response = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .context("bounded HTTP POST failed")?;
    if response.status().is_redirection() {
        bail!("HTTP redirects are disabled for bounded tools")
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        bail!("HTTP response exceeds the configured byte limit")
    }
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut response_body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("cannot read HTTP POST response")?
    {
        if response_body.len().saturating_add(chunk.len()) > max_bytes {
            bail!("HTTP response exceeds the configured byte limit")
        }
        response_body.extend_from_slice(&chunk);
    }
    serde_json::to_string_pretty(&json!({"status":status,"content_type":content_type,"bytes":response_body.len(),"body":String::from_utf8_lossy(&response_body)})).context("cannot encode HTTP POST result")
}

pub struct PreparedDownload {
    pub url: String,
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

impl PreparedDownload {
    pub fn summary(&self) -> String {
        format!(
            "Download {} bytes\nfrom: {}\nto: {}",
            self.bytes.len(),
            self.url,
            self.path.display()
        )
    }

    pub fn apply(self) -> Result<()> {
        let parent = self
            .path
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if !parent.is_dir() {
            bail!(
                "download target directory does not exist: {}",
                parent.display()
            )
        }
        let name = self
            .path
            .file_name()
            .context("download target has no file name")?
            .to_string_lossy();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before Unix epoch")?
            .as_nanos();
        let temporary = parent.join(format!(".{name}.nl2sh-{nonce}.tmp"));
        let result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .with_context(|| format!("cannot create {}", temporary.display()))?;
            file.write_all(&self.bytes)
                .with_context(|| format!("cannot write {}", temporary.display()))?;
            file.sync_all()
                .with_context(|| format!("cannot sync {}", temporary.display()))?;
            fs::rename(&temporary, &self.path).with_context(|| {
                format!(
                    "cannot atomically replace {} with downloaded data",
                    self.path.display()
                )
            })?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

pub async fn http_request(config: &Config, args: &HttpRequestArgs) -> Result<String> {
    let method = args.method.as_deref().unwrap_or("GET").to_ascii_uppercase();
    if !matches!(method.as_str(), "GET" | "HEAD") {
        bail!("http_request only supports GET and HEAD")
    }
    let fetched = fetch(config, &args.url, &method, args.max_bytes).await?;
    serde_json::to_string_pretty(&json!({
        "status": fetched.status,
        "content_type": fetched.content_type,
        "bytes": fetched.body.len(),
        "body": String::from_utf8_lossy(&fetched.body),
    }))
    .context("cannot encode HTTP tool result")
}

pub async fn prepare_download(config: &Config, args: &DownloadUrlArgs) -> Result<PreparedDownload> {
    let fetched = fetch(config, &args.url, "GET", args.max_bytes).await?;
    if !(200..300).contains(&fetched.status) {
        bail!("download returned HTTP {}", fetched.status)
    }
    Ok(PreparedDownload {
        url: args.url.clone(),
        path: PathBuf::from(&args.path),
        bytes: fetched.body,
    })
}

struct Fetched {
    status: u16,
    content_type: Option<String>,
    body: Vec<u8>,
}

async fn fetch(
    config: &Config,
    raw_url: &str,
    method: &str,
    requested_max: Option<usize>,
) -> Result<Fetched> {
    let url = validate_public_url(raw_url).await?;
    let max_bytes = requested_max
        .unwrap_or(DEFAULT_MAX_BYTES)
        .clamp(1, HARD_MAX_BYTES);
    let client = build_tool_http_client(config)?;
    let request = if method == "HEAD" {
        client.head(url)
    } else {
        client.get(url)
    };
    let mut response = request
        .send()
        .await
        .context("bounded HTTP request failed")?;
    if response.status().is_redirection() {
        bail!("HTTP redirects are disabled for bounded tools")
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        bail!("HTTP response exceeds the configured byte limit")
    }
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("cannot read HTTP response")?
    {
        if body.len().saturating_add(chunk.len()) > max_bytes {
            bail!("HTTP response exceeds the configured byte limit")
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Fetched {
        status,
        content_type,
        body,
    })
}

pub(crate) async fn validate_public_url(raw_url: &str) -> Result<Url> {
    let url = Url::parse(raw_url).context("invalid HTTP URL")?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("only http and https URLs are allowed")
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("credentials in HTTP URLs are not allowed")
    }
    let host = url.host().context("HTTP URL has no host")?;
    match host {
        Host::Ipv4(address) => reject_private_ip(IpAddr::V4(address))?,
        Host::Ipv6(address) => reject_private_ip(IpAddr::V6(address))?,
        Host::Domain(domain) => {
            let normalized = domain.trim_end_matches('.').to_ascii_lowercase();
            if normalized == "localhost"
                || normalized.ends_with(".localhost")
                || normalized.ends_with(".local")
            {
                bail!("local HTTP hosts are not allowed")
            }
            let port = url
                .port_or_known_default()
                .context("URL has no known port")?;
            let addresses = tokio::net::lookup_host((domain, port))
                .await
                .context("cannot resolve HTTP host")?
                .collect::<Vec<_>>();
            if addresses.is_empty() {
                bail!("HTTP host resolved to no addresses")
            }
            for address in addresses {
                reject_private_ip(address.ip())?;
            }
        }
    }
    Ok(url)
}

fn reject_private_ip(address: IpAddr) -> Result<()> {
    let forbidden = match address {
        IpAddr::V4(value) => {
            value.is_private()
                || value.is_loopback()
                || value.is_link_local()
                || value.is_broadcast()
                || value.is_documentation()
                || value.is_unspecified()
                || value.is_multicast()
        }
        IpAddr::V6(value) => {
            if let Some(mapped) = value.to_ipv4_mapped() {
                return reject_private_ip(IpAddr::V4(mapped));
            }
            value.is_loopback()
                || value.is_unspecified()
                || value.is_multicast()
                || (value.segments()[0] & 0xfe00) == 0xfc00
                || (value.segments()[0] & 0xffc0) == 0xfe80
        }
    };
    if forbidden {
        bail!("private, local, multicast, or documentation HTTP targets are not allowed")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{reject_private_ip, validate_public_url};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[tokio::test]
    async fn rejects_local_and_credentialed_urls() {
        assert!(validate_public_url("http://127.0.0.1/test").await.is_err());
        assert!(validate_public_url("https://user:pass@example.com/")
            .await
            .is_err());
        assert!(validate_public_url("file:///tmp/test").await.is_err());
    }

    #[test]
    fn rejects_private_ipv4_ranges() {
        assert!(reject_private_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))).is_err());
        assert!(reject_private_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))).is_ok());
        assert!(reject_private_ip(IpAddr::V6(Ipv6Addr::from_bits(0xffff_7f00_0001))).is_err());
    }
}
