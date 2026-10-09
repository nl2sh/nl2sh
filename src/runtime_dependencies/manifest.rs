//! Authenticated compatibility policy shared by native updates and extension acquisition.

use anyhow::{bail, Context, Result};
use pgp::{
    composed::{Deserializable, SignedPublicKey, StandaloneSignature},
    crypto::hash::HashAlgorithm,
    packet::SignatureType,
    types::KeyDetails,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Cursor, sync::OnceLock};

/// Public trust root also used by the existing Termux APT repository.
pub const TRUSTED_PUBLIC_KEY: &[u8] = include_bytes!("../trust/nl2sh-release.gpg");
/// Pinned primary signing-key fingerprint; network responses cannot replace this trust root.
pub const TRUSTED_FINGERPRINT: &str = "5230D3A7CCBEED4616D39C51FC6AD1BC63F7D4D8";
/// Maximum signed policy size before parsing.
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
/// Maximum detached OpenPGP signature size.
pub const MAX_SIGNATURE_BYTES: usize = 16 * 1024;

/// Exact release asset identity, verified before downloading/executing an extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeArtifact {
    /// Actual component build version, independent of its protocol.
    pub version: String,
    /// Supported runtime protocol version.
    pub protocol: u32,
    /// Immutable tagged asset URL.
    pub url: String,
    /// Detached signature URL for the exact asset bytes.
    pub signature_url: String,
    /// SHA-256 digest of the exact artifact.
    pub sha256: String,
    /// Expected byte length, bounded by the consumer.
    pub size_bytes: u64,
    /// Minimum Android API supported by the artifact.
    pub min_android_api: u32,
    /// Optional APK package name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_name: Option<String>,
    /// APK signing certificate digest; distinct from the artifact digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate_sha256: Option<String>,
    /// Features promised by this release.
    #[serde(default)]
    pub features: Vec<String>,
}

/// Minimum compatible installer, based on the actual lifecycle protocol implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperRequirement {
    /// Installer version required for this release's managed deployment.
    pub min_version: String,
    /// Native service protocol consumed by the installer.
    pub service_protocol: u32,
}

/// Signed release policy. Extension-only build input has an empty binary map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeManifest {
    /// Compatibility document schema.
    pub schema: u32,
    /// Native version this policy belongs to.
    pub nl2sh: String,
    /// Native assets keyed by Android ABI.
    #[serde(default)]
    pub binaries: BTreeMap<String, RuntimeArtifact>,
    /// Recommended Bridge APK, when provided.
    pub android_bridge: Option<RuntimeArtifact>,
    /// Supported DEX helper, when provided.
    pub jadx_helper: Option<RuntimeArtifact>,
    /// Installer requirements.
    pub nl2sh_helper: HelperRequirement,
}

/// Authenticate bytes with the pinned public key before parsing or following URLs.
pub fn verify_signature(data: &[u8], signature: &[u8]) -> Result<()> {
    verify_with_key(
        TRUSTED_PUBLIC_KEY,
        data,
        signature,
        Some(TRUSTED_FINGERPRINT),
    )
}

fn verify_with_key(
    public: &[u8],
    data: &[u8],
    signature: &[u8],
    fingerprint: Option<&str>,
) -> Result<()> {
    if signature.is_empty() || signature.len() > MAX_SIGNATURE_BYTES {
        bail!("invalid release signature size")
    }
    let key =
        SignedPublicKey::from_bytes(Cursor::new(public)).context("invalid release trust root")?;
    key.verify()
        .context("invalid release public-key certificate")?;
    let actual = key
        .fingerprint()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    if fingerprint.is_some_and(|expected| expected != actual) {
        bail!("release trust-root fingerprint mismatch")
    }
    let mut signatures = StandaloneSignature::from_bytes_many(Cursor::new(signature))
        .context("invalid detached release signature")?;
    let signed = signatures
        .next()
        .context("release signature is missing")??;
    if signatures.next().is_some()
        || signed.signature.typ() != Some(SignatureType::Binary)
        || signed.signature.hash_alg() != Some(HashAlgorithm::Sha256)
    {
        bail!("release signature must be one binary SHA-256 OpenPGP signature")
    }
    signed
        .verify(&key, data)
        .context("release signature verification failed")
}

