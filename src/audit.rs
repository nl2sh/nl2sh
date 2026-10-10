//! Bounded execution metadata. Arguments, previews and output never enter audit records.

use crate::{config::Config, history::HistoryLog};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(1);
tokio::task_local! { static CONTEXT: AuditContext; }

#[derive(Clone)]
pub(crate) struct AuditContext {
    log: Option<HistoryLog>,
    task_id: String,
    session_id: String,
    source: &'static str,
    active: Arc<Mutex<Option<Arc<Mutex<AuditEvent>>>>>,
}

/// Metadata emitted exactly once when a admitted tool call finishes or is dropped.
#[derive(Serialize)]
pub(crate) struct AuditEvent {
    request_id: String,
    task_id: String,
    session_id: String,
    tool: String,
    source: &'static str,
    risk: String,
    preview_sha256: Option<String>,
    approval: &'static str,
    grant_id: Option<String>,
    process_uid: u32,
    requested_root: bool,
    result: &'static str,
    duration_ms: u128,
    #[serde(skip)]
    completed: bool,
}

pub(crate) fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn identifier() -> String {
    format!(
        "{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

impl AuditContext {
    pub(crate) fn background_event(&self, id: &str, tool: &str, status: &'static str) {
        if let Some(log) = &self.log {
            let metadata = serde_json::json!({
                "task_id": self.task_id, "session_id": self.session_id,
                "source": self.source, "continuation_id": id, "tool": tool,
                "status": status,
            });
            let _ = log.record("background_continuation", &metadata.to_string());
        }
    }

    pub(crate) fn fork_current() -> Option<Self> {
        CONTEXT
            .try_with(|context| Self {
                active: Arc::new(Mutex::new(None)),
                ..context.clone()
            })
            .ok()
    }

    pub(crate) fn new(config: &Config, source: &'static str, session: Option<String>) -> Self {
        let log = config.source.as_ref().and_then(|path| {
            HistoryLog::open_with_limits(
                path,
                &config.history_log_file,
                config.history_log_event_max_bytes,
                config.history_log_max_bytes,
            )
            .ok()
        });
        let task_id = identifier();
        Self {
            log,
            session_id: session
                .map(|id| digest(&id))
                .unwrap_or_else(|| task_id.clone()),
            task_id,
            source,
            active: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) async fn scope<F: std::future::Future>(self, future: F) -> F::Output {
        CONTEXT.scope(self, future).await
    }
}

pub(crate) struct AuditGuard {
    context: Option<AuditContext>,
    event: Arc<Mutex<AuditEvent>>,
    started: Instant,
}

impl AuditGuard {
    pub(crate) fn begin(tool: &str, risk: &str) -> Self {
        let context = CONTEXT.try_with(Clone::clone).ok();
        let known = crate::tools::builtin_descriptors()
            .iter()
            .any(|descriptor| descriptor.name == tool);
        let event = Arc::new(Mutex::new(AuditEvent {
            request_id: identifier(),
            task_id: context
                .as_ref()
                .map(|c| c.task_id.clone())
                .unwrap_or_default(),
            session_id: context
                .as_ref()
                .map(|c| c.session_id.clone())
                .unwrap_or_default(),
            tool: if known {
                tool.to_owned()
            } else {
                "unsupported".into()
            },
            source: context.as_ref().map_or("internal", |c| c.source),
            risk: risk.into(),
            preview_sha256: None,
            approval: "not_requested",
            grant_id: None,
            process_uid: unsafe { libc::geteuid() },
            requested_root: false,
            result: "cancelled",
            duration_ms: 0,
            completed: false,
        }));
        if let Some(context) = &context {
            if let Ok(mut active) = context.active.lock() {
                *active = Some(event.clone());
            }
        }
        Self {
            context,
            event,
            started: Instant::now(),
        }
    }

    pub(crate) fn finish(&self, result: &'static str) {
        if let Ok(mut event) = self.event.lock() {
            if !event.completed {
                event.result = result;
                event.completed = true;
            }
        }
    }
}

impl Drop for AuditGuard {
    fn drop(&mut self) {
        if let Some(context) = &self.context {
            if let Ok(mut event) = self.event.lock() {
                event.duration_ms = self.started.elapsed().as_millis();
                if let Some(log) = &context.log {
                    let _ = log.record_structured(&*event);
                }
            }
            if let Ok(mut active) = context.active.lock() {
                if active
                    .as_ref()
                    .is_some_and(|event| Arc::ptr_eq(event, &self.event))
                {
                    *active = None;
                }
            }
        }
    }
}

fn update(action: impl FnOnce(&mut AuditEvent)) {
    let _ = CONTEXT.try_with(|context| {
        if let Ok(active) = context.active.lock() {
            if let Some(event) = &*active {
                if let Ok(mut event) = event.lock() {
                    action(&mut event);
                }
            }
        }
    });
}

pub(crate) fn assessment(preview: &str, risk: &str, root: bool, confirmation: bool) {
    update(|event| {
        event.preview_sha256 = Some(digest(preview));
        event.risk = risk.into();
        event.requested_root = root;
        event.approval = if confirmation {
            "pending"
        } else {
            "not_required"
        };
    });
}
pub(crate) fn decision(decision: &crate::agent::ConfirmationDecision) {
    use crate::agent::ConfirmationDecision::*;
    update(|event| {
        if matches!(decision, Reject) {
            event.result = "refused";
            event.completed = true;
        }
        event.grant_id = if let ApproveByGrant(id) = decision {
            Some(id.clone())
        } else {
            None
        };
        event.approval = match decision {
            ApproveByGrant(_) => "approved_by_grant",
            Reject => "rejected",
            Edit(_) => "edited",
            ApproveForTask => "approved_for_task",
            ApproveForRun => "approved_for_run",
            _ => "approved_once",
        }
    });
}
pub(crate) fn result(success: bool) {
    update(|event| {
        if event.completed {
            return;
        }
        event.completed = true;
        event.result = if success {
            "success"
        } else if event.approval == "rejected" {
            "refused"
        } else {
            "error"
        }
    });
}

/// Runs an execution entry point with a correlated, argument-free audit record.
pub async fn tool_scope<T, F>(
    config: &Config,
    source: &'static str,
    session: Option<String>,
    tool: &str,
    future: F,
) -> anyhow::Result<T>
where
    F: std::future::Future<Output = anyhow::Result<T>>,
{
    AuditContext::new(config, source, session)
        .scope(async {
            let guard = AuditGuard::begin(tool, "unassessed");
            let result = future.await;
            if let Ok(mut event) = guard.event.lock() {
                if event.result == "cancelled" {
                    event.result = if event.approval == "rejected" {
                        "refused"
                    } else if result.is_err() {
                        "error"
                    } else {
                        "success"
                    };
                }
            }
            result
        })
        .await
}

/// Records an assessment using a preview digest, without persisting its source text.
pub fn record_assessment(preview: &str, assessment: &crate::security::SecurityAssessment) {
    self::assessment(
        preview,
        &format!("{:?}", assessment.risk_level),
        assessment.requires_root,
        assessment.requires_confirmation,
    );
}

/// Records the categorical local decision without edited command content.
pub fn record_decision(value: &crate::agent::ConfirmationDecision) {
    decision(value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::ConfirmationDecision;

    #[tokio::test]
    async fn audits_refusal_failure_and_cancel_without_argument_or_session_secrets(
    ) -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let config = Config {
            source: Some(directory.path().join("config.toml")),
            ..Config::default()
        };
        let secret = "private-token-do-not-log";
        let failed: anyhow::Result<()> = tool_scope(
            &config,
            "test",
            Some(secret.into()),
            "execute_shell_command",
            async {
                assessment(secret, "Mutating", false, true);
                decision(&ConfirmationDecision::Reject);
                anyhow::bail!("{secret}")
            },
        )
        .await;
        assert!(failed.is_err());
        let _: anyhow::Result<()> =
            tool_scope(&config, "test", None, "unsupported-secret", async {
                anyhow::bail!("{secret}")
            })
            .await;
        let cancelled = tool_scope(&config, "test", None, "execute_shell_command", async {
            assessment(secret, "ReadOnly", false, false);
            std::future::pending::<anyhow::Result<()>>().await
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), cancelled)
                .await
                .is_err()
        );
        let log = std::fs::read_to_string(directory.path().join("nl2sh.log"))?;
        assert!(!log.contains(secret));
        assert!(!log.contains("unsupported-secret"));
        let events: Vec<serde_json::Value> = log
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["result"], "refused");
        assert_eq!(events[0]["approval"], "rejected");
        assert_eq!(events[0]["preview_sha256"], digest(secret));
        assert_eq!(events[1]["result"], "error");
        assert_eq!(events[2]["result"], "cancelled");
        for event in events {
            assert_eq!(event["event"], "tool_audit");
            assert_eq!(event["process_uid"], unsafe { libc::geteuid() });
            assert!(event["request_id"]
                .as_str()
                .is_some_and(|id| !id.is_empty()));
            assert!(event["task_id"].as_str().is_some_and(|id| !id.is_empty()));
        }
        Ok(())
    }
}

pub(crate) fn shell_result(execution: &crate::shell::ExecutionResult) {
    let shell = CONTEXT
        .try_with(|context| {
            context.active.lock().ok().and_then(|active| {
                active.as_ref().and_then(|event| {
                    event
                        .lock()
                        .ok()
                        .map(|event| event.tool == "execute_shell_command")
                })
            })
        })
        .ok()
        .flatten()
        .unwrap_or(false);
    if shell {
        execution_result(execution);
    }
}

pub(crate) fn execution_result(execution: &crate::shell::ExecutionResult) {
    let failure = if execution.interrupted {
        Some("cancelled")
    } else if execution.timed_out {
        Some("timed_out")
    } else if execution.exit_code != Some(0) {
        Some("error")
    } else {
        None
    };
    if let Some(failure) = failure {
        update(|event| {
            if !event.completed {
                event.result = failure;
                event.completed = true;
            }
        });
    }
}
