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
    })))
}