/// Fetch HTTPS bytes with a streaming limit, including responses without Content-Length.
pub async fn download_bounded(
    client: &reqwest::Client,
    url: &str,
    maximum: usize,
) -> Result<Vec<u8>> {
    download_bounded_with_progress(client, url, maximum, |_, _| {}).await
}

/// Fetch bounded HTTPS bytes and report the actual received byte count.
pub async fn download_bounded_with_progress(
    client: &reqwest::Client,
    url: &str,
    maximum: usize,
    progress: impl Fn(u64, Option<u64>),
) -> Result<Vec<u8>> {
    use futures_util::StreamExt;
    let response = client
        .get(url)
        .header("User-Agent", concat!("nl2sh/", env!("CARGO_PKG_VERSION")))
        .send()
        .await?
        .error_for_status()?;
    if response.url().scheme() != "https"
        || response
            .content_length()
            .is_some_and(|length| length > maximum as u64)
    {
        bail!("release download violates HTTPS or size boundary");
    }
    let total = response.content_length();
    progress(0, total);
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes
            .len()
            .checked_add(chunk.len())
            .is_none_or(|size| size > maximum)
        {
            bail!("release download exceeds size boundary");
        }
        bytes.extend_from_slice(&chunk);
        progress(bytes.len() as u64, total);
    }
    Ok(bytes)
}

/// Decode an authenticated compatibility manifest and validate its protocol/asset boundaries.
pub fn decode_signed(data: &[u8], signature: &[u8]) -> Result<RuntimeManifest> {
    if data.is_empty() || data.len() > MAX_MANIFEST_BYTES {
        bail!("runtime manifest exceeds size limit")
    }
    verify_signature(data, signature)?;
    let manifest: RuntimeManifest =
        serde_json::from_slice(data).context("invalid runtime manifest")?;
    manifest.validate()?;
    Ok(manifest)
}

impl RuntimeManifest {
    /// Reject unsupported protocols, misplaced assets, invalid digests, and unbounded metadata.
    pub fn validate(&self) -> Result<()> {
        if self.schema != 1
            || !version_token(&self.nl2sh)
            || !version_token(&self.nl2sh_helper.min_version)
            || self.nl2sh_helper.service_protocol != 1
            || self.binaries.len() > 3
        {
            bail!("unsupported runtime manifest schema, version, or lifecycle protocol")
        }
        for (abi, artifact) in &self.binaries {
            if !matches!(abi.as_str(), "arm64-v8a" | "armeabi-v7a" | "x86_64")
                || artifact.version != self.nl2sh
            {
                bail!("runtime binary does not match release or supported ABI")
            }
            self.check_asset(artifact, &format!("nl2sh-android-{abi}"), 1, 32_000_000)?;
        }
        if let Some(bridge) = &self.android_bridge {
            self.check_asset(bridge, "nl2sh-android-bridge.apk", 2, 64 * 1024 * 1024)?;
            if bridge.package_name.as_deref() != Some("com.nl2sh.bridge")
                || !bridge.certificate_sha256.as_deref().is_some_and(valid_sha)
            {
                bail!("Bridge package or signing certificate is missing")
            }
        }
        if let Some(jadx) = &self.jadx_helper {
            self.check_asset(jadx, "jadx-helper.jar", 1, 64 * 1024 * 1024)?;
            if !jadx
                .features
                .iter()
                .any(|feature| feature == "single_class")
            {
                bail!("JADX single-class capability is missing")
            }
        }
        Ok(())
    }

    fn check_asset(
        &self,
        artifact: &RuntimeArtifact,
        name: &str,
        protocol: u32,
        maximum: u64,
    ) -> Result<()> {
        let expected = format!(
            "https://github.com/nl2sh/nl2sh/releases/download/v{}/{name}",
            self.nl2sh
        );
        if !version_token(&artifact.version)
            || artifact.protocol != protocol
            || !valid_sha(&artifact.sha256)
            || !(1..=maximum).contains(&artifact.size_bytes)
            || artifact.min_android_api != 26
            || artifact.url != expected
            || artifact.signature_url != format!("{expected}.sig")
            || artifact.features.len() > 64
            || artifact.features.iter().any(|feature| feature.len() > 128)
        {
            bail!("invalid runtime asset identity, protocol, URL, or limits")
        }
        Ok(())
    }
}

