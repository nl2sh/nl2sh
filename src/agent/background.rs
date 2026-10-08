//! Task-owned timers for trusted tool continuations, never parsed from model text.
use crate::llm::ToolCall;
use anyhow::{bail, Result};
use std::{collections::VecDeque, time::Duration};
use tokio::time::Instant;

pub(crate) struct Continuation {
    pub(crate) ready_at: Instant,
    pub(crate) call: ToolCall,
    audit: Option<crate::audit::AuditContext>,
    finished: bool,
}

impl Continuation {
    pub(crate) fn event(&self, status: &'static str) {
        if let Some(audit) = &self.audit {
            audit.background_event(&self.call.id, &self.call.name, status);
        }
    }
    pub(crate) fn finish(&mut self, success: bool) {
        self.event(if success { "completed" } else { "failed" });
        self.finished = true;
    }
}

impl Drop for Continuation {
    fn drop(&mut self) {
        if !self.finished {
            self.event("cancelled");
        }
    }
}

#[derive(Default)]
pub(crate) struct BackgroundQueue {
    enabled: bool,
    sequence: u64,
    pending: VecDeque<Continuation>,
}

impl BackgroundQueue {
    pub(crate) fn enable(&mut self) {
        self.enabled = true;
    }

    // Only a successfully executed built-in tool may register an action. Direct
    // invocation has no Agent owner and deliberately leaves this queue disabled.
    pub(crate) fn schedule(&mut self, delay: Duration, mut call: ToolCall) -> Result<bool> {
        if !self.enabled {
            return Ok(false);
        }
        if delay > Duration::from_secs(125) || self.pending.len() >= 16 {
            bail!("background continuation exceeds task queue limits")
        }
        self.sequence += 1;
        call.id = format!("nl2sh-background-{}", self.sequence);
        let job = Continuation {
            ready_at: Instant::now() + delay,
            call,
            audit: crate::audit::AuditContext::fork_current(),
            finished: false,
        };
        job.event("scheduled");
        let index = self
            .pending
            .iter()
            .position(|p| p.ready_at > job.ready_at)
            .unwrap_or(self.pending.len());
        self.pending.insert(index, job);
        Ok(true)
    }

    pub(crate) fn pop(&mut self) -> Option<Continuation> {
        self.pending.pop_front()
    }

    pub(crate) fn len(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn call(name: &str) -> ToolCall {
        ToolCall {
            id: "ignored".into(),
            name: name.into(),
            arguments: serde_json::json!({}),
        }
    }
    #[test]
    fn direct_calls_do_not_schedule_and_task_queues_are_isolated_and_bounded() -> Result<()> {
        let mut direct = BackgroundQueue::default();
        assert!(!direct.schedule(Duration::ZERO, call("direct"))?);
        assert_eq!(direct.len(), 0);
        let mut first = BackgroundQueue::default();
        let mut second = BackgroundQueue::default();
        first.enable();
        second.enable();
        first.schedule(Duration::from_secs(30), call("later"))?;
        first.schedule(Duration::ZERO, call("earlier"))?;
        second.schedule(Duration::ZERO, call("other task"))?;
        assert_eq!(first.pop().expect("earlier").call.name, "earlier");
        assert_eq!(second.pop().expect("other").call.name, "other task");
        assert_eq!(first.pop().expect("later").call.name, "later");
        assert!(first
            .schedule(Duration::from_secs(126), call("too long"))
            .is_err());
        for _ in 0..16 {
            first.schedule(Duration::ZERO, call("bounded"))?;
        }
        assert!(first.schedule(Duration::ZERO, call("overflow")).is_err());
        Ok(())
    }
}
