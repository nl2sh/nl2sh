//! Approved shell execution entry shared by interactive frontends.

use super::{CommandExecutor, ExecutionResult};
use crate::security::ApprovedShellCommand;
use anyhow::Result;

/// Dispatches a command-bound capability to the existing Android executor.
pub struct ExecutionBroker;

impl ExecutionBroker {
    /// Executes exactly the command and root plan held by the capability.
    pub async fn execute(
        executor: &dyn CommandExecutor,
        approved: ApprovedShellCommand,
        interactive: bool,
    ) -> Result<ExecutionResult> {
        let _lease = if crate::runtime::resources::privileged_android_process()
            && !crate::runtime::resources::ui_lease_held()
        {
            Some(crate::runtime::resources::UiLease::acquire().await?)
        } else {
            None
        };
        let result = executor
            .execute(approved.command(), approved.requires_root(), interactive)
            .await;
        if let Ok(execution) = &result {
            crate::audit::shell_result(execution);
        }

        result
    }
}
