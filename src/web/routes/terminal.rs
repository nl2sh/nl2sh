use super::super::*;

pub(in crate::web) struct TerminalClientGuard(pub(in crate::web) Arc<WebSession>);

impl Drop for TerminalClientGuard {
    fn drop(&mut self) {
        self.0.terminal_clients.fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(Deserialize)]
pub(in crate::web) struct TerminalCommand {
    pub(in crate::web) command: String,
}

pub(in crate::web) async fn terminal_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<Shared>>,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<impl IntoResponse> {
    let current = session(&state, &id)?;
    Ok(ws.on_upgrade(move |socket| terminal_socket(socket, state, current)))
}

pub(in crate::web) async fn terminal_socket(
    mut socket: WebSocket,
    state: Arc<Shared>,
    current: Arc<WebSession>,
) {
    current.terminal_clients.fetch_add(1, Ordering::Relaxed);
    let _terminal_client = TerminalClientGuard(current.clone());
    while let Some(Ok(message)) = socket.recv().await {
        let WsMessage::Text(text) = message else {
            continue;
        };
        let response = match serde_json::from_str::<TerminalCommand>(&text) {
            Ok(request) => run_terminal_command(&state, &current, request.command)
                .await
                .unwrap_or_else(|e| format!("error: {e:#}")),
            Err(e) => format!("error: invalid terminal message: {e}"),
        };
        if socket.send(WsMessage::Text(response.into())).await.is_err() {
            break;
        }
    }
}

pub(in crate::web) async fn run_terminal_command(
    state: &Shared,
    current: &Arc<WebSession>,
    command: String,
) -> Result<String> {
    let cfg = load_config(state.path.clone()).await?;
    let session_id = current
        .inner
        .lock()
        .map_err(|_| anyhow!("session lock poisoned"))?
        .id
        .clone();
    crate::audit::tool_scope(
        &cfg,
        "web_terminal",
        Some(session_id),
        "execute_shell_command",
        run_terminal_command_scoped(current, command, &cfg),
    )
    .await
}

pub(in crate::web) async fn run_terminal_command_scoped(
    current: &Arc<WebSession>,
    mut command: String,
    cfg: &Config,
) -> Result<String> {
    if command.trim().is_empty() || command.len() > 16 * 1024 {
        bail!("command must contain 1–16384 bytes")
    }
    let confirmer = WebConfirmer {
        session: current.clone(),
        state: None,
    };
    let mut assessment = crate::security::assess(&command, &cfg);
    crate::audit::record_assessment(&command, &assessment);
    let mut approved_command = None;
    while assessment.requires_confirmation {
        let decision = confirmer.confirm(&command, &assessment).await?;
        crate::audit::record_decision(&decision);
        match decision {
            ConfirmationDecision::Approve
            | ConfirmationDecision::ApproveCaptured
            | ConfirmationDecision::ApproveInteractive
            | ConfirmationDecision::ApproveForTask
            | ConfirmationDecision::ApproveForRun => {
                approved_command = Some(command.clone());
                break;
            }
            ConfirmationDecision::Reject => bail!("command rejected"),
            ConfirmationDecision::Edit(edited) => {
                command = edited;
                assessment = crate::security::assess(&command, &cfg);
                crate::audit::record_assessment(&command, &assessment);
            }
        }
    }
    let capability =
        PrivilegeBroker::authorize(&command, &assessment, &cfg, approved_command.as_deref())?;
    let output = Arc::new(WebOutput {
        session: current.clone(),
    });
    let executor = CapturedExecutor(ShellExecutor::new(cfg.clone()).with_output(output));
    let result = ExecutionBroker::execute(&executor, capability, false).await?;
    Ok(serde_json::json!({"stdout":result.stdout,"stderr":result.stderr,"exit_code":result.exit_code,"timed_out":result.timed_out,"interrupted":result.interrupted}).to_string())
}
