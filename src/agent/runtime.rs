use crate::shell::ExecutionResult;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::{Duration, Instant},
};

/// Compile-time ceiling that ordinary configuration can only lower.
pub const SYSTEM_HARD_MAX_AGENT_STEPS: usize = 200;

/// Stable reason why a task stopped before model completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitType {
    /// Configured task step budget was exhausted.
    Step,
    /// Absolute system step ceiling was exhausted.
    SystemHardStep,
    /// Tool-call budget was exhausted.
    ToolCalls,
    /// Active task wall-clock budget was exhausted.
    ExecutionTime,
    /// Consecutive no-progress rounds exceeded policy.
    Stalled,
}

/// Runtime counters returned with every completed or bounded task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskStats {
    /// Completed model/act/result steps.
    pub steps_used: usize,
    /// Tool calls admitted by the runtime, including rejected/unsupported calls.
    pub tool_calls_used: usize,
    /// Active runtime excluding user confirmation waits.
    pub active_time: Duration,
    /// Consecutive no-progress steps at task end.
    pub stalled_steps: usize,
    /// Number of forced replans.
    pub replans: usize,
    /// Limit responsible for termination, if any.
    pub limit_reached: Option<LimitType>,
}

pub(crate) struct TaskRuntime {
    started: Instant,
    confirmation_time: Duration,
    pub(crate) steps_used: usize,
    pub(crate) tool_calls_used: usize,
    pub(crate) stalled_steps: usize,
    pub(crate) replans: usize,
}

impl TaskRuntime {
    pub(crate) fn new() -> Self {
        Self {
            started: Instant::now(),
            confirmation_time: Duration::ZERO,
            steps_used: 0,
            tool_calls_used: 0,
            stalled_steps: 0,
            replans: 0,
        }
    }

    pub(crate) fn active_time(&self) -> Duration {
        self.started
            .elapsed()
            .saturating_sub(self.confirmation_time)
    }

    pub(crate) fn add_confirmation_time(&mut self, duration: Duration) {
        self.confirmation_time = self.confirmation_time.saturating_add(duration);
    }

    pub(crate) fn stats(&self, limit_reached: Option<LimitType>) -> TaskStats {
        TaskStats {
            steps_used: self.steps_used,
            tool_calls_used: self.tool_calls_used,
            active_time: self.active_time(),
            stalled_steps: self.stalled_steps,
            replans: self.replans,
            limit_reached,
        }
    }
}

pub(crate) fn normalize_command(command: &str) -> String {
    command.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn action_fingerprint(command: &str, result: &ExecutionResult) -> u64 {
    let mut hasher = DefaultHasher::new();
    normalize_command(command).hash(&mut hasher);
    result.exit_code.hash(&mut hasher);
    result.timed_out.hash(&mut hasher);
    result.interrupted.hash(&mut hasher);
    result.stdout.trim().hash(&mut hasher);
    result.stderr.trim().hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandOutcomeStatus {
    Complete,
    Partial,
    Failed,
    TimedOut,
}

impl CommandOutcomeStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
        }
    }

    pub(crate) const fn has_evidence(self) -> bool {
        matches!(self, Self::Complete | Self::Partial)
    }
}

pub(crate) fn command_outcome_status(result: &ExecutionResult) -> CommandOutcomeStatus {
    if result.timed_out || result.interrupted {
        return CommandOutcomeStatus::TimedOut;
    }
    if result.exit_code == Some(0) {
        return CommandOutcomeStatus::Complete;
    }
    if !result.stdout.trim().is_empty() {
        return CommandOutcomeStatus::Partial;
    }
    CommandOutcomeStatus::Failed
}

#[cfg(test)]
mod tests {
    use super::{command_outcome_status, CommandOutcomeStatus};
    use crate::shell::ExecutionResult;

    fn result(exit_code: Option<i32>, stdout: &str) -> ExecutionResult {
        ExecutionResult {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code,
            timed_out: false,
            interrupted: false,
        }
    }

    #[test]
    fn preserves_partial_evidence_from_nonzero_compound_commands() {
        assert_eq!(
            command_outcome_status(&result(Some(1), "/system/bin/sqlite3\n")),
            CommandOutcomeStatus::Partial
        );
        assert_eq!(
            command_outcome_status(&result(Some(1), "")),
            CommandOutcomeStatus::Failed
        );
    }
}
