use super::super::*;
use crate::tools::tailcat::quick;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::web) struct TailcatStart {
    web: bool,
    adb: bool,
    #[serde(default = "default_adb_port")]
    adb_port: u16,
}
fn default_adb_port() -> u16 {
    quick::DEFAULT_ADB_PORT
}
pub(in crate::web) async fn get_tailcat_ports(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<serde_json::Value>> {
    let cfg = load_config(state.path.clone()).await?;
    Ok(Json(
        serde_json::json!({"web_port":state.port,"adb_port":quick::detect_adb_port(&cfg).await}),
    ))
}
pub(in crate::web) async fn get_tailcat() -> ApiResult<Json<quick::Snapshot>> {
    Ok(Json(quick::snapshot()?))
}
pub(in crate::web) async fn start_tailcat(
    State(state): State<Arc<Shared>>,
    Json(args): Json<TailcatStart>,
) -> ApiResult<Json<quick::Snapshot>> {
    let cfg = load_config(state.path.clone()).await?;
    Ok(Json(quick::begin(
        cfg,
        args.web.then_some(state.port),
        args.adb.then_some(args.adb_port),
    )?))
}
