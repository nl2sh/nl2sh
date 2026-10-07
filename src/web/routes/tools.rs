use super::super::*;

pub(in crate::web) async fn get_device_overview(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<serde_json::Value>> {
    let path = state.path.clone();
    let mut cfg =
        tokio::task::spawn_blocking(move || config::load_or_default_unvalidated(&path)).await??;
    cfg.execute_user_mode = ExecuteUserMode::Normal;
    cfg.enable_pty = false;
    cfg.execute_timeout_secs = cfg.execute_timeout_secs.clamp(1, 15);
    let executor = ShellExecutor::new(cfg);
    let details = inspect_environment(&executor).await?;
    Ok(Json(serde_json::from_str(&details)?))
}

#[derive(Serialize)]
pub(in crate::web) struct ToolSummary {
    pub(in crate::web) name: String,
    pub(in crate::web) description: String,
    pub(in crate::web) category: String,
    pub(in crate::web) risk: String,
    pub(in crate::web) group: Option<&'static str>,
    pub(in crate::web) enabled: bool,
    pub(in crate::web) available: bool,
    pub(in crate::web) descriptor: &'static crate::tools::ToolDescriptor,
    pub(in crate::web) parameters: serde_json::Value,
}

pub(in crate::web) async fn get_tools(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<Vec<ToolSummary>>> {
    let cfg = load_config(state.path.clone()).await?;
    let capabilities = if cfg.ima_enabled {
        vec![Capability::Ima]
    } else {
        Vec::new()
    };
    let executor = ShellExecutor::new(cfg.clone());
    let runtime = crate::runtime::RuntimeCapabilities::discover(&cfg, &executor).await;
    let registry = ToolRegistry::catalog(&[Capability::Ima]);
    Ok(Json(
        registry
            .definitions()
            .into_iter()
            .filter_map(|tool| {
                let metadata = registry.get(&tool.name)?.metadata();
                Some(ToolSummary {
                    name: tool.name.clone(),
                    description: tool.description,
                    category: format!("{:?}", metadata.category),
                    risk: format!("{:?}", metadata.risk),
                    group: metadata.group.map(crate::tools::ToolGroup::id),
                    enabled: metadata.available(&capabilities)
                        && crate::tools::tool_enabled(&cfg, &tool.name),
                    available: runtime.supports(metadata),
                    descriptor: metadata,
                    parameters: tool.parameters,
                })
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::web) struct ToolToggle {
    pub(in crate::web) group: Option<String>,
    pub(in crate::web) tool: Option<String>,
    pub(in crate::web) enabled: bool,
}

pub(in crate::web) async fn toggle_tool(
    State(state): State<Arc<Shared>>,
    Json(change): Json<ToolToggle>,
) -> ApiResult<Json<Vec<ToolSummary>>> {
    let path = state.path.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let mut cfg = config::load_or_default_unvalidated(&path)?;
        match (change.group, change.tool) {
            (Some(group), None)
                if crate::tools::optional_groups()
                    .iter()
                    .any(|known| known.id() == group) =>
            {
                cfg.tool_overrides
                    .retain(|name, _| !crate::tools::tool_in_group(name, &group));
                cfg.tool_groups.insert(group, change.enabled);
            }
            (None, Some(tool)) if crate::tools::optional_tool_names().contains(&tool.as_str()) => {
                cfg.tool_overrides.insert(tool, change.enabled);
            }
            _ => anyhow::bail!("specify one known optional tool or group"),
        }
        cfg.validate_runtime()?;
        config::save_config(&path, &cfg)
    })
    .await??;
    get_tools(State(state)).await
}

#[derive(Serialize)]
pub(in crate::web) struct InstalledApp {
    pub(in crate::web) package: String,
}

pub(in crate::web) async fn get_apps(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<Vec<InstalledApp>>> {
    let path = state.path.clone();
    let mut cfg =
        tokio::task::spawn_blocking(move || config::load_or_default_unvalidated(&path)).await??;
    cfg.execute_user_mode = ExecuteUserMode::Normal;
    cfg.enable_pty = false;
    cfg.execute_timeout_secs = cfg.execute_timeout_secs.clamp(1, 15);
    let executor = ShellExecutor::new(cfg);
    let output = list_android_apps(
        &executor,
        &ListAndroidAppsArgs {
            scope: None,
            limit: Some(500),
        },
    )
    .await?;
    let value: serde_json::Value = serde_json::from_str(&output)?;
    if value["status"] == "failed" || value["status"] == "timed_out" {
        return Err(ApiError::bad(anyhow!("cannot list Android applications")));
    }
    let apps = value["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["package"].as_str())
        .map(|package| InstalledApp {
            package: package.to_owned(),
        })
        .collect();
    Ok(Json(apps))
}
