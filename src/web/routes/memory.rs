use super::super::*;

#[derive(Deserialize)]
pub(in crate::web) struct MemorySetRequest {
    pub(in crate::web) key: String,
    pub(in crate::web) value: String,
}

#[derive(Deserialize)]
pub(in crate::web) struct MemoryKeyRequest {
    pub(in crate::web) key: String,
}

pub(in crate::web) async fn get_memory(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<Vec<MemoryEntry>>> {
    memory_entries(state.path.clone()).await
}

pub(in crate::web) async fn set_memory(
    State(state): State<Arc<Shared>>,
    Json(request): Json<MemorySetRequest>,
) -> ApiResult<Json<Vec<MemoryEntry>>> {
    let path = state.path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        AgentMemory::open(&path)?.apply(&AgentMemoryArgs {
            action: AgentMemoryAction::Set,
            key: Some(request.key),
            value: Some(request.value),
        })?;
        Ok(())
    })
    .await??;
    memory_entries(state.path.clone()).await
}

pub(in crate::web) async fn delete_memory(
    State(state): State<Arc<Shared>>,
    Json(request): Json<MemoryKeyRequest>,
) -> ApiResult<Json<Vec<MemoryEntry>>> {
    let path = state.path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        AgentMemory::open(&path)?.apply(&AgentMemoryArgs {
            action: AgentMemoryAction::Delete,
            key: Some(request.key),
            value: None,
        })?;
        Ok(())
    })
    .await??;
    memory_entries(state.path.clone()).await
}

pub(in crate::web) async fn clear_memory(
    State(state): State<Arc<Shared>>,
) -> ApiResult<Json<Vec<MemoryEntry>>> {
    let path = state.path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        AgentMemory::open(&path)?.apply(&AgentMemoryArgs {
            action: AgentMemoryAction::Clear,
            key: None,
            value: None,
        })?;
        Ok(())
    })
    .await??;
    memory_entries(state.path.clone()).await
}

pub(in crate::web) async fn memory_entries(path: PathBuf) -> ApiResult<Json<Vec<MemoryEntry>>> {
    Ok(Json(
        tokio::task::spawn_blocking(move || AgentMemory::open(&path)?.entries()).await??,
    ))
}
