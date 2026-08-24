//! GitHub Release discovery and checksum-verified executable replacement.

use crate::config::Config;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{fs, io::Write, path::PathBuf};

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/Ernest-su/nl2sh/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq)]
/// One installable release for the current Android ABI.
pub struct UpdateRelease {
    /// Version without the leading `v`.
    pub version: String,
    /// Direct binary download URL.
    pub binary_url: String,
    /// Direct SHA-256 file download URL.
    pub checksum_url: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

/// Checks GitHub's latest non-draft release and returns a newer compatible build.
pub async fn check(config: &Config) -> Result<Option<UpdateRelease>> {
    let abi = android_abi()?;
    let release: GithubRelease = crate::network::build_http_client(config)?
        .get(LATEST_RELEASE_URL)
        .header("User-Agent", concat!("nl2sh/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .context("update check failed")?
        .error_for_status()
        .context("update server returned an error")?
        .json()
        .await
        .context("invalid update metadata")?;
    let version = release.tag_name.trim_start_matches('v').to_owned();
    if !is_newer(&version, env!("CARGO_PKG_VERSION"))? {
        return Ok(None);
    }
    let asset_name = format!("nl2sh-android-{abi}");
    let binary_url = asset_url(&release.assets, &asset_name)?;
    let checksum_url = asset_url(&release.assets, &format!("{asset_name}.sha256"))?;
    Ok(Some(UpdateRelease {
        version,
        binary_url,
        checksum_url,
    }))
}

/// Downloads, verifies, and atomically replaces the running executable.
pub async fn install(config: &Config, release: &UpdateRelease) -> Result<()> {
    let client = crate::network::build_http_client(config)?;
    let binary = download(&client, &release.binary_url).await?;
    let checksum = String::from_utf8(download(&client, &release.checksum_url).await?)
        .context("update checksum is not UTF-8")?;
    let expected = checksum
        .split_whitespace()
        .next()
        .context("update checksum is empty")?;
    let actual = sha256_hex(&binary);
    if !expected.eq_ignore_ascii_case(&actual) {
        bail!("update checksum mismatch")
    }
    replace_current_executable(&binary)
}

async fn download(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    Ok(client
        .get(url)
        .header("User-Agent", concat!("nl2sh/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .context("update download failed")?
        .error_for_status()
        .context("update asset returned an error")?
        .bytes()
        .await
        .context("cannot read update asset")?
        .to_vec())
}

fn asset_url(assets: &[GithubAsset], name: &str) -> Result<String> {
    assets
        .iter()
        .find(|asset| asset.name == name)
        .map(|asset| asset.browser_download_url.clone())
        .with_context(|| format!("release does not contain compatible asset {name}"))
}

fn android_abi() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "aarch64" => Ok("arm64-v8a"),
        "arm" => Ok("armeabi-v7a"),
        other => bail!("self-update is unsupported on architecture {other}"),
    }
}

fn parse_version(value: &str) -> Result<Vec<u64>> {
    value
        .split('-')
        .next()
        .unwrap_or(value)
        .split('.')
        .map(|part| part.parse::<u64>().context("invalid release version"))
        .collect()
}

fn is_newer(candidate: &str, current: &str) -> Result<bool> {
    let mut candidate = parse_version(candidate)?;
    let mut current = parse_version(current)?;
    let width = candidate.len().max(current.len());
    candidate.resize(width, 0);
    current.resize(width, 0);
    Ok(candidate > current)
}

fn replace_current_executable(binary: &[u8]) -> Result<()> {
    let executable = std::env::current_exe().context("cannot locate current executable")?;
    let parent = executable
        .parent()
        .context("executable has no parent directory")?;
    let temporary: PathBuf = parent.join(".nl2sh-update");
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o755);
    }
    let mut file = options
        .open(&temporary)
        .context("cannot create update file")?;
    if let Err(error) = file.write_all(binary).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(error).context("cannot write update file");
    }
    if let Err(error) = fs::rename(&temporary, &executable) {
        let _ = fs::remove_file(&temporary);
        return Err(error).context("cannot replace executable; check directory permissions");
    }
    Ok(())
}

fn sha256_hex(input: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(input))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_numeric_versions() -> Result<()> {
        assert!(is_newer("0.10.0", "0.9.9")?);
        assert!(!is_newer("0.2.0", "0.2.0")?);
        assert!(!is_newer("0.1.9", "0.2.0")?);
        Ok(())
    }
}
