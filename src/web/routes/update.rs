use super::super::*;

#[derive(Clone, Serialize, Default)]
pub(in crate::web) struct UpdateJob {
    pub(in crate::web) busy: bool,
    pub(in crate::web) version: Option<String>,
    pub(in crate::web) progress: Option<crate::update::UpdateProgress>,
    pub(in crate::web) error: Option<String>,
}

impl UpdateJob {
    fn begin(&mut self, version: String) -> Result<()> {
        if self.busy {
            bail!("更新正在进行，请查看进度")
        }
        if self
            .progress
            .as_ref()
            .is_some_and(|p| p.stage == "complete")
        {
            bail!("更新已安装，请重启 nl2sh 后再检查更新")
        }
        *self = Self {
            busy: true,
            version: Some(version),
            progress: Some(crate::update::UpdateProgress {
                stage: "checking",
                downloaded: 0,
                total: 0,
            }),
            error: None,
        };
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::web) struct UpdateStart {
    pub(in crate::web) version: String,
}

pub(in crate::web) async fn update_progress(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<UpdateJob>> {
    Ok(Json(
        state
            .update
            .lock()
            .map_err(|_| anyhow!("update lock poisoned"))?
            .clone(),
    ))
}

pub(in crate::web) async fn start_update(
    State(state): State<Arc<Shared>>,
    Json(args): Json<UpdateStart>,
) -> ApiResult<Json<UpdateJob>> {
    if args.version.is_empty()
        || args.version.len() > 64
        || !args
            .version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
    {
        return Err(ApiError::bad(anyhow!("invalid update version")));
    }
    let owner = crate::update::ownership().await;
    if !owner.self_update_allowed {
        return Err(ApiError::bad(anyhow!(owner.message)));
    }
    if !cfg!(target_os = "android") {
        return Err(ApiError::bad(anyhow!("自更新仅支持 Android 安装")));
    }
    let cfg = load_config(state.path.clone()).await?;
    let snapshot = {
        let mut job = state
            .update
            .lock()
            .map_err(|_| anyhow!("update lock poisoned"))?;
        job.begin(args.version.clone()).map_err(ApiError::bad)?;
        job.clone()
    };
    tokio::spawn(async move {
        let result = run_update(&state, &cfg, &args.version).await;
        if let Ok(mut job) = state.update.lock() {
            job.busy = false;
            if let Err(error) = result {
                job.error = Some(format!("{error:#}"));
            }
        }
    });
    Ok(Json(snapshot))
}

async fn run_update(state: &Arc<Shared>, cfg: &Config, version: &str) -> Result<()> {
    let release = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        crate::update::check(cfg),
    )
    .await
    .context("检查更新超时")??
    .context("暂无可安装的新版本")?;
    // Approval binds the displayed version; a changed latest release needs a fresh choice.
    if release.version != version {
        bail!("最新版本已改变，请重新检查更新并选择版本")
    }
    let current = state.clone();
    crate::update::install_with_progress(cfg, &release, move |progress| {
        if let Ok(mut job) = current.update.lock() {
            job.progress = Some(progress);
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_and_installed_jobs_cannot_be_started_again() -> Result<()> {
        let mut job = UpdateJob::default();
        job.begin("1.2.0".into())?;
        assert!(job.busy);
        assert!(job.begin("1.3.0".into()).is_err());
        assert_eq!(job.version.as_deref(), Some("1.2.0"));
        job.busy = false;
        job.error = Some("download failed".into());
        job.begin("1.2.0".into())?;
        assert!(job.error.is_none());
        job.busy = false;
        job.progress = Some(crate::update::UpdateProgress {
            stage: "complete",
            downloaded: 100,
            total: 100,
        });
        assert!(job.begin("1.3.0".into()).is_err());
        Ok(())
    }
}
