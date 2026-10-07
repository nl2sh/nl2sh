use super::super::*;

#[derive(Deserialize)]
pub(in crate::web) struct FileSuggestionQuery {
    pub(in crate::web) fragment: String,
}

pub(in crate::web) async fn get_file_suggestions(
    Query(query): Query<FileSuggestionQuery>,
) -> ApiResult<Json<Vec<String>>> {
    if query.fragment.len() > 1024 || query.fragment.chars().any(char::is_whitespace) {
        return Err(ApiError::bad(anyhow!("invalid file suggestion fragment")));
    }
    let suggestions =
        tokio::task::spawn_blocking(move || file_suggestions(&query.fragment)).await?;
    Ok(Json(suggestions))
}

#[derive(Deserialize)]
pub(in crate::web) struct FilePathQuery {
    pub(in crate::web) path: String,
}

#[derive(Serialize)]
pub(in crate::web) struct BrowserFile {
    pub(in crate::web) name: String,
    pub(in crate::web) path: String,
    pub(in crate::web) is_dir: bool,
    pub(in crate::web) size: Option<u64>,
    pub(in crate::web) modified_ms: Option<u64>,
    pub(in crate::web) preview_kind: Option<&'static str>,
}

pub(in crate::web) fn preview_type(path: &Path) -> Option<(&'static str, &'static str, u64)> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if matches!(
        name.as_str(),
        "readme" | "license" | "makefile" | "dockerfile" | ".gitignore" | ".env.example"
    ) {
        return Some(("text", "text/plain; charset=utf-8", MAX_FILE_PREVIEW_BYTES));
    }
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let (kind, mime, limit) = match extension.as_str() {
        "png" => ("image", "image/png", MAX_IMAGE_PREVIEW_BYTES),
        "jpg" | "jpeg" => ("image", "image/jpeg", MAX_IMAGE_PREVIEW_BYTES),
        "gif" => ("image", "image/gif", MAX_IMAGE_PREVIEW_BYTES),
        "webp" => ("image", "image/webp", MAX_IMAGE_PREVIEW_BYTES),
        "bmp" => ("image", "image/bmp", MAX_IMAGE_PREVIEW_BYTES),
        "avif" => ("image", "image/avif", MAX_IMAGE_PREVIEW_BYTES),
        "mp4" | "m4v" => ("video", "video/mp4", MAX_VIDEO_PREVIEW_BYTES),
        "webm" => ("video", "video/webm", MAX_VIDEO_PREVIEW_BYTES),
        "ogv" => ("video", "video/ogg", MAX_VIDEO_PREVIEW_BYTES),
        "mov" => ("video", "video/quicktime", MAX_VIDEO_PREVIEW_BYTES),
        "wav" => ("audio", "audio/wav", MAX_AUDIO_PREVIEW_BYTES),
        "pcm" | "raw" => (
            "audio",
            "application/octet-stream",
            MAX_RAW_PCM_PREVIEW_BYTES,
        ),
        "mp3" => ("audio", "audio/mpeg", MAX_AUDIO_PREVIEW_BYTES),
        "m4a" | "aac" => ("audio", "audio/mp4", MAX_AUDIO_PREVIEW_BYTES),
        "ogg" | "oga" => ("audio", "audio/ogg", MAX_AUDIO_PREVIEW_BYTES),
        "flac" => ("audio", "audio/flac", MAX_AUDIO_PREVIEW_BYTES),
        "txt" | "log" | "md" | "markdown" | "rs" | "py" | "js" | "jsx" | "ts" | "tsx" | "json"
        | "toml" | "yaml" | "yml" | "xml" | "css" | "html" | "htm" | "sh" | "bash" | "zsh"
        | "java" | "kt" | "kts" | "c" | "h" | "cpp" | "hpp" | "go" | "sql" | "ini" | "conf"
        | "properties" | "gradle" | "gitignore" | "dockerfile" => {
            ("text", "text/plain; charset=utf-8", MAX_FILE_PREVIEW_BYTES)
        }
        _ => return None,
    };
    Some((kind, mime, limit))
}

