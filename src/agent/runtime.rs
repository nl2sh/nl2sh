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
