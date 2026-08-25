use crate::{
    agent::AgentOutcome,
    history::HistoryLog,
    limits::{truncate_text, TRUNCATION_LABEL},
    llm::ConversationItem,
    shell::OutputSink,
};
use anyhow::Result;
use std::sync::{Arc, Mutex};

pub(super) const LIVE_OUTPUT_PREFIX: &str = "\u{1e}LIVE:";
pub(super) const TOOL_RESULT_PREFIX: &str = "\u{1e}RESULT:";
pub(super) const LLM_STREAM_PREFIX: &str = "\u{1e}LLM:";

pub(super) struct SessionOutput {
    pub history: Arc<Mutex<Vec<String>>>,
    pub ascii: bool,
    pub log: HistoryLog,
    pub max_bytes: usize,
}

impl OutputSink for SessionOutput {
    fn stdout(&self, text: &str) {
        self.push(if self.ascii { "[OUT]" } else { "✅" }, "stdout", text);
    }
    fn stderr(&self, text: &str) {
        self.push(if self.ascii { "[ERR]" } else { "❌" }, "stderr", text);
    }
}

impl SessionOutput {
    fn push(&self, prefix: &str, event: &str, text: &str) {
        let _ = self.log.record(event, text);
        if let Ok(mut history) = self.history.lock() {
            let used = history
                .iter()
                .filter(|entry| entry.starts_with(LIVE_OUTPUT_PREFIX))
                .map(String::len)
                .sum::<usize>();
            if used >= self.max_bytes {
                if !history.iter().any(|entry| {
                    entry.starts_with(LIVE_OUTPUT_PREFIX) && entry.contains(TRUNCATION_LABEL)
                }) {
                    history.push(format!(
                        "{LIVE_OUTPUT_PREFIX}{prefix} [... {TRUNCATION_LABEL}: live UI limit {} bytes reached; later live output omitted ...]",
                        self.max_bytes
                    ));
                }
                return;
            }
            let bounded = truncate_text(text, self.max_bytes - used);
            let mut remaining = self.max_bytes - used;
            for line in bounded.lines() {
                let entry = format!("{LIVE_OUTPUT_PREFIX}{prefix} {line}");
                if entry.len() > remaining {
                    let marker = format!(
                        "{LIVE_OUTPUT_PREFIX}{prefix} [... {TRUNCATION_LABEL}: live UI limit {} bytes reached ...]",
                        self.max_bytes
                    );
                    if marker.len() <= remaining {
                        history.push(marker);
                    }
                    break;
                }
                remaining -= entry.len();
                history.push(entry);
            }
        }
    }
}

pub(super) fn append_transcript(
    history: &Arc<Mutex<Vec<String>>>,
    outcome: &AgentOutcome,
    ascii: bool,
    log: &HistoryLog,
) -> Result<()> {
    log.record(
        "task_finished",
        &format!(
            "steps={} tool_calls={} active_ms={} stalled_steps={} replans={} limit={:?}",
            outcome.stats.steps_used,
            outcome.stats.tool_calls_used,
            outcome.stats.active_time.as_millis(),
            outcome.stats.stalled_steps,
            outcome.stats.replans,
            outcome.stats.limit_reached
        ),
    )?;
    let mut visible = history
        .lock()
        .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?;
    visible.retain(|entry| {
        !entry.starts_with(LIVE_OUTPUT_PREFIX) && !entry.starts_with(LLM_STREAM_PREFIX)
    });
    for item in &outcome.transcript {
        if let ConversationItem::Tools(round) = item {
            for call in &round.calls {
                log.record("tool_call", &call.name)?;
                visible.push(format!(
                    "{} {}",
                    if ascii { "[TOOL]" } else { "🔧" },
                    call.name
                ));
                if let Some(command) = call
                    .arguments
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                {
                    log.record("command", command)?;
                    visible.push(format!("{} {command}", if ascii { "[CMD]" } else { "💻" }));
                }
            }
            for result in &round.results {
                log.record(
                    if result.success {
                        "tool_result"
                    } else {
                        "tool_error"
                    },
                    &result.output,
                )?;
                let prefix = if result.success {
                    if ascii {
                        "[OK]"
                    } else {
                        "✅"
                    }
                } else if ascii {
                    "[ERROR]"
                } else {
                    "❌"
                };
                visible.push(encode_tool_result(prefix, &result.output));
            }
        }
    }
    visible.push(format!(
        "{} {}",
        if ascii { "[AGENT]" } else { "🤖" },
        outcome.final_text
    ));
    log.record("agent", &outcome.final_text)?;
    Ok(())
}

