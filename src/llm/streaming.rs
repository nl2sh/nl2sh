use super::{responses, FinishReason, LlmResponse, TextDeltaSink, ToolCall, Usage};
use crate::config::ApiType;
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Default)]
struct ChatToolCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
struct ChatStream {
    text: String,
    calls: BTreeMap<usize, ChatToolCall>,
    finish: Option<FinishReason>,
    usage: Usage,
}

pub(super) async fn parse(
    mut response: reqwest::Response,
    api_type: ApiType,
    sink: &dyn TextDeltaSink,
) -> Result<LlmResponse> {
    let mut buffer = Vec::new();
    let mut chat = ChatStream::default();
    let mut responses_final = None;
    loop {
        let chunk = tokio::select! {
            chunk = response.chunk() => chunk.context("failed to read LLM stream")?,
            signal = tokio::signal::ctrl_c() => {
                signal?;
                bail!("LLM response cancelled by user");
            }
        };
        let Some(chunk) = chunk else { break };
        buffer.extend_from_slice(&chunk);
        while let Some((end, delimiter_len)) = event_boundary(&buffer) {
            let event = std::str::from_utf8(&buffer[..end])
                .context("LLM stream event was not UTF-8")?
                .to_owned();
            buffer.drain(..end + delimiter_len);
            handle_event(&event, api_type, sink, &mut chat, &mut responses_final)?;
        }
    }
    if !buffer.iter().all(u8::is_ascii_whitespace) {
        let event = std::str::from_utf8(&buffer).context("LLM stream event was not UTF-8")?;
        handle_event(event, api_type, sink, &mut chat, &mut responses_final)?;
    }
    match api_type {
        ApiType::Auto => bail!("automatic API negotiation was not resolved"),
        ApiType::ChatCompletions => chat.finish(),
        ApiType::Responses => responses_final.context("Responses stream ended before completion"),
    }
}

fn event_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2))
        .or_else(|| {
            buffer
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|index| (index, 4))
        })
}

fn handle_event(
    event: &str,
    api_type: ApiType,
    sink: &dyn TextDeltaSink,
    chat: &mut ChatStream,
    responses_final: &mut Option<LlmResponse>,
) -> Result<()> {
    let data = event
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    let value: Value = serde_json::from_str(&data).context("invalid LLM stream event")?;
    match api_type {
        ApiType::Auto => bail!("automatic API negotiation was not resolved"),
        ApiType::ChatCompletions => chat.push(&value, sink),
        ApiType::Responses => {
            if value["type"] == "response.output_text.delta" {
                if let Some(delta) = value["delta"].as_str() {
                    sink.delta(delta);
                }
            } else if value["type"] == "response.completed" {
                *responses_final = Some(responses::response(value["response"].clone())?);
            } else if value["type"] == "error" {
                bail!("Responses stream error: {}", value["message"]);
            }
            Ok(())
        }
    }
}

impl ChatStream {
    fn push(&mut self, value: &Value, sink: &dyn TextDeltaSink) -> Result<()> {
        if let Some(input) = value["usage"]["prompt_tokens"].as_u64() {
            self.usage.input_tokens = Some(input);
        }
        if let Some(output) = value["usage"]["completion_tokens"].as_u64() {
            self.usage.output_tokens = Some(output);
        }
        let Some(choice) = value["choices"]
            .as_array()
            .and_then(|choices| choices.first())
        else {
            return Ok(());
        };
        let delta = &choice["delta"];
        if let Some(text) = delta["content"].as_str() {
            self.text.push_str(text);
            sink.delta(text);
        }
        if let Some(calls) = delta["tool_calls"].as_array() {
            for call in calls {
                let index = call["index"].as_u64().unwrap_or(0) as usize;
                let target = self.calls.entry(index).or_default();
                if let Some(id) = call["id"].as_str() {
                    target.id.push_str(id);
                }
                if let Some(name) = call["function"]["name"].as_str() {
                    target.name.push_str(name);
                }
                if let Some(arguments) = call["function"]["arguments"].as_str() {
                    target.arguments.push_str(arguments);
                }
            }
        }
        if let Some(reason) = choice["finish_reason"].as_str() {
            self.finish = Some(match reason {
                "stop" => FinishReason::Stop,
                "tool_calls" => FinishReason::ToolCalls,
                "length" => FinishReason::Length,
                other => FinishReason::Other(other.into()),
            });
        }
        Ok(())
    }

    fn finish(self) -> Result<LlmResponse> {
        let calls = self
            .calls
            .into_values()
            .map(|call| {
                if call.id.is_empty() || call.name.is_empty() {
                    bail!("incomplete streamed tool call");
                }
                Ok(ToolCall {
                    id: call.id,
                    name: call.name,
                    arguments: serde_json::from_str(if call.arguments.is_empty() {
                        "{}"
                    } else {
                        &call.arguments
                    })
                    .context("invalid streamed tool arguments")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let text = (!self.text.is_empty()).then_some(self.text);
        if text.is_none() && calls.is_empty() {
            bail!("empty Chat Completions stream");
        }
        Ok(LlmResponse {
            text,
            finish_reason: self.finish.unwrap_or({
                if calls.is_empty() {
                    FinishReason::Stop
                } else {
                    FinishReason::ToolCalls
                }
            }),
            tool_calls: calls,
            usage: self.usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Sink(Mutex<String>);
    impl TextDeltaSink for Sink {
        fn delta(&self, text: &str) {
            if let Ok(mut value) = self.0.lock() {
                value.push_str(text);
            }
        }
    }

    #[test]
    fn chat_stream_accumulates_text_and_fragmented_tool_arguments() -> Result<()> {
        let sink = Sink::default();
        let mut stream = ChatStream::default();
        stream.push(
            &serde_json::json!({"choices":[{"delta":{"content":"你"}}]}),
            &sink,
        )?;
        stream.push(&serde_json::json!({"choices":[{"delta":{"content":"好","tool_calls":[{"index":0,"id":"call_1","function":{"name":"execute_shell_command","arguments":"{\"command\":"}}]}}]}), &sink)?;
        stream.push(&serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"id\"}"}}]},"finish_reason":"tool_calls"}]}), &sink)?;
        let result = stream.finish()?;
        assert_eq!(result.text.as_deref(), Some("你好"));
        assert_eq!(result.tool_calls[0].arguments["command"], "id");
        assert_eq!(*sink.0.lock().map_err(|_| anyhow::anyhow!("lock"))?, "你好");
        Ok(())
    }

    #[test]
    fn detects_lf_and_crlf_event_boundaries() {
        assert_eq!(event_boundary(b"data: {}\n\nnext"), Some((8, 2)));
        assert_eq!(event_boundary(b"data: {}\r\n\r\nnext"), Some((8, 4)));
    }
}
