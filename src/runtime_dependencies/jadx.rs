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

struct HelperSource {
    url: String,
    sha: String,
    signed: Option<super::manifest::RuntimeArtifact>,
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

/// Whether an explicit offline helper or validated download source can be used after approval.
/// This does not download or execute the helper during discovery.
pub fn provisionable() -> bool {
    if let Some(path) =
        std::env::var_os("NL2SH_JADX_ANDROID_HELPER_PATH").filter(|value| !value.is_empty())
    {
        return Path::new(&path).is_file();
    }
    download_source().is_ok()
}

fn download_source() -> Result<HelperSource> {
    let source = if let Ok(url) = std::env::var("NL2SH_JADX_ANDROID_HELPER_URL") {
        let sha = std::env::var("NL2SH_JADX_ANDROID_HELPER_SHA256")
            .context("NL2SH_JADX_ANDROID_HELPER_SHA256 is required with a custom helper URL")?;
        HelperSource {
            url,
            sha: parse_sha(sha)?,
            signed: None,
        }
    } else {
        let artifact = super::manifest::embedded()?
            .and_then(|manifest| manifest.jadx_helper)
            .context("this build has no signed default JADX policy; configure an explicit offline helper or HTTPS URL with SHA-256")?;
        HelperSource {
            url: artifact.url.clone(),
            sha: artifact.sha256.to_ascii_lowercase(),
            signed: Some(artifact),
        }
    };
    let parsed = reqwest::Url::parse(&source.url).context("invalid Android JADX helper URL")?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        bail!("Android JADX helper URL must be HTTPS without credentials or fragment")
    }
    Ok(source)
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

async fn ensure_helper(config: &Config) -> Result<PathBuf> {
    if let Some(path) =
        std::env::var_os("NL2SH_JADX_ANDROID_HELPER_PATH").filter(|value| !value.is_empty())
    {
        let path = PathBuf::from(path);
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
        return fs::canonicalize(path).context("cannot resolve Android helper path");
    }
    let source = download_source()?;
    let url = source.url;
    let sha = source.sha;
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
    if let Some(artifact) = source.signed {
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

/// Decompiles one APK class using an Android DEX helper and `app_process`.
pub async fn decompile_class(config: &Config, apk_path: &str, class_name: &str) -> Result<String> {
    if !cfg!(target_os = "android") {
        bail!("{{\"error\":\"jadx_android_only\",\"detail\":\"DEX helper decompilation requires the Android app_process runtime\"}}")
    }
    let jar = ensure_helper(config).await.map_err(|error| {
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
}