pub(super) fn begin_llm_stream(history: &Arc<Mutex<Vec<String>>>) -> Result<()> {
    let mut history = history
        .lock()
        .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?;
    history.retain(|entry| !entry.starts_with(LLM_STREAM_PREFIX));
    history.push(format!("{LLM_STREAM_PREFIX}0:"));
    Ok(())
}

pub(super) fn append_llm_delta(
    history: &Arc<Mutex<Vec<String>>>,
    delta: &str,
    max_bytes: usize,
) -> Result<()> {
    let mut history = history
        .lock()
        .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?;
    let Some(entry) = history
        .iter_mut()
        .rev()
        .find(|entry| entry.starts_with(LLM_STREAM_PREFIX))
    else {
        return Ok(());
    };
    let Some((header, text)) = entry
        .split_once(':')
        .and_then(|(_, rest)| rest.split_once(':'))
    else {
        return Ok(());
    };
    let mut combined = text.to_owned();
    combined.push_str(delta);
    *entry = format!(
        "{LLM_STREAM_PREFIX}{header}:{}",
        truncate_text(&combined, max_bytes)
    );
    Ok(())
}

pub(super) fn advance_llm_gradient(history: &Arc<Mutex<Vec<String>>>) -> Result<()> {
    let mut history = history
        .lock()
        .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?;
    if let Some(entry) = history
        .iter_mut()
        .rev()
        .find(|entry| entry.starts_with(LLM_STREAM_PREFIX))
    {
        if let Some((phase, text)) = entry
            .strip_prefix(LLM_STREAM_PREFIX)
            .and_then(|value| value.split_once(':'))
        {
            let phase = phase.parse::<usize>().unwrap_or(0).wrapping_add(1) % 24;
            *entry = format!("{LLM_STREAM_PREFIX}{phase}:{text}");
        }
    }
    Ok(())
}

pub(super) fn discard_llm_stream(history: &Arc<Mutex<Vec<String>>>) -> Result<()> {
    history
        .lock()
        .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
        .retain(|entry| !entry.starts_with(LLM_STREAM_PREFIX));
    Ok(())
}

pub(super) fn encode_tool_result(prefix: &str, output: &str) -> String {
    format!("{TOOL_RESULT_PREFIX}{prefix}\n{output}")
}

pub(super) fn finalize_live_output(history: &Arc<Mutex<Vec<String>>>) -> Result<()> {
    let mut history = history
        .lock()
        .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?;
    for entry in history.iter_mut() {
        if let Some(visible) = entry.strip_prefix(LIVE_OUTPUT_PREFIX) {
            *entry = visible.to_owned();
        }
    }
    Ok(())
}

pub(super) fn push_history(
    history: &Arc<Mutex<Vec<String>>>,
    value: String,
    log: &HistoryLog,
    event: &str,
) -> Result<()> {
    log.record(event, &value)?;
    history
        .lock()
        .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
        .push(value);
    Ok(())
}

pub(super) fn snapshot(history: &Arc<Mutex<Vec<String>>>) -> Result<Vec<String>> {
    Ok(history
        .lock()
        .map_err(|_| anyhow::anyhow!("TUI history lock is poisoned"))?
        .clone())
}
