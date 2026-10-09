//! Android-only, SHA-256-pinned DEX helper acquisition and ART invocation.

use crate::{
    config::{self, Config},
    network,
};
use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command};

const MAX_HELPER_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SOURCE_BYTES: u64 = 64 * 1024;
const ENTRYPOINT: &str = "com.nl2sh.jadx.Main";
const ENTRYPOINT_DESCRIPTOR: &[u8] = b"Lcom/nl2sh/jadx/Main;";
static DOWNLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn cache_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("NL2SH_JADX_CACHE_DIR").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path).join(cache_identity()?));
    }
    let config_path = config::default_config_path()?;
    Ok(config::state_dir(&config_path)?
        .join("runtime/jadx-helper")
        .join(cache_identity()?))
}

fn cache_identity() -> Result<String> {
    if std::env::var_os("NL2SH_JADX_ANDROID_HELPER_PATH").is_some() {
        return Ok("offline".into());
    }
    let source = download_source()?;
    Ok(source.sha)
}

/// How an acquisition source is authenticated before any download is trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HelperTrust {
    /// Operator-supplied and not covered by any release policy: a URL with digest, or a local file.
    Explicit,
    /// Verified against this build's OpenPGP-signed runtime policy.
    SignedPolicy,
    /// Pinned release checksum compiled into this source build.
    ReleasePin,
}

/// Release pin for source builds that embed no signed runtime policy.
///
/// Precedence is explicit environment, then the embedded signed policy, then this pin. An
/// embedded policy that fails to decode is an error rather than a reason to fall back, so a
/// damaged signature can never silently downgrade to the unsigned pin. The digest is the
/// authority here exactly as it is for the pinned Tailcat release; `version` is reported for
/// diagnostics only and is not independently verified.
const RELEASE_PIN_VERSION: &str = "0.2.0";
const RELEASE_PIN_URL: &str =
    "https://github.com/nl2sh/jadx-helper/releases/download/v0.2.0/jadx-helper.jar";
const RELEASE_PIN_SHA256: &str = "083b8d9d6223c009350688ed5f8fc3ae1ad1a39ce23e930498aa8035a4b89899";

struct HelperSource {
    url: String,
    sha: String,
    signed: Option<super::manifest::RuntimeArtifact>,
    trust: HelperTrust,
}

fn parse_sha(sha: String) -> Result<String> {
    if sha.len() != 64 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("Android JADX helper SHA-256 must be 64 hexadecimal characters")
    }
    Ok(sha.to_ascii_lowercase())
}

/// Runtime contract returned by the installed DEX helper without opening an APK.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct JadxInfo {
    /// Supported helper invocation protocol.
    pub protocol: u32,
    /// Helper build version.
    pub helper_version: String,
    /// Pinned upstream JADX core version.
    pub jadx_core: String,
    /// Supported bounded analysis features.
    pub features: Vec<String>,
}

/// Probe only an already installed, validated helper; never download during discovery.
pub async fn installed_info(
    executor: &dyn crate::shell::CommandExecutor,
) -> Result<Option<JadxInfo>> {
    if !cfg!(target_os = "android") {
        return Ok(None);
    }
    let explicit =
        std::env::var_os("NL2SH_JADX_ANDROID_HELPER_PATH").filter(|value| !value.is_empty());
    let (path, sha) = if let Some(path) = explicit {
        (
            PathBuf::from(path),
            std::env::var("NL2SH_JADX_ANDROID_HELPER_SHA256")
                .ok()
                .map(parse_sha)
                .transpose()?,
        )
    } else {
        let source = match download_source() {
            Ok(source) => source,
            Err(_) => return Ok(None),
        };
        (cache_dir()?.join("jadx-helper.jar"), Some(source.sha))
    };
    let verified = tokio::task::spawn_blocking(move || -> Result<Option<PathBuf>> {
        if !path.is_file() {
            return Ok(None);
        }
        validate_dex_jar(&path)?;
        if let Some(sha) = sha {
            if digest_file(&path)? != sha {
                bail!("Android JADX helper SHA-256 mismatch");
            }
        }
        Ok(Some(fs::canonicalize(path)?))
    })
    .await
    .context("helper discovery worker failed")??;
    let Some(path) = verified else {
        return Ok(None);
    };
    let quoted = format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"));
    let command = format!("CLASSPATH={quoted} /system/bin/app_process / {ENTRYPOINT} --info");
    let result = executor.execute_probe(&command).await?;
    if result.exit_code != Some(0)
        || result.timed_out
        || result.interrupted
        || result.stdout.len() > 4096
    {
        bail!("Android JADX helper information unavailable");
    }
    let info: JadxInfo =
        serde_json::from_str(&result.stdout).context("invalid Android JADX helper information")?;
    validate_info(&info, expected_version()?.as_deref())?;
    Ok(Some(info))
}

