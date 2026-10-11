use super::super::*;

pub(in crate::web) async fn get_channels(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<serde_json::Value>> {
    let manager = crate::channels::ChannelManager::lookup(&state.path)
        .context("channel manager is unavailable")?;
    Ok(Json(
        serde_json::json!({"channels":manager.snapshot().await?}),
    ))
}
pub(in crate::web) async fn channel_action(
    State(state): State<Arc<Shared>>,
    Json(action): Json<crate::channels::ChannelAction>,
) -> ApiResult<Json<serde_json::Value>> {
    let manager = crate::channels::ChannelManager::lookup(&state.path)
        .context("channel manager is unavailable")?;
    manager.apply(action).await?;
    Ok(Json(
        serde_json::json!({"channels":manager.snapshot().await?}),
    ))
}

/// Render the configured official QQ invitation locally; contains no credentials.
pub(in crate::web) async fn qq_invitation_qr(
    State(state): State<Arc<Shared>>,
) -> ApiResult<impl IntoResponse> {
    let manager = crate::channels::ChannelManager::lookup(&state.path)
        .context("channel manager unavailable")?;
    let status = manager.snapshot().await?;
    let url = status
        .iter()
        .find(|s| s.kind == "qq")
        .and_then(|s| s.invite_url.as_deref())
        .context("configure QQ AppID and bot QQ number first")?;
    let svg = crate::channels::qr::svg(url)?;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "image/svg+xml"),
            (axum::http::header::CACHE_CONTROL, "no-store"),
        ],
        svg,
    ))
}
