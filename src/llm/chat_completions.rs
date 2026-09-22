use super::{ConversationItem, FinishReason, LlmRequest, LlmResponse, Role, ToolCall, Usage};
use anyhow::{Context, Result};
use serde_json::{json, Value};

pub fn request(req: &LlmRequest) -> Value {
    let mut messages = Vec::new();
    for item in &req.items {
        match item {
            ConversationItem::Message(m) => messages.push(
                json!({"role":role(&m.role),"content":m.content,"tool_call_id":m.tool_call_id}),
            ),
            ConversationItem::Tools(round) => {
                messages.push(json!({
                    "role": "assistant", "content": null,
                    "tool_calls": round.calls.iter().map(|call| json!({
                        "id": call.id, "type": "function",
                        "function": {"name": call.name, "arguments": call.arguments.to_string()}
                    })).collect::<Vec<_>>()
                }));
                for result in &round.results {
                    messages.push(json!({"role":"tool","tool_call_id":result.call_id,"content":result.output}));
                    if !result.attachments.is_empty() {
                        let mut content = vec![
                            json!({"type":"text","text":format!("Image attachment produced by tool call {}.",result.call_id)}),
                        ];
                        content.extend(result.attachments.iter().map(|a| json!({"type":"image_url","image_url":{"url":format!("data:{};base64,{}",a.media_type,a.base64_data)}})));
                        messages.push(json!({"role":"user","content":content}));
                    }
                }
            }
        }
    }
    let tools:Vec<Value>=req.tools.iter().map(|t|json!({"type":"function","function":{"name":t.name,"description":t.description,"parameters":t.parameters}})).collect();
    let mut body = json!({"model":req.model,"messages":messages});
    if !tools.is_empty() {
        body["tools"] = json!(tools);
        body["tool_choice"] = json!("auto")
    }
    body
}
fn role(r: &Role) -> &'static str {
    match r {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}
pub fn response(v: Value) -> Result<LlmResponse> {
    let choice = v
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|x| x.first())
        .context("empty Chat Completions response")?;
    let msg = &choice["message"];
    let calls = msg
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|c| {
                    let args = c["function"]["arguments"].as_str().unwrap_or("{}");
                    let id = c["id"]
                        .as_str()
                        .filter(|value| !value.is_empty())
                        .context("tool call is missing id")?;
                    let name = c["function"]["name"]
                        .as_str()
                        .filter(|value| !value.is_empty())
                        .context("tool call is missing function name")?;
                    Ok(ToolCall {
                        id: id.into(),
                        name: name.into(),
                        arguments: serde_json::from_str(args).context("invalid tool arguments")?,
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let reason = match choice["finish_reason"].as_str().unwrap_or("stop") {
        "stop" => FinishReason::Stop,
        "tool_calls" => FinishReason::ToolCalls,
        "length" => FinishReason::Length,
        x => FinishReason::Other(x.into()),
    };
    let text = msg["content"].as_str().map(str::to_owned);
    if text.as_deref().is_none_or(str::is_empty) && calls.is_empty() {
        anyhow::bail!("empty Chat Completions message")
    }
    Ok(LlmResponse {
        text,
        tool_calls: calls,
        usage: Usage {
            input_tokens: v["usage"]["prompt_tokens"].as_u64(),
            output_tokens: v["usage"]["completion_tokens"].as_u64(),
        },
        finish_reason: reason,
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
    fn image_tool_results_become_followup_user_image_content() {
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
        assert_eq!(body["messages"][1]["role"], "tool");
        assert_eq!(body["messages"][2]["role"], "user");
        assert_eq!(body["messages"][2]["content"][1]["type"], "image_url");
    }
}