fn expected_version() -> Result<Option<String>> {
    if std::env::var_os("NL2SH_JADX_ANDROID_HELPER_PATH").is_some() {
        return Ok(None);
    }
    Ok(download_source()?.signed.map(|artifact| artifact.version))
}

fn validate_info(info: &JadxInfo, expected: Option<&str>) -> Result<()> {
    if info.protocol != 1
        || info.helper_version.is_empty()
        || info.helper_version.len() > 128
        || info.jadx_core.is_empty()
        || info.jadx_core.len() > 128
        || info.features.len() > 32
        || info.features.iter().any(|feature| feature.len() > 128)
        || !info
            .features
            .iter()
            .any(|feature| feature == "single_class")
    {
        bail!("unsupported Android JADX helper protocol or features");
    }
    if expected.is_some_and(|version| version != info.helper_version) {
        bail!("Android JADX helper version does not match signed policy");
    }
    Ok(())
}

/// Cheap advisory state for the Agent prompt.
///
/// This only reports whether a helper file exists at the expected location; full digest and DEX
/// structure validation still runs before any helper is executed.
pub fn installed_hint() -> &'static str {
    let offline = offline_path().is_some_and(|path| path.is_file());
    let cached = cache_dir()
        .ok()
        .is_some_and(|dir| dir.join("jadx-helper.jar").is_file());
    if offline || cached {
        "installed"
    } else if download_source().is_ok() {
        "absent"
    } else {
        "unprovisionable"
    }
}

/// Resolve and validate the acquisition source without touching the network.
fn download_source() -> Result<HelperSource> {
    let source = resolve_source()?;
    validate_source_url(&source.url)?;
    Ok(source)
}

/// Enforce the transport requirements every acquisition source must meet.
fn validate_source_url(url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url).context("invalid Android JADX helper URL")?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        bail!("Android JADX helper URL must be HTTPS without credentials or fragment")
    }
    Ok(())
}

/// Resolve the acquisition source without touching the network.
fn resolve_source() -> Result<HelperSource> {
    let explicit = match std::env::var("NL2SH_JADX_ANDROID_HELPER_URL") {
        Ok(url) => {
            let sha = std::env::var("NL2SH_JADX_ANDROID_HELPER_SHA256")
                .context("NL2SH_JADX_ANDROID_HELPER_SHA256 is required with a custom helper URL")?;
            Some((url, sha))
        }
        Err(_) => None,
    };
    choose_source(
        explicit,
        super::manifest::embedded()?.and_then(|it| it.jadx_helper),
    )
}

/// Pick the authoritative acquisition source.
///
/// Precedence is an operator-supplied URL and digest, then the embedded signed policy, then the
/// compiled-in release pin. The caller passes the embedded policy as `Ok(None)` only when this
/// build genuinely carries no policy; a policy that fails to decode is returned as an error by
/// the caller so a damaged signature can never silently downgrade to the unsigned pin.
fn choose_source(
    explicit: Option<(String, String)>,
    policy: Option<super::manifest::RuntimeArtifact>,
) -> Result<HelperSource> {
    if let Some((url, sha)) = explicit {
        return Ok(HelperSource {
            url,
            sha: parse_sha(sha)?,
            signed: None,
            trust: HelperTrust::Explicit,
        });
    }
    let Some(artifact) = policy else {
        return Ok(HelperSource {
            url: RELEASE_PIN_URL.to_string(),
            sha: RELEASE_PIN_SHA256.to_string(),
            signed: None,
            trust: HelperTrust::ReleasePin,
        });
    };
    Ok(HelperSource {
        url: artifact.url.clone(),
        sha: artifact.sha256.to_ascii_lowercase(),
        signed: Some(artifact),
        trust: HelperTrust::SignedPolicy,
    })
}

