//! MCP is a direct adapter over device execution, without A2A round trips.
use super::tasks::{self, Operation, Tasks, Ticket};
use anyhow::{bail, Result};
use rmcp::{model::*, service::RequestContext, ErrorData, RoleServer, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{borrow::Cow, sync::Arc};

#[derive(Clone)]
pub(super) struct Mcp {
    pub tasks: Arc<Tasks>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct InvokeArgs {
    tool: String,
    arguments: serde_json::Map<String, Value>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AskArgs {
    message: String,
    context_id: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TaskArgs {
    task_id: String,
}

fn definitions() -> Vec<Tool> {
    let empty = json!({"type":"object", "properties":{}, "additionalProperties":false});
    vec![
        Tool::new("nl2sh_inspect", "Read fixed device environment facts; no model required.", empty.as_object().cloned().unwrap_or_default()),
        Tool::new("nl2sh_tools", "List currently available device tools and their parameter schemas.", empty.as_object().cloned().unwrap_or_default()),
        Tool::new("nl2sh_invoke", "Invoke a registered device tool directly. Mutations require local approval unless protocol_auto_approve is explicitly enabled.", schemars::schema_for!(InvokeArgs).as_object().cloned().unwrap_or_default()),
        Tool::new("nl2sh_read_screen", "Read the Android screen as text and an MCP image; no model required.", empty.as_object().cloned().unwrap_or_default()),
        Tool::new("nl2sh_ask", "Delegate a task to the built-in Agent; requires a configured model. Reuse context_id to continue the conversation.", schemars::schema_for!(AskArgs).as_object().cloned().unwrap_or_default()),
        Tool::new("nl2sh_get_task", "Read a persisted device task, including its status and result.", schemars::schema_for!(TaskArgs).as_object().cloned().unwrap_or_default()),
        Tool::new("nl2sh_cancel_task", "Request cooperative cancellation and wait for the current operation to settle; cancellation does not undo changes.", schemars::schema_for!(TaskArgs).as_object().cloned().unwrap_or_default()),
    ]
}
impl ServerHandler for Mcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("nl2sh", env!("CARGO_PKG_VERSION")))
            .with_protocol_version(ProtocolVersion::V_2025_11_25)
            .with_instructions("Execution occurs on this device. Inspect success and failed_tools before claiming completion. Approval is device-local.")
    }
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(vec![
            ProtocolVersion::V_2025_11_25,
            ProtocolVersion::V_2025_06_18,
            ProtocolVersion::V_2024_11_05,
        ])
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        definitions().into_iter().find(|tool| tool.name == name)
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(definitions()))
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        let args = Value::Object(request.arguments.unwrap_or_default());
        if serde_json::to_vec(&args)
            .map_err(|_| ErrorData::invalid_params("invalid arguments", None))?
            .len()
            > 16 * 1024
        {
            return Err(ErrorData::invalid_params("arguments exceed 16 KiB", None));
        }
        match self.call(&request.name, args, &context).await {
            Ok(result) => Ok(result.into()),
            Err(error) => Ok(CallToolResult::error(vec![ContentBlock::text(
                crate::limits::truncate_text(&error.to_string(), 2000),
            )])
            .into()),
        }
    }
}
// Dropping a transport request signals cancellation but never drops execution futures.
struct CancelOnDrop(tokio::sync::watch::Sender<bool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}
impl Mcp {
    async fn wait(&self, ticket: Ticket, context: &RequestContext<RoleServer>) -> Result<Value> {
        let cancellation = CancelOnDrop(ticket.cancel.clone());
        let wait = ticket.wait();
        tokio::pin!(wait);
        let task = tokio::select! {
            result = &mut wait => result?,
            _ = context.ct.cancelled() => { cancellation.0.send_replace(true); wait.await? },
        };
        Ok(task)
    }
    async fn call(
        &self,
        name: &str,
        args: Value,
        context: &RequestContext<RoleServer>,
    ) -> Result<CallToolResult> {
        let operation = match name {
            "nl2sh_inspect" | "nl2sh_tools" | "nl2sh_read_screen" => {
                if args != json!({}) {
                    bail!("this tool takes no arguments");
                }
                match name {
                    "nl2sh_inspect" => Operation::Inspect,
                    "nl2sh_tools" => Operation::Tools,
                    _ => Operation::Invoke {
                        tool: "android.read_screen".into(),
                        arguments: json!({}),
                    },
                }
            }
            "nl2sh_invoke" => {
                let args: InvokeArgs = serde_json::from_value(args)?;
                Operation::Invoke {
                    tool: args.tool,
                    arguments: Value::Object(args.arguments),
                }
            }
            "nl2sh_ask" => {
                let args: AskArgs = serde_json::from_value(args)?;
                if args.message.trim().is_empty() || args.message.len() > 8192 {
                    bail!("message must contain 1–8192 UTF-8 bytes");
                }
                let ticket = self
                    .tasks
                    .submit(
                        Operation::Ask {
                            message: args.message,
                        },
                        args.context_id,
                        None,
                    )
                    .await?;
                return Ok(structured(self.wait(ticket, context).await?));
            }
            "nl2sh_get_task" => {
                let args: TaskArgs = serde_json::from_value(args)?;
                return Ok(structured(
                    self.tasks
                        .get(&args.task_id)
                        .await?
                        .ok_or_else(|| anyhow::anyhow!("task not found"))?,
                ));
            }
            "nl2sh_cancel_task" => {
                let args: TaskArgs = serde_json::from_value(args)?;
                let ticket = self
                    .tasks
                    .cancel(&args.task_id)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("task not found or already terminal"))?;
                return Ok(structured(ticket.wait().await?));
            }
            _ => bail!("unknown MCP tool"),
        };
        let task = self
            .wait(self.tasks.submit(operation, None, None).await?, context)
            .await?;
        if task["status"]["state"] != "TASK_STATE_COMPLETED" {
            return Ok(structured(task));
        }
        let value = tasks::result(&task)?;
        if name == "nl2sh_read_screen" {
            return screen_result(value);
        }
        Ok(structured(value))
    }
}
fn structured(value: Value) -> CallToolResult {
    let failed = value["success"] == false
        || matches!(
            value["status"]["state"].as_str(),
            Some("TASK_STATE_FAILED" | "TASK_STATE_CANCELED")
        )
        || value["artifacts"][0]["parts"][0]["data"]["failed_tools"]
            .as_array()
            .is_some_and(|items| !items.is_empty());
    let mut result = CallToolResult::success(vec![ContentBlock::text(value.to_string())]);
    result.structured_content = Some(value);
    result.is_error = Some(failed);
    result
}
pub(super) fn screen_result(value: Value) -> Result<CallToolResult> {
    if value["success"] != true {
        return Ok(structured(value));
    }
    let attachments = value["attachments"]
        .as_array()
        .filter(|items| items.len() == 1)
        .ok_or_else(|| anyhow::anyhow!("screen result must contain exactly one image"))?;
    let media = attachments[0]["media_type"].as_str().unwrap_or("");
    let data = attachments[0]["base64_data"].as_str().unwrap_or("");
    if !matches!(media, "image/png" | "image/jpeg")
        || data.is_empty()
        || data.len() > 3 * 1024 * 1024
    {
        bail!("unsupported or oversized screen image");
    }
    let mut result = CallToolResult::success(vec![
        ContentBlock::text(value["output"].as_str().unwrap_or("")),
        ContentBlock::image(data, media),
    ]);
    result.structured_content =
        Some(json!({"tool":value["tool"],"success":true,"output":value["output"]}));
    Ok(result)
}