fn version_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
}
fn valid_sha(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

static EMBEDDED: OnceLock<std::result::Result<Option<RuntimeManifest>, String>> = OnceLock::new();

/// Immutable signed build policy; unsigned source builds have no default extension download.
pub fn embedded() -> Result<Option<RuntimeManifest>> {
    let result = EMBEDDED.get_or_init(|| {
        let data = include_bytes!(concat!(env!("OUT_DIR"), "/nl2sh-runtime-extensions.json"));
        let signature = include_bytes!(concat!(
            env!("OUT_DIR"),
            "/nl2sh-runtime-extensions.json.sig"
        ));
        if data.is_empty() {
            return Ok(None);
        }
        decode_signed(data, signature)
            .and_then(|manifest| {
                if manifest.nl2sh != env!("CARGO_PKG_VERSION") {
                    bail!("embedded runtime manifest belongs to a different native version")
                }
                Ok(Some(manifest))
            })
            .map_err(|error| format!("{error:#}"))
    });
    result.clone().map_err(anyhow::Error::msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    const PUBLIC: &[u8] = include_bytes!("../../tests/fixtures/signatures/public.gpg");
    const MESSAGE: &[u8] = include_bytes!("../../tests/fixtures/signatures/message.txt");
    const SIGNATURE: &[u8] = include_bytes!("../../tests/fixtures/signatures/message.sig");

    #[tokio::test]
    async fn download_progress_cannot_bypass_https_boundary() -> Result<()> {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(b"untrusted bytes"))
            .mount(&server)
            .await;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let called = std::sync::atomic::AtomicBool::new(false);
        let result = download_bounded_with_progress(&client, &server.uri(), 100, |_, _| {
            called.store(true, std::sync::atomic::Ordering::Relaxed);
        })
        .await;
        assert!(result.is_err());
        assert!(!called.load(std::sync::atomic::Ordering::Relaxed));
        assert!(download_bounded(&client, &server.uri(), 100).await.is_err());
        Ok(())
    }

    #[test]
    fn detached_signature_authenticates_exact_bytes() {
        assert!(verify_with_key(PUBLIC, MESSAGE, SIGNATURE, None).is_ok());
        assert!(verify_with_key(PUBLIC, b"modified", SIGNATURE, None).is_err());
        assert!(verify_signature(MESSAGE, SIGNATURE).is_err());
        assert!(verify_with_key(PUBLIC, MESSAGE, SIGNATURE, Some(TRUSTED_FINGERPRINT)).is_err());
        let duplicate = [SIGNATURE, SIGNATURE].concat();
        assert!(verify_with_key(PUBLIC, MESSAGE, &duplicate, None).is_err());
        assert!(verify_signature(MESSAGE, &[]).is_err());
        assert!(decode_signed(&vec![0; MAX_MANIFEST_BYTES + 1], SIGNATURE).is_err());
    }

    #[test]
    fn published_trust_root_matches_pin() {
        let key = SignedPublicKey::from_bytes(Cursor::new(TRUSTED_PUBLIC_KEY));
        let key = key.unwrap_or_else(|error| panic!("test trust root: {error}"));
        assert!(key.verify().is_ok());
        let fingerprint = key
            .fingerprint()
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>();
        assert_eq!(fingerprint, TRUSTED_FINGERPRINT);
    }

    #[test]
    fn manifest_rejects_protocol_and_asset_identity_drift() {
        let mut manifest = RuntimeManifest {
            schema: 1,
            nl2sh: "1.1.0".into(),
            binaries: BTreeMap::new(),
            android_bridge: None,
            jadx_helper: None,
            nl2sh_helper: HelperRequirement {
                min_version: "0.2.0".into(),
                service_protocol: 1,
            },
        };
        assert!(manifest.validate().is_ok());
        let url = "https://github.com/nl2sh/nl2sh/releases/download/v1.1.0/jadx-helper.jar";
        manifest.jadx_helper = Some(RuntimeArtifact {
            version: "0.2.0".into(),
            protocol: 1,
            url: url.into(),
            signature_url: format!("{url}.sig"),
            sha256: "a".repeat(64),
            size_bytes: 100,
            min_android_api: 26,
            package_name: None,
            certificate_sha256: None,
            features: vec!["single_class".into()],
        });
        assert!(manifest.validate().is_ok());
        if let Some(jadx) = &mut manifest.jadx_helper {
            jadx.url = "https://example.com/helper.jar".into();
        }
        assert!(manifest.validate().is_err());
        manifest.jadx_helper = None;
        manifest.schema = 2;
        assert!(manifest.validate().is_err());
    }
}