/// Whether an offline helper or a downloadable source can supply the helper.
///
/// Discovery never downloads or executes anything.
pub fn provisionable() -> bool {
    if let Some(path) =
        std::env::var_os("NL2SH_JADX_ANDROID_HELPER_PATH").filter(|value| !value.is_empty())
    {
        return Path::new(&path).is_file();
    }
    download_source().is_ok()
}

fn make_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("cannot create helper cache {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .context("cannot protect helper cache")?;
    }
    Ok(())
}

fn digest_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).context("cannot open Android JADX helper")?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .context("cannot hash Android JADX helper")?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn validate_dex_jar(path: &Path) -> Result<()> {
    let file = fs::File::open(path).context("cannot open Android JADX helper")?;
    if file.metadata()?.len() > MAX_HELPER_BYTES {
        bail!("Android JADX helper exceeds 64 MiB limit")
    }
    let mut zip = zip::ZipArchive::new(file).context("Android JADX helper is not a ZIP/JAR")?;
    let mut dex = zip.by_name("classes.dex").context(
        "Android JADX helper has no classes.dex; a JVM class JAR cannot run with app_process",
    )?;
    if dex.size() > MAX_HELPER_BYTES {
        bail!("Android helper classes.dex exceeds 64 MiB limit")
    }
    let mut magic = [0u8; 8];
    dex.read_exact(&mut magic)
        .context("Android helper has a truncated classes.dex")?;
    if &magic[..4] != b"dex\n" || magic[7] != 0 {
        bail!("Android helper contains an invalid classes.dex")
    }
    let mut chunk = [0u8; 64 * 1024];
    let mut tail = Vec::with_capacity(ENTRYPOINT_DESCRIPTOR.len());
    let mut found = false;
    loop {
        let count = dex
            .read(&mut chunk)
            .context("cannot inspect Android helper DEX")?;
        if count == 0 {
            break;
        }
        tail.extend_from_slice(&chunk[..count]);
        if tail
            .windows(ENTRYPOINT_DESCRIPTOR.len())
            .any(|part| part == ENTRYPOINT_DESCRIPTOR)
        {
            found = true;
            break;
        }
        let keep = ENTRYPOINT_DESCRIPTOR.len().saturating_sub(1);
        if tail.len() > keep {
            tail.drain(..tail.len() - keep);
        }
    }
    if !found {
        bail!("Android JADX helper lacks the com.nl2sh.jadx.Main entrypoint")
    }
    Ok(())
}

fn verified(path: &Path, expected: &str) -> bool {
    path.is_file()
        && digest_file(path).is_ok_and(|digest| digest == expected)
        && validate_dex_jar(path).is_ok()
}