pub(in crate::web) fn checked_browser_path(path: &str) -> ApiResult<PathBuf> {
    if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
        return Err(ApiError::bad(anyhow!("invalid file path")));
    }
    if path == "~" || path.starts_with("~/") {
        let home = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .ok_or_else(|| ApiError::bad(anyhow!("home directory unavailable")))?;
        Ok(PathBuf::from(home).join(path.trim_start_matches('~').trim_start_matches('/')))
    } else {
        Ok(PathBuf::from(path))
    }
}

pub(in crate::web) async fn get_files(
    Query(query): Query<FilePathQuery>,
) -> ApiResult<Json<Vec<BrowserFile>>> {
    let directory = checked_browser_path(&query.path)?;
    let files = tokio::task::spawn_blocking(move || -> Result<Vec<BrowserFile>> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&directory)
            .context("cannot read directory")?
            .take(MAX_DIRECTORY_ENTRIES)
        {
            let entry = entry.context("cannot read directory entry")?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let is_dir = metadata.is_dir();
            let path = entry.path();
            let Some(path) = path.to_str().map(str::to_owned) else {
                continue;
            };
            let preview_kind = (!is_dir && metadata.is_file())
                .then(|| preview_type(Path::new(&path)))
                .flatten()
                .and_then(|(kind, _, limit)| (metadata.len() <= limit).then_some(kind));
            let modified_ms = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .and_then(|duration| u64::try_from(duration.as_millis()).ok());
            files.push(BrowserFile {
                name,
                path,
                is_dir,
                size: (!is_dir).then_some(metadata.len()),
                modified_ms,
                preview_kind,
            });
        }
        files.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(files)
    })
    .await??;
    Ok(Json(files))
}

pub(in crate::web) fn requested_range(value: &str, length: u64) -> Option<(u64, u64)> {
    let value = value.strip_prefix("bytes=")?;
    if value.contains(',') || length == 0 {
        return None;
    }
    let (start, end) = value.split_once('-')?;
    if start.is_empty() {
        let suffix = end.parse::<u64>().ok()?;
        if suffix == 0 {
            return None;
        }
        return Some((length.saturating_sub(suffix), length - 1));
    }
    let start = start.parse::<u64>().ok()?;
    let end = if end.is_empty() {
        length - 1
    } else {
        end.parse::<u64>().ok()?.min(length - 1)
    };
    (start < length && start <= end).then_some((start, end))
}

pub(in crate::web) async fn get_file_preview(
    Query(query): Query<FilePathQuery>,
    headers: axum::http::HeaderMap,
) -> ApiResult<Response> {
    let path = checked_browser_path(&query.path)?;
    let (_, mime, limit) =
        preview_type(&path).ok_or_else(|| ApiError::bad(anyhow!("unsupported preview type")))?;
    let mut file = tokio::fs::File::open(&path)
        .await
        .context("cannot open preview file")?;
    let metadata = file
        .metadata()
        .await
        .context("cannot inspect preview file")?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(ApiError::bad(anyhow!(
            "preview file is unavailable or too large"
        )));
    }
    let length = metadata.len();
    let range_header = headers.get(header::RANGE);
    let (start, end, status) = if let Some(value) = range_header {
        let range = value
            .to_str()
            .ok()
            .and_then(|value| requested_range(value, length));
        let Some((start, end)) = range else {
            return Ok(Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{length}"))
                .body(Body::empty())
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()));
        };
        (start, end, StatusCode::PARTIAL_CONTENT)
    } else {
        (0, length.saturating_sub(1), StatusCode::OK)
    };
    file.seek(std::io::SeekFrom::Start(start))
        .await
        .context("cannot seek preview file")?;
    let bytes = if length == 0 { 0 } else { end - start + 1 };
    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, bytes.to_string())
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_DISPOSITION, "inline")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    if status == StatusCode::PARTIAL_CONTENT {
        response = response.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{length}"),
        );
    }
    Ok(response
        .body(Body::from_stream(ReaderStream::new(file.take(bytes))))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}
