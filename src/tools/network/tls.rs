use anyhow::{bail, Context, Result};
use rustls::{pki_types::ServerName, ClientConfig, RootCertStore};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tokio::{net::TcpStream, time::timeout};
use tokio_rustls::TlsConnector;
use x509_parser::parse_x509_certificate;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TlsInspectArgs {
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
}

pub async fn inspect_tls(args: &TlsInspectArgs, timeout_secs: u64) -> Result<String> {
    validate_host(&args.host)?;
    let port = args.port.unwrap_or(443);
    let validation_url = format!("https://{}:{port}/", bracket_ipv6(&args.host));
    crate::tools::network::domain::validate_public_url(&validation_url).await?;

    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let server_name =
        ServerName::try_from(args.host.clone()).context("TLS host is not a valid server name")?;
    let operation = async {
        let tcp = TcpStream::connect((args.host.as_str(), port))
            .await
            .context("cannot connect to TLS endpoint")?;
        let connector = TlsConnector::from(Arc::new(config));
        connector
            .connect(server_name, tcp)
            .await
            .context("TLS certificate, chain, hostname, or validity verification failed")
    };
    let stream = timeout(Duration::from_secs(timeout_secs.max(1)), operation)
        .await
        .context("TLS diagnostic timed out")??;
    let certificates = stream
        .get_ref()
        .1
        .peer_certificates()
        .context("TLS peer returned no certificates")?;
    let mut details = Vec::new();
    for (index, certificate) in certificates.iter().enumerate() {
        let (_, parsed) = parse_x509_certificate(certificate.as_ref())
            .map_err(|error| anyhow::anyhow!("cannot parse peer certificate {index}: {error}"))?;
        details.push(json!({
            "index": index,
            "subject": parsed.subject().to_string(),
            "issuer": parsed.issuer().to_string(),
            "not_before": parsed.validity().not_before.to_string(),
            "not_after": parsed.validity().not_after.to_string(),
            "sha256": hex_sha256(certificate.as_ref()),
            "der_bytes": certificate.as_ref().len(),
        }));
    }
    serde_json::to_string_pretty(&json!({
        "status": "verified",
        "host": args.host,
        "port": port,
        "hostname_verified": true,
        "chain_verified": true,
        "certificates": details,
    }))
    .context("cannot encode TLS diagnostic result")
}

fn validate_host(host: &str) -> Result<()> {
    if host.is_empty()
        || host.contains(['/', '\\', '@', '\n', '\r', '\0'])
        || host.chars().any(char::is_whitespace)
    {
        bail!("invalid TLS host")
    }
    Ok(())
}

fn bracket_ipv6(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|value| format!("{value:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{bracket_ipv6, validate_host};

    #[test]
    fn validates_tls_hosts_without_accepting_url_syntax() {
        assert!(validate_host("example.com").is_ok());
        assert!(validate_host("example.com/path").is_err());
        assert!(validate_host("user@example.com").is_err());
        assert_eq!(
            bracket_ipv6("2001:4860:4860::8888"),
            "[2001:4860:4860::8888]"
        );
    }
}