fn offline_path() -> Option<PathBuf> {
    std::env::var_os("NL2SH_JADX_ANDROID_HELPER_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Validate an operator-supplied offline helper without downloading anything.
async fn validated_offline(path: PathBuf) -> Result<PathBuf> {
    let checked = path.clone();
    tokio::task::spawn_blocking(move || validate_dex_jar(&checked))
        .await
        .context("helper validation worker failed")??;
    if std::env::var_os("NL2SH_JADX_ANDROID_HELPER_SHA256").is_some() {
        let expected = parse_sha(std::env::var("NL2SH_JADX_ANDROID_HELPER_SHA256")?)?;
        let checked = path.clone();
        if !tokio::task::spawn_blocking(move || {
            digest_file(&checked).is_ok_and(|digest| digest == expected)
        })
        .await
        .context("helper hash worker failed")?
        {
            bail!("Android JADX helper SHA-256 mismatch")
        }
    }
    fs::canonicalize(path).context("cannot resolve Android helper path")
}

/// Resolve an installed helper without downloading, executing, or mutating anything.
pub async fn installed_helper() -> Result<PathBuf> {
    if let Some(path) = offline_path() {
        return validated_offline(path).await;
    }
    let expected = download_source()?.sha;
    let dest = cache_dir()?.join("jadx-helper.jar");
    let cached = dest.clone();
    let sha = expected.clone();
    let ready = tokio::task::spawn_blocking(move || verified(&cached, &sha))
        .await
        .context("helper cache worker failed")?;
    if !ready {
        bail!(
            "Android JADX helper is not installed; use jadx_install after approval, then retry decompilation"
        )
    }
    fs::canonicalize(dest).context("cannot resolve cached Android helper")
}

async fn acquire(config: &Config) -> Result<PathBuf> {
    if let Some(path) = offline_path() {
        return validated_offline(path).await;
    }
    let source = download_source()?;
    let url = source.url;
    let sha = source.sha;
    let signed = source.signed;
    let dir = cache_dir()?;
    let dest = dir.join("jadx-helper.jar");
    let _guard = DOWNLOAD_LOCK.lock().await;
    let cached = dest.clone();
    let expected = sha.clone();
    if tokio::task::spawn_blocking(move || verified(&cached, &expected))
        .await
        .context("helper cache worker failed")?
    {
        return fs::canonicalize(dest).context("cannot resolve cached Android helper");
    }
    make_private_dir(&dir)?;
    let staged =
        tempfile::NamedTempFile::new_in(&dir).context("cannot stage Android JADX helper")?;
    let mut output = tokio::fs::File::from_std(
        staged
            .reopen()
            .context("cannot open staged Android helper")?,
    );
    let response = network::build_http_client(config)?
        .get(&url)
        .send()
        .await
        .context("cannot download Android JADX helper")?
        .error_for_status()
        .context("Android JADX helper download failed")?;
    if response.url().scheme() != "https"
        || response
            .content_length()
            .is_some_and(|size| size > MAX_HELPER_BYTES)
    {
        bail!("Android JADX helper download must remain HTTPS and at most 64 MiB")
    }
    let mut bytes = 0u64;
    let mut hash = Sha256::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Android JADX helper download interrupted")?;
        bytes = bytes
            .checked_add(chunk.len() as u64)
            .context("helper download size overflow")?;
        if bytes > MAX_HELPER_BYTES {
            bail!("Android JADX helper exceeds download limit")
        }
        hash.update(&chunk);
        output
            .write_all(&chunk)
            .await
            .context("cannot write staged Android helper")?;
    }
    output
        .sync_all()
        .await
        .context("cannot sync staged Android helper")?;
    drop(output);
    if format!("{:x}", hash.finalize()) != sha {
        bail!("Android JADX helper SHA-256 mismatch")
    }
    if let Some(artifact) = signed {
        if bytes != artifact.size_bytes {
            bail!("Android JADX helper size mismatch")
        }
        let signature = super::manifest::download_bounded(
            &network::build_http_client(config)?,
            &artifact.signature_url,
            super::manifest::MAX_SIGNATURE_BYTES,
        )
        .await?;
        let path = staged.path().to_owned();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let data = fs::read(path)?;
            super::manifest::verify_signature(&data, &signature)
        })
        .await
        .context("helper signature worker failed")??;
    }
    let published = dest.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        validate_dex_jar(staged.path())?;
        staged
            .persist(&published)
            .context("cannot publish Android JADX helper")?;
        Ok(())
    })
    .await
    .context("helper publication worker failed")??;
    fs::canonicalize(dest).context("cannot resolve downloaded Android helper")
}

fn helper_command(
    jar: &Path,
    apk: &Path,
    class_name: &str,
    source: &Path,
    temp_dir: &Path,
) -> Command {
    let mut command = Command::new("/system/bin/app_process");
    let mut java_temp_arg = std::ffi::OsString::from("-Djava.io.tmpdir=");
    java_temp_arg.push(temp_dir);
    command
        .env("CLASSPATH", jar)
        .arg(java_temp_arg)
        .arg("/")
        .arg(ENTRYPOINT)
        .arg(apk)
        .arg(class_name)
        .arg(source);
    command
}

async fn probe_info(jar: &Path, work: &Path) -> Result<JadxInfo> {
    let stdout = tempfile::NamedTempFile::new_in(work)?;
    let mut child = Command::new("/system/bin/app_process")
        .env("CLASSPATH", jar)
        .arg("/")
        .arg(ENTRYPOINT)
        .arg("--info")
        .stdout(std::process::Stdio::from(stdout.reopen()?))
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let status = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(result) => result?,
        Err(_) => {
            child.kill().await?;
            let _ = child.wait().await;
            bail!("Android JADX protocol probe timed out")
        }
    };
    if !status.success() || stdout.as_file().metadata()?.len() > 4096 {
        bail!("Android JADX protocol probe failed or exceeded limit")
    }
    let mut bytes = Vec::new();
    stdout.reopen()?.take(4097).read_to_end(&mut bytes)?;
    serde_json::from_slice(&bytes).context("invalid Android JADX protocol information")
}

