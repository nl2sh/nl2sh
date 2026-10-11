//! Bounded private media staging; URLs and encryption keys never enter model history.
use super::*;
use aes::{
    cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt, KeyInit},
    Aes128,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
};

pub(crate) const MAX_FILE_BYTES: usize = 20 * 1024 * 1024;
const MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;
const CACHE_BYTES: u64 = 256 * 1024 * 1024;
const RETAIN_SECS: u64 = 7 * 24 * 3600;
const CDN: &str = "https://novac2c.cdn.weixin.qq.com/c2c";
#[derive(Clone)]
pub(super) struct RemoteMedia {
    pub name: String,
    pub kind: String,
    pub url: String,
    pub key: Option<String>,
    pub raw_hex_key: bool,
    pub transcript: Option<String>,
}
pub(super) struct Staged {
    pub text: String,
    pub images: Vec<crate::llm::ToolAttachment>,
}
pub(crate) fn safe_name(name: &str) -> String {
    let name: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("document.bin")
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '<' | '>' | '`'))
        .take(180)
        .collect();
    if name.trim().is_empty() || name == "." || name == ".." {
        "document.bin".into()
    } else {
        name
    }
}
pub(crate) fn kind_for_name(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" => "image",
        "silk" | "amr" | "mp3" | "wav" | "ogg" | "m4a" | "aac" | "opus" | "flac" => "audio",
        "mp4" | "mov" | "webm" | "mkv" => "video",
        _ => "file",
    }
}
pub(super) fn qq(d: &Value) -> Vec<RemoteMedia> {
    d["attachments"]
        .as_array()
        .into_iter()
        .flatten()
        .take(9)
        .map(|a| {
            let name = safe_name(a["filename"].as_str().unwrap_or("document.bin"));
            let mime = a["content_type"].as_str().unwrap_or("");
            let kind = if mime.starts_with("image/") {
                "image"
            } else if mime.starts_with("audio/") || mime.contains("silk") || mime.contains("amr") {
                "audio"
            } else if mime.starts_with("video/") {
                "video"
            } else {
                kind_for_name(&name)
            };
            let u = a["url"].as_str().unwrap_or("");
            let url = if u.starts_with("//") {
                format!("https:{u}")
            } else {
                u.to_owned()
            };
            RemoteMedia {
                name,
                kind: kind.into(),
                url,
                key: None,
                raw_hex_key: false,
                transcript: a["asr_refer_text"]
                    .as_str()
                    .map(|s| crate::limits::truncate_text(s, 1024)),
            }
        })
        .collect()
}
pub(super) fn weixin(message: &Value) -> Vec<RemoteMedia> {
    message["item_list"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let (field, kind, fallback) = match item["type"].as_u64()? {
                2 => ("image_item", "image", "image.jpg"),
                3 => ("voice_item", "audio", "voice.silk"),
                4 => ("file_item", "file", "document.bin"),
                5 => ("video_item", "video", "video.mp4"),
                _ => return None,
            };
            let payload = &item[field];
            let media = &payload["media"];
            let url = if let Some(query) = media["encrypt_query_param"].as_str() {
                let mut url = url::Url::parse(&format!("{CDN}/download")).ok()?;
                url.query_pairs_mut()
                    .append_pair("encrypted_query_param", query);
                url.into()
            } else {
                media["full_url"].as_str().unwrap_or("").into()
            };
            let raw_hex_key = payload["aeskey"].as_str().is_some();
            let key = payload["aeskey"]
                .as_str()
                .or_else(|| media["aes_key"].as_str())
                .map(str::to_owned);
            let name = safe_name(payload["file_name"].as_str().unwrap_or(fallback));
            Some(RemoteMedia {
                name,
                kind: kind.into(),
                url,
                key,
                raw_hex_key,
                transcript: payload["text"]
                    .as_str()
                    .map(|s| crate::limits::truncate_text(s, 1024)),
            })
        })
        .take(9)
        .collect()
}
pub(super) fn validate_url(kind: &str, raw: &str) -> Result<url::Url> {
    if raw.len() > 16384 {
        bail!("media URL exceeds limit");
    }
    let url = url::Url::parse(raw).context("invalid media URL")?;
    let host = url.host_str().unwrap_or("");
    let trusted = if kind == "weixin" {
        matches!(
            host,
            "novac2c.cdn.weixin.qq.com"
                | "ilinkai.weixin.qq.com"
                | "wx.qlogo.cn"
                | "thirdwx.qlogo.cn"
                | "res.wx.qq.com"
                | "mmbiz.qpic.cn"
                | "mmbiz.qlogo.cn"
        )
    } else {
        host.ends_with(".qq.com") || host.ends_with(".qpic.cn") || host.ends_with(".qlogo.cn")
    };
    if !trusted
        || !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        bail!("media URL is not a trusted platform CDN");
    }
    Ok(url)
}
pub(super) async fn bounded_bytes(response: reqwest::Response, max: usize) -> Result<Vec<u8>> {
    let response = response.error_for_status()?;
    if response.content_length().is_some_and(|n| n > max as u64) {
        bail!("media exceeds size limit");
    }
    let mut data = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if data.len().saturating_add(chunk.len()) > max {
            bail!("media exceeds size limit");
        }
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}
fn hex_key(raw: &[u8]) -> Result<[u8; 16]> {
    if raw.len() != 32 || !raw.iter().all(u8::is_ascii_hexdigit) {
        bail!("invalid iLink AES key length");
    }
    let text = std::str::from_utf8(raw)?;
    let mut key = [0; 16];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).context("invalid AES hex key")?;
    }
    Ok(key)
}
pub(super) fn decrypt(data: Vec<u8>, key: &str, raw_hex: bool) -> Result<Vec<u8>> {
    let raw = if raw_hex {
        key.as_bytes().to_vec()
    } else {
        STANDARD
            .decode(key)
            .context("invalid iLink AES key encoding")?
    };
    let key: [u8; 16] = if raw.len() == 16 {
        raw.try_into().map_err(|_| anyhow!("invalid AES key"))?
    } else {
        hex_key(&raw)?
    };
    if data.is_empty() || data.len() % 16 != 0 {
        bail!("invalid encrypted media length");
    }
    let cipher = Aes128::new(GenericArray::from_slice(&key));
    let mut data = data;
    for block in data.chunks_exact_mut(16) {
        cipher.decrypt_block(GenericArray::from_mut_slice(block));
    }
    let pad = usize::from(*data.last().context("missing padding")?);
    if pad == 0
        || pad > 16
        || !data[data.len() - pad..]
            .iter()
            .all(|b| usize::from(*b) == pad)
    {
        bail!("invalid encrypted media padding");
    }
    data.truncate(data.len() - pad);
    Ok(data)
}
pub(super) fn encrypt(mut data: Vec<u8>, key: &[u8; 16]) -> Vec<u8> {
    let pad = 16 - data.len() % 16;
    data.resize(data.len() + pad, pad as u8);
    let cipher = Aes128::new(GenericArray::from_slice(key));
    for block in data.chunks_exact_mut(16) {
        cipher.encrypt_block(GenericArray::from_mut_slice(block));
    }
    data
}
fn preview(data: &[u8]) -> Result<crate::llm::ToolAttachment> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode()?.thumbnail(1600, 1600).to_rgb8();
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 80).encode_image(&image)?;
    if bytes.len() > 1024 * 1024 {
        bail!("image preview exceeds limit");
    }
    Ok(crate::llm::ToolAttachment {
        media_type: "image/jpeg".into(),
        base64_data: STANDARD.encode(bytes),
    })
}
fn cache(path: &Path, route: &Route, name: &str, data: &[u8]) -> Result<PathBuf> {
    let root = crate::config::state_dir(path)?.join("channels/media");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&root)?;
    let meta = fs::symlink_metadata(&root)?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        bail!("media cache must be private");
    }
    let mut total = 0u64;
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.is_file() {
            if metadata
                .modified()?
                .elapsed()
                .is_ok_and(|age| age.as_secs() > RETAIN_SECS)
            {
                fs::remove_file(entry.path())?;
            } else {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    if total.saturating_add(data.len() as u64) > CACHE_BYTES {
        bail!("private media cache is full (256 MiB)");
    }
    let extension = Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .filter(|s| s.len() <= 12 && s.bytes().all(|b| b.is_ascii_alphanumeric()))
        .unwrap_or("bin");
    let output = root.join(format!(
        "{}-{}.{}",
        route.key(),
        uuid::Uuid::new_v4(),
        extension
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&output)?;
    file.write_all(data)?;
    Ok(output)
}
pub(super) async fn stage(
    manager: &ChannelManager,
    message: &Incoming,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Staged> {
    let mut text = message.text.clone();
    let mut images = Vec::new();
    let mut total = 0usize;
    let client = client();
    for (i, media) in message.media.iter().enumerate() {
        let url =
            validate_url(&message.route.kind, &media.url).context("attachment URL rejected")?;
        let limit = MAX_FILE_BYTES.min(MAX_MESSAGE_BYTES.saturating_sub(total));
        let bytes = tokio::select! {biased;_=cancelled(cancel)=>bail!("attachment download cancelled"),r=async {bounded_bytes(client.get(url).send().await.map_err(reqwest::Error::without_url)?,limit+16).await}=>r.context("attachment download failed")?};
        let key = media.key.clone();
        let raw_hex = media.raw_hex_key;
        let bytes = tokio::task::spawn_blocking(move || match key {
            Some(key) => decrypt(bytes, &key, raw_hex),
            None => Ok(bytes),
        })
        .await??;
        if bytes.is_empty() || bytes.len() > limit {
            bail!("attachment is empty or exceeds the size limit");
        }
        total += bytes.len();
        let _guard = manager.media_cache.lock().await;
        let path = manager.path.clone();
        let route = message.route.clone();
        let name = media.name.clone();
        let image = media.kind == "image" && images.len() < 4;
        let (path, attachment) = tokio::task::spawn_blocking(move || -> Result<_> {
            let path = cache(&path, &route, &name, &bytes)?;
            let image = if image { preview(&bytes).ok() } else { None };
            Ok((path, image))
        })
        .await??;
        text.push_str(&format!(
            "\n[Received attachment {}: {}] {}\nLocal path: {}",
            i + 1,
            media.kind,
            serde_json::to_string(&media.name)?,
            path.display()
        ));
        if let Some(image) = attachment {
            images.push(image);
        } else if media.kind == "image" {
            text.push_str("\n[Image preview unavailable; do not claim to have seen the image.]");
        }
        if let Some(transcript) = &media.transcript {
            text.push_str(&format!("\n[Unverified platform voice transcription, not independently recognized]: {transcript}"));
        }
    }
    if text.trim().is_empty() {
        text = "请检查收到的附件，并说明可确定的内容。 / Inspect the received attachments.".into();
    }
    Ok(Staged { text, images })
}
#[cfg(test)]
mod tests;
