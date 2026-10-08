use super::super::*;
use crate::tools::tailcat::quick;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::web) struct TailcatStart {
    web: bool,
    adb: bool,
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
        args.adb,
    )?))
}