/// Report acquisition state without downloading or executing the helper.
///
/// This is the read-only half of the explicit acquisition flow: it names the source that
/// `jadx_install` would use, its authentication, and whether a validated helper is already
/// present.
pub async fn check(executor: &dyn crate::shell::CommandExecutor) -> Result<String> {
    let offline = offline_path();
    let source = if offline.is_some() {
        None
    } else {
        download_source().ok()
    };
    let trust = if offline.is_some() {
        Some(HelperTrust::Explicit)
    } else {
        source.as_ref().map(|item| item.trust)
    };
    let path = if offline.is_some() {
        offline
    } else {
        cache_dir()
            .ok()
            .map(|dir| dir.join("jadx-helper.jar"))
            .filter(|path| path.is_file())
    };
    let installed = match path.clone() {
        Some(candidate) => {
            // Hashing and ZIP inspection read up to the 64 MiB helper ceiling, so keep it off the
            // async runtime just like `installed_info` does.
            tokio::task::spawn_blocking(move || validate_dex_jar(&candidate).is_ok())
                .await
                .context("helper validation worker failed")?
        }
        None => false,
    };
    let info = if installed {
        installed_info(executor).await.ok().flatten()
    } else {
        None
    };
    Ok(serde_json::json!({
        "status": "ok",
        "supported_platform": cfg!(target_os = "android"),
        "provisionable": provisionable(),
        "trust": trust,
        "source_url": source.as_ref().map(|item| item.url.clone()),
        "source_sha256": source.as_ref().map(|item| item.sha.clone()),
        "pinned_version": match trust {
            Some(HelperTrust::ReleasePin) => Some(RELEASE_PIN_VERSION.to_string()),
            Some(HelperTrust::SignedPolicy) => source
                .as_ref()
                .and_then(|item| item.signed.as_ref())
                .map(|artifact| artifact.version.clone()),
            _ => None,
        },
        "installed": installed,
        "installed_path": path.map(|path| path.to_string_lossy().into_owned()),
        "installed_info": info,
    })
    .to_string())
}

/// Human-readable confirmation preview for the explicit acquisition step.
///
/// The preview names the exact bytes that approval would authorize, mirroring the pinned
/// Tailcat install preview so the digest is visible before any request is made.
pub fn install_preview() -> Result<String> {
    if let Some(path) = offline_path() {
        return Ok(format!(
            "Use the offline Android DEX helper at {} after verifying it is the intended file",
            path.to_string_lossy()
        ));
    }
    let source = download_source()?;
    Ok(format!(
        "Download the Android DEX helper from {}, verify SHA-256 {}, and cache it privately for decompilation",
        source.url, source.sha
    ))
}

/// Download, verify, and publish the helper after explicit approval.
///
/// This is the only path that reaches the network. An already valid cache is reused without a
/// request, and the signed policy additionally verifies the detached signature.
pub async fn install(config: &Config) -> Result<String> {
    let trust = if offline_path().is_some() {
        HelperTrust::Explicit
    } else {
        download_source()?.trust
    };
    let path = acquire(config).await?;
    Ok(serde_json::json!({
        "status": "ok",
        "trust": trust,
        "path": path.to_string_lossy(),
        "next_step": "decompile_apk_class",
    })
    .to_string())
}

