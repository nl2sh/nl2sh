//! A2A 1.0 JSON-RPC Agent delegation over the shared device task service.
use super::tasks::{validate_id, Operation, Tasks};
use axum::{extract::State, http::HeaderMap, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) struct RpcError {
    code: i32,
    message: String,
}
impl RpcError {
    fn params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }
    fn internal(_: anyhow::Error) -> Self {
        Self {
            code: -32603,
            message: "device task service failed".into(),
        }
    }
    fn missing() -> Self {
        Self {
            code: -32001,
            message: "task not found".into(),
        }
    }
    fn unsupported() -> Self {
        Self {
            code: -32004,
            message: "operation not supported".into(),
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendParams {
    message: Message,
    #[serde(default)]
    configuration: SendConfig,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendConfig {
    #[serde(default)]
    return_immediately: bool,
    history_length: Option<usize>,
    accepted_output_modes: Option<Vec<String>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    message_id: String,
    role: String,
    parts: Vec<Value>,
    context_id: Option<String>,
    task_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetParams {
    id: String,
    history_length: Option<usize>,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListParams {
    context_id: Option<String>,
    status: Option<String>,
    page_size: Option<usize>,
    page_token: Option<String>,
    history_length: Option<usize>,
    #[serde(default)]
    include_artifacts: bool,
    status_timestamp_after: Option<String>,
}

pub(super) async fn rpc(
    State(tasks): State<Arc<Tasks>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Json<Value> {
    let request: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return Json(error(
                Value::Null,
                RpcError {
                    code: -32700,
                    message: "invalid JSON payload".into(),
                },
            ))
        }
    };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    if !request.is_object()
        || request["jsonrpc"] != "2.0"
        || !request["method"].is_string()
        || !(id.is_string() || id.is_number())
    {
        return Json(error(
            id,
            RpcError {
                code: -32600,
                message: "invalid JSON-RPC request".into(),
            },
        ));
    }
    if headers
        .get("a2a-version")
        .is_some_and(|value| value != "1.0")
    {
        return Json(error(
            id,
            RpcError {
                code: -32009,
                message: "unsupported A2A version; supported: 1.0".into(),
            },
        ));
    }
    let method = request["method"].as_str().unwrap_or("");
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    let result = dispatch(&tasks, method, params).await;
    Json(match result {
        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
        Err(err) => error(id, err),
    })
}
fn error(id: Value, err: RpcError) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":err.code,"message":err.message}})
}
fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, RpcError> {
    serde_json::from_value(value).map_err(|_| RpcError::params("invalid parameters"))
}
fn check_id(id: &str) -> Result<(), RpcError> {
    validate_id(id).map_err(|_| RpcError::params("invalid identifier"))
}
fn trim_history(task: &mut Value, length: Option<usize>) {
    if let (Some(length), Some(history)) = (length, task["history"].as_array_mut()) {
        if history.len() > length {
            history.drain(..history.len() - length);
        }
    }
}
async fn dispatch(tasks: &Arc<Tasks>, method: &str, params: Value) -> Result<Value, RpcError> {
    match method {
        "SendMessage" => {
            let params: SendParams = parse(params)?;
            check_id(&params.message.message_id)?;
            if params.message.role != "ROLE_USER" || params.message.parts.is_empty() {
                return Err(RpcError::params("expected a user message with text parts"));
            }
            if params.message.task_id.is_some() {
                return Err(RpcError::unsupported());
            }
            if let Some(context) = &params.message.context_id {
                check_id(context)?;
            }
            if params
                .configuration
                .accepted_output_modes
                .as_ref()
                .is_some_and(|modes| {
                    !modes.is_empty() && !modes.iter().any(|mode| mode == "application/json")
                })
            {
                return Err(RpcError {
                    code: -32005,
                    message: "output mode not supported; use application/json".into(),
                });
            }
            let mut text = String::new();
            for part in &params.message.parts {
                if !part.is_object()
                    || part.get("data").is_some()
                    || part.get("file").is_some()
                    || part.get("url").is_some()
                    || part.get("raw").is_some()
                {
                    return Err(RpcError {
                        code: -32005,
                        message: "only text input is supported".into(),
                    });
                }
                let fragment = part["text"]
                    .as_str()
                    .ok_or_else(|| RpcError::params("missing text part"))?;
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(fragment);
            }
            if text.trim().is_empty() || text.len() > 8192 {
                return Err(RpcError::params("message must contain 1–8192 UTF-8 bytes"));
            }
            let message = json!({"messageId":params.message.message_id,"role":"ROLE_USER","parts":[{"text":text}]});
            let ticket = tasks
                .submit(
                    Operation::Ask { message: text },
                    params.message.context_id,
                    Some(message),
                )
                .await
                .map_err(RpcError::internal)?;
            let mut task = if params.configuration.return_immediately {
                ticket.updates.borrow().clone()
            } else {
                ticket.wait().await.map_err(RpcError::internal)?
            };
            trim_history(&mut task, params.configuration.history_length);
            Ok(json!({"task":task}))
        }
        "GetTask" => {
            let params: GetParams = parse(params)?;
            check_id(&params.id)?;
            let mut task = tasks
                .get(&params.id)
                .await
                .map_err(RpcError::internal)?
                .ok_or_else(RpcError::missing)?;
            trim_history(&mut task, params.history_length);
            Ok(task)
        }
        "CancelTask" => {
            let params: GetParams = parse(params)?;
            check_id(&params.id)?;
            if let Some(ticket) = tasks.cancel(&params.id).await.map_err(RpcError::internal)? {
                return ticket.wait().await.map_err(RpcError::internal);
            }
            if tasks
                .get(&params.id)
                .await
                .map_err(RpcError::internal)?
                .is_none()
            {
                Err(RpcError::missing())
            } else {
                Err(RpcError {
                    code: -32002,
                    message: "task is not cancelable".into(),
                })
            }
        }
        "ListTasks" => list(tasks, parse(params)?).await,
        "SendStreamingMessage"
        | "SubscribeToTask"
        | "CreateTaskPushNotificationConfig"
        | "GetTaskPushNotificationConfig"
        | "ListTaskPushNotificationConfigs"
        | "DeleteTaskPushNotificationConfig"
        | "GetExtendedAgentCard" => Err(RpcError::unsupported()),
        _ => Err(RpcError {
            code: -32601,
            message: "method not found".into(),
        }),
    }
}
async fn list(tasks: &Tasks, params: ListParams) -> Result<Value, RpcError> {
    let size = params.page_size.unwrap_or(50);
    if !(1..=100).contains(&size) {
        return Err(RpcError::params("pageSize must be 1–100"));
    }
    if let Some(context) = &params.context_id {
        check_id(context)?;
    }
    if params.status.as_ref().is_some_and(|state| {
        !matches!(
            state.as_str(),
            "TASK_STATE_SUBMITTED"
                | "TASK_STATE_WORKING"
                | "TASK_STATE_COMPLETED"
                | "TASK_STATE_FAILED"
                | "TASK_STATE_CANCELED"
                | "TASK_STATE_REJECTED"
                | "TASK_STATE_INPUT_REQUIRED"
                | "TASK_STATE_AUTH_REQUIRED"
        )
    }) {
        return Err(RpcError::params("invalid task status"));
    }
    let after = params
        .status_timestamp_after
        .as_ref()
        .map(|value| {
            time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        })
        .transpose()
        .map_err(|_| RpcError::params("invalid timestamp"))?;
    let cursor = params
        .page_token
        .as_deref()
        .filter(|token| !token.is_empty())
        .map(|token| {
            if token.len() > 512 {
                return Err(RpcError::params("invalid pageToken"));
            }
            let (stamp, id) = token
                .split_once('|')
                .ok_or_else(|| RpcError::params("invalid pageToken"))?;
            time::OffsetDateTime::parse(stamp, &time::format_description::well_known::Rfc3339)
                .map_err(|_| RpcError::params("invalid pageToken"))?;
            check_id(id)?;
            Ok((stamp.to_owned(), id.to_owned()))
        })
        .transpose()?;
    let all: Vec<_> = tasks
        .all()
        .await
        .map_err(RpcError::internal)?
        .into_iter()
        .filter(|task| {
            params
                .context_id
                .as_ref()
                .is_none_or(|context| task["contextId"] == *context)
                && params
                    .status
                    .as_ref()
                    .is_none_or(|state| task["status"]["state"] == *state)
                && after.is_none_or(|after| {
                    task["status"]["timestamp"]
                        .as_str()
                        .and_then(|stamp| {
                            time::OffsetDateTime::parse(
                                stamp,
                                &time::format_description::well_known::Rfc3339,
                            )
                            .ok()
                        })
                        .is_some_and(|stamp| stamp >= after)
                })
        })
        .collect();
    let total = all.len();
    let mut page: Vec<_> = all
        .into_iter()
        .filter(|task| {
            cursor.as_ref().is_none_or(|(stamp, id)| {
                (
                    task["status"]["timestamp"].as_str().unwrap_or(""),
                    task["id"].as_str().unwrap_or(""),
                ) < (stamp.as_str(), id.as_str())
            })
        })
        .take(size + 1)
        .collect();
    let more = page.len() > size;
    page.truncate(size);
    let next = if more {
        page.last()
            .map(|task| {
                format!(
                    "{}|{}",
                    task["status"]["timestamp"].as_str().unwrap_or(""),
                    task["id"].as_str().unwrap_or("")
                )
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    for task in &mut page {
        trim_history(task, params.history_length);
        if !params.include_artifacts {
            task.as_object_mut()
                .map(|object| object.remove("artifacts"));
        }
    }
    Ok(json!({"tasks":page,"totalSize":total,"pageSize":size,"nextPageToken":next}))
}
pub(super) fn card(origin: &str) -> Value {
    json!({"name":"nl2sh", "description":"Device-native Android Agent with local safety and approval", "version":env!("CARGO_PKG_VERSION"), "supportedInterfaces":[{"url":format!("{origin}/a2a"), "protocolBinding":"JSONRPC", "protocolVersion":"1.0"}], "capabilities":{"streaming":false,"pushNotifications":false,"extendedAgentCard":false}, "defaultInputModes":["text/plain"],"defaultOutputModes":["application/json"],"skills":[{"id":"device_agent","name":"Device Agent","description":"Delegate a task to the device Agent. Model configuration is required; mutations require device-local approval.","tags":["android","agent"]}],"securitySchemes":{"bearer":{"httpAuthSecurityScheme":{"scheme":"bearer"}}},"securityRequirements":[{"schemes":{"bearer":{"list":[]}}}]})
}
