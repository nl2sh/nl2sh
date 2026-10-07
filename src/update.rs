//! GitHub Release discovery and checksum-verified executable replacement.

use crate::config::Config;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// Installation owner and the supported update entry point; contains no credentials.
#[derive(Debug, Serialize)]
pub struct UpdateOwnership {
    /// `standalone`, `nl2sh-helper`, `termux-apt`, or `unknown`.
    pub owner: &'static str,
    /// Whether this installation can replace its own executable.
    pub self_update_allowed: bool,
    /// Guidance for the correct update entry point.
    pub message: &'static str,
}

#[derive(Deserialize)]
struct OwnerMarker {
    protocol: u32,
    owner: String,
    version: String,
    sha256: String,
    source: String,
}

/// Inspect a bounded adjacent Helper marker and verify the installed file's checksum.
pub async fn ownership() -> UpdateOwnership {
    if !self_update_enabled() {
        return UpdateOwnership {
            owner: "termux-apt",
            self_update_allowed: false,
            message: package_update_message(),
        };
    }
    let result = tokio::task::spawn_blocking(|| {
        let executable = std::env::current_exe().context("cannot locate current executable")?;
        // A running older inode can remain after an atomic Helper upgrade.
        let path = PathBuf::from(executable.to_string_lossy().trim_end_matches(" (deleted)"));
        helper_owned(&path)
    })
    .await;
    match result {
        Ok(Ok(false)) => UpdateOwnership { owner: "standalone", self_update_allowed: true, message: "Use nl2sh update" },
        Ok(Ok(true)) => UpdateOwnership { owner: "nl2sh-helper", self_update_allowed: false,
            message: "此安装由 nl2sh助手管理，请在助手检查更新 / Managed by nl2sh Helper; check updates in Helper" },
        _ => UpdateOwnership { owner: "unknown", self_update_allowed: false,
            message: "安装归属记录无法验证，请检查 owner.json / Cannot verify installation ownership; inspect owner.json" },
    }
}

fn helper_owned(executable: &Path) -> Result<bool> {
    use std::os::unix::fs::OpenOptionsExt;
    let marker_path = PathBuf::from(format!("{}.owner.json", executable.display()));
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(marker_path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("cannot read installation owner"),
    };
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        bail!("installation owner marker exceeds limit")
    }
    let marker: OwnerMarker = serde_json::from_slice(&bytes)?;
    if marker.protocol != 1
        || marker.owner != "nl2sh-helper"
        || marker.version.is_empty()
        || marker.version.len() > 64
        || marker.source != "https://github.com/nl2sh/nl2sh"
        || marker.sha256.len() != 64
        || !marker.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        bail!("invalid installation ownership")
    }
    let mut binary = Vec::new();
    fs::File::open(executable)?
        .take(32_000_001)
        .read_to_end(&mut binary)?;
    if binary.len() > 32_000_000 || !sha256_hex(&binary).eq_ignore_ascii_case(&marker.sha256) {
        bail!("installation owner checksum mismatch")
    }
    Ok(true)
}

async fn require_self_update_owner() -> Result<()> {
    let owner = ownership().await;
    if !owner.self_update_allowed {
        bail!(owner.message)
    }
    Ok(())
}

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/nl2sh/nl2sh/releases/latest";

/// Whether this build may replace its own executable.
pub const fn self_update_enabled() -> bool {
    cfg!(feature = "self-update")
}

/// User-facing guidance for package-manager builds.
pub const fn package_update_message() -> &'static str {
    "此版本由 Termux APT 管理，请运行 pkg upgrade nl2sh / This build is managed by Termux APT; run pkg upgrade nl2sh"
}

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
    if !self_update_enabled() {
        bail!(package_update_message())
    }
    require_self_update_owner().await?;
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
    if !self_update_enabled() {
        bail!(package_update_message())
    }
    require_self_update_owner().await?;
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
    if !cfg!(target_os = "android") {
        bail!("self-update is only supported on Android")
    }
    android_abi_for_arch(std::env::consts::ARCH)
}

fn android_abi_for_arch(arch: &str) -> Result<&'static str> {
    match arch {
        "aarch64" => Ok("arm64-v8a"),
        "arm" => Ok("armeabi-v7a"),
        "x86_64" => Ok("x86_64"),
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
    fn helper_owner_is_verified_and_cannot_silently_fall_back_on_corruption() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("nl2sh");
        fs::write(&executable, b"verified program")?;
        assert!(!helper_owned(&executable)?);
        let marker = serde_json::json!({ "protocol": 1, "owner": "nl2sh-helper", "version": "1.0.6",
            "sha256": sha256_hex(b"verified program"), "source": "https://github.com/nl2sh/nl2sh" });
        let marker_path = directory.path().join("nl2sh.owner.json");
        fs::write(&marker_path, serde_json::to_vec(&marker)?)?;
        assert!(helper_owned(&executable)?);
        fs::write(&executable, b"different program")?;
        assert!(helper_owned(&executable).is_err());
        fs::write(&marker_path, b"invalid")?;
        assert!(helper_owned(&executable).is_err());
        Ok(())
    }

    #[test]
    fn selects_android_update_assets() -> Result<()> {
        assert_eq!(android_abi_for_arch("aarch64")?, "arm64-v8a");
        assert_eq!(android_abi_for_arch("arm")?, "armeabi-v7a");
        assert_eq!(android_abi_for_arch("x86_64")?, "x86_64");
        assert!(android_abi_for_arch("x86").is_err());
        Ok(())
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn rejects_android_update_on_host() {
        assert!(android_abi().is_err());
    }

    #[test]
    fn compares_numeric_versions() -> Result<()> {
        assert!(is_newer("0.10.0", "0.9.9")?);
        assert!(!is_newer("0.2.0", "0.2.0")?);
        assert!(!is_newer("0.1.9", "0.2.0")?);
        Ok(())
    }
}
