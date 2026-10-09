//! Shared bounded task ownership, context serialization and cooperative cancellation.
use super::{
    execution, new_id,
    store::{timestamp, Store},
};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
};
use tokio::sync::{watch, Mutex, Semaphore};

pub(super) enum Operation {
    Inspect,
    Tools,
    Invoke { tool: String, arguments: Value },
    Ask { message: String },
}
pub(super) struct Ticket {
    pub updates: watch::Receiver<Value>,
    pub cancel: watch::Sender<bool>,
}
impl Ticket {
    pub async fn wait(mut self) -> Result<Value> {
        loop {
            let value = self.updates.borrow().clone();
            if terminal(&value) {
                return Ok(value);
            }
            self.updates
                .changed()
                .await
                .context("task worker stopped without final status")?;
        }
    }
}
struct Active {
    cancel: watch::Sender<bool>,
    updates: watch::Receiver<Value>,
}
pub(super) struct Tasks {
    pub path: PathBuf,
    store: Store,
    active: Mutex<HashMap<String, Active>>,
    contexts: Mutex<HashMap<String, Weak<Mutex<()>>>>,
    capacity: Arc<Semaphore>,
    stopping: AtomicBool,
}
impl Tasks {
    pub fn open(path: PathBuf) -> Result<Arc<Self>> {
        execution::load(&path)?;
        let store = Store::open(&path)?;
        Ok(Arc::new(Self {
            path,
            store,
            active: Mutex::new(HashMap::new()),
            contexts: Mutex::new(HashMap::new()),
            capacity: Arc::new(Semaphore::new(16)),
            stopping: AtomicBool::new(false),
        }))
    }
    pub async fn submit(
        self: &Arc<Self>,
        operation: Operation,
        context: Option<String>,
        message: Option<Value>,
    ) -> Result<Ticket> {
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .context("protocol server busy: at most 16 active tasks")?;
        let id = new_id();
        let context = context.unwrap_or_else(new_id);
        validate_id(&context)?;
        let mut task = json!({"id": id, "contextId": context, "status":{"state":"TASK_STATE_SUBMITTED", "timestamp":timestamp()}, "history":message.into_iter().collect::<Vec<_>>()});
        let cfg = execution::load(&self.path)?;
        let mut secrets = vec![
            cfg.api_key,
            cfg.proxy_password,
            cfg.ima_api_key,
            cfg.jev_api_key,
        ];
        if let Ok(token) = std::env::var("NL2SH_PROTOCOL_TOKEN") {
            secrets.push(token);
        }
        redact(&mut task, &secrets);
        let (cancel, rx) = watch::channel(false);
        let (updates, update_rx) = watch::channel(task.clone());
        let mut active = self.active.lock().await;
        if self.stopping.load(Ordering::Acquire) {
            bail!("protocol server shutting down");
        }
        self.store.put(task.clone()).await?;
        active.insert(
            id.clone(),
            Active {
                cancel: cancel.clone(),
                updates: update_rx.clone(),
            },
        );
        drop(active);
        let service = self.clone();
        let job_id = id.clone();
        tokio::spawn(async move {
            let mut task = task;
            task["status"] = json!({"state":"TASK_STATE_WORKING", "timestamp":timestamp()});
            let result = match service.store.put(task.clone()).await {
                Ok(()) => {
                    updates.send_replace(task.clone());
                    service.execute(operation, &context, rx.clone()).await
                }
                Err(error) => Err(error),
            };
            let cancelled = *rx.borrow();
            task["status"] = json!({"state": if cancelled {"TASK_STATE_CANCELED"} else if result.is_ok() {"TASK_STATE_COMPLETED"} else {"TASK_STATE_FAILED"}, "timestamp": timestamp()});
            match result {
                Ok(value) => {
                    task["artifacts"] = json!([{"artifactId":new_id(), "name":"nl2sh_result", "parts":[{"data":value}]}])
                }
                Err(error) => {
                    task["status"]["message"] = json!({"messageId":new_id(), "role":"ROLE_AGENT", "parts":[{"text":crate::limits::truncate_text(&error.to_string(), 2000)}]})
                }
            }
            redact(&mut task, &secrets);
            if let Err(error) = service.store.put(task.clone()).await {
                // Oversize results are failures; never retain an apparently successful task.
                task.as_object_mut()
                    .map(|object| object.remove("artifacts"));
                task["status"] = json!({"state":"TASK_STATE_FAILED", "timestamp":timestamp(), "message":{"messageId":new_id(),"role":"ROLE_AGENT","parts":[{"text":crate::limits::truncate_text(&error.to_string(),2000)}]}});
                let _ = service.store.put(task.clone()).await;
            }
            service.active.lock().await.remove(&job_id);
            drop(permit);
            drop(service);
            updates.send_replace(task);
        });
        Ok(Ticket {
            updates: update_rx,
            cancel,
        })
    }
    async fn execute(
        &self,
        operation: Operation,
        context: &str,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<Value> {
        if *cancel.borrow() {
            bail!("task cancelled before execution");
        }
        match operation {
            Operation::Inspect => execution::inspect(&self.path).await,
            Operation::Tools => execution::tools(&self.path).await,
            Operation::Invoke { tool, arguments } => {
                execution::invoke_tool(&self.path, &tool, arguments, cancel).await
            }
            Operation::Ask { message } => {
                let session = format!(
                    "protocol-{}",
                    &format!("{:x}", Sha256::digest(context.as_bytes()))[..32]
                );
                let lock = {
                    let mut contexts = self.contexts.lock().await;
                    contexts.retain(|_, lock| lock.strong_count() > 0);
                    if let Some(lock) = contexts.get(&session).and_then(Weak::upgrade) {
                        lock
                    } else {
                        let lock = Arc::new(Mutex::new(()));
                        contexts.insert(session.clone(), Arc::downgrade(&lock));
                        lock
                    }
                };
                let _guard = tokio::select! {
                    biased;
                    _ = async { while !*cancel.borrow() { if cancel.changed().await.is_err() { break; } } } => bail!("task cancelled waiting for context"),
                    guard = lock.lock() => guard,
                };
                execution::ask(&self.path, &session, &message, cancel).await
            }
        }
    }
    pub async fn get(&self, id: &str) -> Result<Option<Value>> {
        validate_id(id)?;
        if let Some(active) = self.active.lock().await.get(id) {
            return Ok(Some(active.updates.borrow().clone()));
        }
        self.store.get(id).await
    }
    pub async fn all(&self) -> Result<Vec<Value>> {
        self.store.all().await
    }
    pub async fn cancel(&self, id: &str) -> Result<Option<Ticket>> {
        validate_id(id)?;
        let active = self.active.lock().await;
        Ok(active.get(id).map(|task| {
            task.cancel.send_replace(true);
            Ticket {
                updates: task.updates.clone(),
                cancel: task.cancel.clone(),
            }
        }))
    }
    pub async fn shutdown(&self) {
        self.stopping.store(true, Ordering::Release);
        let waits: Vec<_> = {
            let active = self.active.lock().await;
            active
                .values()
                .map(|task| {
                    task.cancel.send_replace(true);
                    Ticket {
                        updates: task.updates.clone(),
                        cancel: task.cancel.clone(),
                    }
                })
                .collect()
        };
        for ticket in waits {
            let _ = ticket.wait().await;
        }
    }
}
pub(super) fn validate_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 256 {
        bail!("identifier must contain 1–256 UTF-8 bytes");
    }
    Ok(())
}
pub(super) fn terminal(task: &Value) -> bool {
    !matches!(
        task["status"]["state"].as_str(),
        Some("TASK_STATE_SUBMITTED" | "TASK_STATE_WORKING")
    )
}
pub(super) fn result(task: &Value) -> Result<Value> {
    task["artifacts"][0]["parts"][0]["data"]
        .as_object()
        .map(|value| Value::Object(value.clone()))
        .or_else(|| {
            let value = &task["artifacts"][0]["parts"][0]["data"];
            (!value.is_null()).then(|| value.clone())
        })
        .context("task did not complete successfully; inspect task status")
}

fn redact(value: &mut Value, secrets: &[String]) {
    match value {
        Value::String(text) => {
            for secret in secrets.iter().filter(|secret| !secret.is_empty()) {
                *text = text.replace(secret, "[REDACTED]");
            }
        }
        Value::Array(items) => {
            for item in items {
                redact(item, secrets);
            }
        }
        Value::Object(items) => {
            for item in items.values_mut() {
                redact(item, secrets);
            }
        }
        _ => {}
    }
}
