use super::super::*;

pub(in crate::web) async fn get_info(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<serde_json::Value>> {
    let cfg = load_config(state.path.clone()).await?;
    let executor = ShellExecutor::new(cfg.clone());
    let capabilities = crate::runtime::RuntimeCapabilities::discover(&cfg, &executor).await;
    let runtime_policy = match crate::runtime_dependencies::manifest::embedded() {
        Ok(Some(policy)) => serde_json::json!({"status": "verified", "manifest": policy}),
        Ok(None) => serde_json::json!({"status": "unsigned_source_build"}),
        Err(error) => serde_json::json!({"status": "invalid", "error": error.to_string()}),
    };
    Ok(Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"), "protocol": 1,
        "runtime_policy": runtime_policy,
        "update_ownership": crate::update::ownership().await,
        "pid": std::process::id(), "uptime": state.started.elapsed().as_secs(),
        "port": state.port,
        "abi": match std::env::consts::ARCH {
            "aarch64" => "arm64-v8a", "arm" => "armeabi-v7a", other => other,
        },
        "capabilities": capabilities,
        "connections": crate::protocol::connection_info(&state.path).await,
    })))
}

// Owner connection dialog includes credentials and must never be cached.
pub(in crate::web) async fn get_connections(
    State(state): State<Arc<Shared>>,
) -> impl axum::response::IntoResponse {
    (
        [("cache-control", "no-store")],
        Json(crate::protocol::connection_details(&state.path).await),
    )
}

// Read-only discovery; installation remains with its existing owner.
pub(in crate::web) async fn get_update(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<serde_json::Value>> {
    let cfg = load_config(state.path.clone()).await?;
    let latest = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        crate::update::available_version(&cfg),
    )
    .await
    .map_err(|_| ApiError::bad(anyhow!("update check timed out")))??;
    let owner = crate::update::ownership().await;
    Ok(Json(
        serde_json::json!({"current": env!("CARGO_PKG_VERSION"), "latest": latest,
        "can_install": owner.self_update_allowed && cfg!(target_os = "android"),
        "ownership": owner}),
    ))
}