/// Decompiles one APK class using an installed Android DEX helper and `app_process`.
///
/// This never reaches the network. Acquisition is a separate, explicitly approved step so the
/// dangerous call runs only code the operator has already installed and verified.
pub async fn decompile_class(apk_path: &str, class_name: &str) -> Result<String> {
    if !cfg!(target_os = "android") {
        bail!("{{\"error\":\"jadx_android_only\",\"detail\":\"DEX helper decompilation requires the Android app_process runtime\"}}")
    }
    let jar = installed_helper().await.map_err(|error| {
        anyhow::anyhow!(
            "{}",
            serde_json::json!({
                "error":"jadx_android_helper_unavailable", "detail":error.to_string()
            })
        )
    })?;
    let apk =
        fs::canonicalize(apk_path).with_context(|| format!("cannot resolve APK {apk_path}"))?;
    let work_dir = cache_dir()?;
    make_private_dir(&work_dir)?;
    let work = tempfile::tempdir_in(work_dir).context("cannot create JADX work directory")?;
    let info = probe_info(&jar, work.path()).await?;
    validate_info(&info, expected_version()?.as_deref())?;
    let source = work.path().join("class.java");
    let stderr =
        tempfile::NamedTempFile::new_in(work.path()).context("cannot stage helper diagnostics")?;
    let stderr_output = stderr.reopen().context("cannot open helper diagnostics")?;
    let mut command = helper_command(&jar, &apk, class_name, &source, work.path());
    command
        .env("JADX_CONFIG_DIR", work.path())
        .env("JADX_CACHE_DIR", work.path())
        .env("JADX_TMP_DIR", work.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(stderr_output))
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .context("cannot start Android JADX helper")?;
    let status = match tokio::time::timeout(Duration::from_secs(120), child.wait()).await {
        Ok(result) => result.context("cannot wait for Android JADX helper")?,
        Err(_) => {
            child
                .kill()
                .await
                .context("cannot stop timed-out Android JADX helper")?;
            let _ = child.wait().await;
            bail!("Android JADX helper timed out")
        }
    };
    if !status.success() {
        let mut bytes = Vec::new();
        stderr
            .reopen()
            .context("cannot read helper diagnostics")?
            .take(4096)
            .read_to_end(&mut bytes)
            .context("cannot read helper diagnostics")?;
        bail!(
            "Android JADX helper failed (exit {status}): {}",
            String::from_utf8_lossy(&bytes)
        )
    }
    let path = source.clone();
    let content = tokio::task::spawn_blocking(move || -> Result<(String, bool)> {
        let file =
            fs::File::open(&path).context("Android JADX helper did not produce requested class")?;
        let size = file.metadata()?.len();
        let mut bytes = Vec::new();
        file.take(MAX_SOURCE_BYTES + 1)
            .read_to_end(&mut bytes)
            .context("cannot read decompiled class")?;
        let text = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_SOURCE_BYTES as usize)])
            .to_string();
        Ok((text, size > MAX_SOURCE_BYTES))
    })
    .await
    .context("helper result reader failed")??;
    Ok(serde_json::json!({"status":"ok","class":class_name,"helper_version":info.helper_version,"source":content.0,"truncated":content.1}).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rejects_jvm_class_jar_and_checks_dex_entrypoint() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let class_jar = dir.path().join("class.jar");
        let mut zip = zip::ZipWriter::new(fs::File::create(&class_jar)?);
        zip.start_file(
            "com/nl2sh/jadx/Main.class",
            zip::write::FileOptions::default(),
        )?;
        zip.write_all(b"JVM class")?;
        zip.finish()?;
        assert!(validate_dex_jar(&class_jar).is_err());

        let dex_jar = dir.path().join("dex.jar");
        let mut zip = zip::ZipWriter::new(fs::File::create(&dex_jar)?);
        zip.start_file("classes.dex", zip::write::FileOptions::default())?;
        zip.write_all(b"dex\n035\0Lcom/nl2sh/jadx/Main;")?;
        zip.finish()?;
        assert!(validate_dex_jar(&dex_jar).is_ok());
        Ok(())
    }

    #[test]
    fn app_process_command_uses_dex_classpath_and_fixed_entrypoint() {
        let command = helper_command(
            Path::new("/data/local/tmp/helper.jar"),
            Path::new("/data/local/tmp/app.apk"),
            "com.example.Main",
            Path::new("/data/local/tmp/out.java"),
            Path::new("/data/local/tmp/work"),
        );
        assert_eq!(command.as_std().get_program(), "/system/bin/app_process");
        let args = command.as_std().get_args().collect::<Vec<_>>();
        assert_eq!(args[0], "-Djava.io.tmpdir=/data/local/tmp/work");
        assert_eq!(args[1], "/");
        assert_eq!(args[2], ENTRYPOINT);
        assert_eq!(args[3], "/data/local/tmp/app.apk");
        assert!(command
            .as_std()
            .get_envs()
            .any(|(name, value)| name == "CLASSPATH"
                && value == Some(std::ffi::OsStr::new("/data/local/tmp/helper.jar"))));
    }

    #[test]
    fn helper_protocol_and_signed_version_are_enforced() {
        let mut info = JadxInfo {
            protocol: 1,
            helper_version: "0.2.0".into(),
            jadx_core: "1.5.1".into(),
            features: vec!["single_class".into()],
        };
        assert!(validate_info(&info, Some("0.2.0")).is_ok());
        assert!(validate_info(&info, Some("0.1.0")).is_err());
        info.protocol = 2;
        assert!(validate_info(&info, None).is_err());
    }

    fn policy_artifact() -> super::super::manifest::RuntimeArtifact {
        super::super::manifest::RuntimeArtifact {
            version: "9.9.9".into(),
            protocol: 1,
            url: "https://example.invalid/signed.jar".into(),
            signature_url: "https://example.invalid/signed.jar.sig".into(),
            sha256: "AA".repeat(32),
            size_bytes: 1,
            min_android_api: 26,
            package_name: None,
            certificate_sha256: None,
            features: Vec::new(),
        }
    }

    #[test]
    fn source_precedence_prefers_explicit_then_signed_policy_then_pin() -> Result<()> {
        let signed = choose_source(None, Some(policy_artifact()))?;
        assert_eq!(signed.trust, HelperTrust::SignedPolicy);
        assert_eq!(signed.url, "https://example.invalid/signed.jar");
        assert_eq!(
            signed.signed.map(|artifact| artifact.version),
            Some("9.9.9".into())
        );

        let pinned = choose_source(None, None)?;
        assert_eq!(pinned.trust, HelperTrust::ReleasePin);
        assert_eq!(pinned.url, RELEASE_PIN_URL);
        assert_eq!(pinned.sha, RELEASE_PIN_SHA256);
        assert!(pinned.signed.is_none());

        let explicit = choose_source(
            Some(("https://example.invalid/mine.jar".into(), "BB".repeat(32))),
            Some(policy_artifact()),
        )?;
        assert_eq!(explicit.trust, HelperTrust::Explicit);
        assert_eq!(explicit.url, "https://example.invalid/mine.jar");
        assert_eq!(explicit.sha, "bb".repeat(32));
        assert!(explicit.signed.is_none());
        Ok(())
    }

    #[test]
    fn explicit_source_requires_a_well_formed_digest() {
        for bad in ["short".to_string(), "ZZ".repeat(32), String::new()] {
            assert!(
                choose_source(Some(("https://example.invalid/a.jar".into(), bad)), None).is_err()
            );
        }
    }

    #[test]
    fn compiled_release_pin_is_a_valid_https_digest_pair() -> Result<()> {
        assert_eq!(RELEASE_PIN_SHA256.len(), 64);
        assert!(RELEASE_PIN_SHA256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit()));
        let source = choose_source(None, None)?;
        validate_source_url(&source.url)?;
        assert!(source.url.contains("/releases/download/v"));
        Ok(())
    }

    #[test]
    fn source_urls_reject_plaintext_and_embedded_credentials() {
        for bad in [
            "http://example.invalid/a.jar",
            "https://user:pass@example.invalid/a.jar",
            "https://example.invalid/a.jar#fragment",
        ] {
            assert!(validate_source_url(bad).is_err(), "{bad}");
        }
        assert!(validate_source_url("https://example.invalid/a.jar").is_ok());
    }

    #[test]
    fn install_preview_names_the_digest_before_any_request() -> Result<()> {
        let preview = install_preview()?;
        assert!(preview.contains(RELEASE_PIN_SHA256), "{preview}");
        Ok(())
    }

    #[test]
    fn advisory_hint_distinguishes_absent_from_unprovisionable() {
        // The compiled-in release pin always resolves, so an uninstalled helper is "absent"
        // (recoverable through jadx_install) rather than "unprovisionable".
        let hint = installed_hint();
        assert!(
            matches!(hint, "installed" | "absent"),
            "unexpected hint {hint}"
        );
        if !std::env::var_os("NL2SH_JADX_ANDROID_HELPER_PATH").is_some() {
            assert_ne!(hint, "unprovisionable");
        }
    }
}
