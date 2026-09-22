use super::{ConversationItem, FinishReason, LlmRequest, LlmResponse, Role, ToolCall, Usage};
use anyhow::{Context, Result};
use serde_json::{json, Value};
pub fn request(req: &LlmRequest) -> Value {
    let mut input = Vec::new();
    for item in &req.items {
        match item {
            ConversationItem::Message(m) => {
                input.push(json!({"role":role(&m.role),"content":m.content}))
            }
            ConversationItem::Tools(round) => {
                input.extend(round.calls.iter().map(|call| {
                    json!({"type":"function_call","call_id":call.id,"name":call.name,"arguments":call.arguments.to_string()})
                }));
                for result in &round.results {
                    input.push(json!({"type":"function_call_output","call_id":result.call_id,"output":result.output}));
                    if !result.attachments.is_empty() {
                        let mut content = vec![
                            json!({"type":"input_text","text":format!("Image attachment produced by tool call {}.",result.call_id)}),
                        ];
                        content.extend(result.attachments.iter().map(|a| json!({"type":"input_image","image_url":format!("data:{};base64,{}",a.media_type,a.base64_data)})));
                        input.push(json!({"type":"message","role":"user","content":content}));
                    }
                }
            }
        }
    }
    let tools:Vec<Value>=req.tools.iter().map(|t|json!({"type":"function","name":t.name,"description":t.description,"parameters":t.parameters})).collect();
    let mut body = json!({"model":req.model,"input":input});
    if !tools.is_empty() {
        body["tools"] = json!(tools)
    }
    body
}
fn role(r: &Role) -> &'static str {
    match r {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "user",
    }
}
pub fn response(v: Value) -> Result<LlmResponse> {
    let output = v
        .get("output")
        .and_then(Value::as_array)
        .context("empty Responses output")?;
    let mut text_parts = Vec::new();
    let mut calls = Vec::new();
    for item in output {
        match item["type"].as_str().unwrap_or("") {
            "function_call" => {
                let a = item["arguments"].as_str().unwrap_or("{}");
                let id = item["call_id"]
                    .as_str()
                    .or_else(|| item["id"].as_str())
                    .filter(|value| !value.is_empty())
                    .context("function call is missing call_id")?;
                let name = item["name"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .context("function call is missing name")?;
                calls.push(ToolCall {
                    id: id.into(),
                    name: name.into(),
                    arguments: serde_json::from_str(a).context("invalid function arguments")?,
                });
            }
            "message" => {
                if let Some(content) = item["content"].as_array() {
                    for c in content {
                        if let Some(t) = c["text"].as_str() {
                            text_parts.push(t.to_owned())
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let text = if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join("\n"))
    };
    if text.as_deref().is_none_or(str::is_empty) && calls.is_empty() {
        anyhow::bail!("empty Responses output")
    }
    let finish = if calls.is_empty() {
        FinishReason::Stop
    } else {
        FinishReason::ToolCalls
    };
    Ok(LlmResponse {
        text,
        tool_calls: calls,
        usage: Usage {
            input_tokens: v["usage"]["input_tokens"].as_u64(),
            output_tokens: v["usage"]["output_tokens"].as_u64(),
        },
        finish_reason: finish,
    })
}

#[cfg(test)]
mod tests {
    use super::request;
    use crate::llm::{
        ConversationItem, LlmRequest, ToolAttachment, ToolCall, ToolResult, ToolRound,
    };
    use serde_json::json;

    #[test]
    fn image_tool_results_become_followup_input_image_content() {
        let body = request(&LlmRequest {
            model: "vision".into(),
            items: vec![ConversationItem::Tools(ToolRound {
                calls: vec![ToolCall {
                    id: "c".into(),
                    name: "view_screenshot".into(),
                    arguments: json!({}),
                }],
                results: vec![ToolResult {
                    call_id: "c".into(),
                    output: "attached".into(),
                    success: true,
                    attachments: vec![ToolAttachment {
                        media_type: "image/png".into(),
                        base64_data: "AA==".into(),
                    }],
                }],
            })],
            tools: Vec::new(),
        });
        assert_eq!(body["input"][1]["type"], "function_call_output");
        assert_eq!(body["input"][2]["type"], "message");
        assert_eq!(body["input"][2]["content"][1]["type"], "input_image");
    }
}
